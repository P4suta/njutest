// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Starts one child process, supervises the platform's declared process set, and returns what happened.

mod cancel;
mod clock;
mod event;
mod group;
pub mod output;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::observation::{Event, Observation, Signal, WaitNote};
use output::{OutputError, TailBuffer};
use rust_mutants_decision::answered::{Answer, Observed, Wait};
use rust_mutants_decision::stall::Stillness;

pub use clock::Clock;
pub use group::GroupChild;
#[cfg(unix)]
pub use group::Leader;
pub use output::{DEFAULT_OUTPUT_LIMIT, HeadBuffer, MIN_OUTPUT_LIMIT, OUTPUT_TRUNCATED_PREFIX};

/// The conventional stand-in used only by legacy report projections when there is no exit status to report.
pub const EXIT_CODE_UNAVAILABLE: i32 = -1;

/// How much stdout a short probe may retain: version banners and one-line paths are bounded well below this.
pub const PROBE_OUTPUT_LIMIT: usize = 64 * 1024;

/// The containment guarantee the platform supervisor can actually provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum SupervisionBoundary {
    /// POSIX process-group inheritance: every retained member must have a confirmed exit before the leader is reaped, while a process that deliberately leaves with `setsid` or `setpgid` needs an explicit escaped-process owner.
    InheritedProcessGroup,
    /// An operating-system container that descendants cannot leave on their own.
    ContainedTree,
}

/// How long a POSIX process group is given to shut down after SIGTERM before it is sent SIGKILL.
/// Windows has no equivalent phase.
pub const TERMINATION_GRACE: Duration = Duration::from_secs(2);

/// How long [`run`] waits for the output pipe to reach EOF after the child itself has exited.
pub const IO_DRAIN_GRACE: Duration = Duration::from_secs(2);

pub use cancel::{Cancel, Cancelled};

/// How long a child may run before this process stops it.
///
/// A required argument rather than a field with a default, because a wait nobody bounded is a wait that can be forever: five commands here asked a tool for its version with no bound at all, and one of them held a Windows runner for forty minutes until the job's own timeout killed it.
/// There is no value of this that can be reached by forgetting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// Stopped after this long, and reported as [`Termination::TimedOut`].
    After(Duration),
    /// Never stopped by this process, which is a claim that something else ends it.
    Unbounded,
}

/// How long a tool asked a question it already knows the answer to may take.
///
/// A version banner, a path the compiler prints, a line from git: none of them does work, so a minute is already an answer of its own.
pub const PROBE: Duration = Duration::from_secs(60);

impl Bound {
    /// The bound as the runner holds it.
    #[must_use]
    pub const fn timeout(self) -> Option<Duration> {
        match self {
            Self::After(bound) => Some(bound),
            Self::Unbounded => None,
        }
    }
}

/// One process to run.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Spec {
    /// The argument vector, executable first.
    /// Each element becomes exactly one argument to the child.
    /// A bare program name is resolved through `PATH`; anything with a separator is used as given.
    pub argv: Vec<OsString>,
    /// The child's working directory.
    /// `None` means this process's directory.
    pub dir: Option<PathBuf>,
    /// The child's complete environment, but for [`PRESENTATION`], which the runner sets over it.
    /// `None` inherits this process's environment, which is convenient for one-shot probes; the engine composes the full set explicitly for mutant executions.
    pub env: Option<crate::vars::Variables>,
    /// Bounds the child's wall-clock run time.
    /// `None` means no timeout.
    pub timeout: Option<Duration>,
    /// Caps the retained combined output in bytes.
    /// `None` selects [`DEFAULT_OUTPUT_LIMIT`]; anything below [`MIN_OUTPUT_LIMIT`] is raised to it so the truncation notice still fits inside the budget.
    pub output_limit: Option<usize>,
    /// Captures stdout on its own, head-capped at this many bytes, for a child that writes structured data (JSON lines) to stdout and chatter to stderr — `cargo metadata`, `cargo check --message-format=json`.
    /// `None` merges stdout into [`RunResult::output`] with stderr.
    pub structured_stdout: Option<usize>,
    /// A private side-channel file whose appearance asks the supervisor to stop the declared process set.
    /// The execution layer validates its contents before drawing any conclusion.
    pub(crate) stop_file: Option<PathBuf>,
    /// A private side-channel file the child rewrites as it makes progress, which turns [`Spec::timeout`] into a ceiling and ends the run early only when the file stays unchanged for a whole quiet window.
    pub(crate) progress: Option<Progress>,
    /// Where the process that leads this run is recorded the moment it starts, so a child it leaves is known to be its own before anything else looks.
    pub(crate) leaders: Option<crate::orphan::Leaders>,
    /// Whether the run ends at the first line in which libtest says a test failed, because that one failure is the whole answer the caller wants.
    pub(crate) stop_at_first_failure: bool,
    /// Test-only terminal ownership fault selected explicitly by the composition root.
    reaping: Reaping,
}

impl Spec {
    /// A spec for `argv`, bounded as `bound` says, with every other default.
    #[must_use]
    pub fn new<I, S>(argv: I, bound: Bound) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            dir: None,
            env: None,
            timeout: bound.timeout(),
            output_limit: None,
            structured_stdout: None,
            stop_file: None,
            progress: None,
            leaders: None,
            stop_at_first_failure: false,
            reaping: Reaping::Normal,
        }
    }

    /// Makes this test process exercise the bounded terminal ownership failure.
    #[cfg(any(test, feature = "testkit"))]
    pub const fn simulate_unreapable_child(&mut self) {
        self.reaping = Reaping::SimulatedUnreapable;
    }
}

/// Files whose content changes whenever the child makes progress, and how long they may all go unchanged.
#[derive(Debug, Clone)]
pub(crate) struct Progress {
    /// The file the child rewrites whenever it takes a reservation of its count.
    pub(crate) path: PathBuf,
    /// The file the child rewrites while it spends a reservation, at most [`Progress::beat_every`] apart.
    pub(crate) beat: PathBuf,
    /// How long both files may stay unchanged before the run is [`Termination::Stalled`].
    pub(crate) quiet: Duration,
}

impl Progress {
    /// How long the child may spend a reservation before it rewrites [`Progress::beat`], as [`rust_mutants_decision::stall::beat_every`] decides.
    pub(crate) fn beat_every(&self) -> Duration {
        rust_mutants_decision::stall::beat_every(self.quiet)
    }

    /// What the child is told so that it is never quiet for a window while it moves, set over its environment by the runner that watches it.
    pub(crate) fn told(&self) -> (&'static str, OsString) {
        let mut value = OsString::from(format!("{}@", self.beat_every().as_millis()));
        value.push(self.beat.as_os_str());
        (crate::instrument::STEP_BEAT_ENV, value)
    }

    /// Every file a change to which is the child moving.
    fn signals(&self) -> [&Path; 2] {
        [&self.path, &self.beat]
    }
}

#[derive(Debug, Clone, Copy)]
enum Reaping {
    Normal,
    #[cfg(any(test, feature = "testkit"))]
    SimulatedUnreapable,
}

/// A failure to start or supervise a process — never a process that ran and failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunnerError {
    /// The platform's declared process set could not be placed under supervision.
    /// Always fatal to the run.
    #[error("could not supervise the child process set: {message}")]
    SupervisionUnavailable {
        /// What failed.
        message: String,
        /// The underlying failure, if any.
        #[source]
        source: Option<io::Error>,
    },
    /// The child could not be started at all.
    #[error("could not start {program:?}: {source}")]
    ProcessStartFailed {
        /// The program that was asked for.
        program: OsString,
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// The spec cannot describe a process: an empty or blank argument vector.
    #[error("the command {message}")]
    SpecInvalid {
        /// What is wrong.
        message: &'static str,
    },
    /// The child was started and supervised but the operating system refused to say how it ended, which leaves the exit code untrustworthy.
    #[error("could not collect the child process's exit status: {source}")]
    ProcessWaitFailed {
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// A child output pipe could not be read exactly.
    #[error("could not read the child process's output: {source}")]
    OutputReadFailed {
        /// The pipe read failure.
        #[source]
        source: io::Error,
    },
    /// A bounded child-output buffer could not preserve its invariants.
    #[error("could not capture the child process's output: {source}")]
    OutputCaptureFailed {
        /// The bounded-buffer failure.
        #[source]
        source: OutputError,
    },
    /// A child output reader did not close after the supervised process ended.
    #[error("the child process's {stream} pipe did not close within the drain bound")]
    OutputDrainTimedOut {
        /// Which capture did not reach EOF.
        stream: &'static str,
    },
    /// A child output reader ended without reporting whether the pipe was read exactly.
    #[error("the child process's {stream} reader ended without a result")]
    OutputReaderDisconnected {
        /// Which capture lost its reader.
        stream: &'static str,
    },
    /// A child output reader thread could not be created.
    #[error("could not start the child process's {stream} reader: {source}")]
    OutputReaderStartFailed {
        /// Which capture could not start.
        stream: &'static str,
        /// The operating system's reason.
        #[source]
        source: io::Error,
    },
    /// A child output reader panicked before its owner joined it.
    #[error("the child process's {stream} reader panicked")]
    OutputReaderPanicked {
        /// Which capture panicked.
        stream: &'static str,
    },
    /// A child output reader owner no longer held the join handle it must consume.
    #[error("the child process's {stream} reader lost its owned join handle")]
    OutputReaderOwnershipLost {
        /// Which capture lost ownership.
        stream: &'static str,
    },
    /// A child output pipe could not be configured for bounded, cancellable reads.
    #[error("could not configure the child process's {stream} pipe: {source}")]
    OutputReaderConfigurationFailed {
        /// Which capture could not be configured.
        stream: &'static str,
        /// The platform failure.
        #[source]
        source: io::Error,
    },
    /// The configured wall-clock duration cannot be represented as a deadline.
    #[error("the child process timeout {timeout:?} cannot be represented as a deadline")]
    DeadlineOverflow {
        /// The configured finite timeout.
        timeout: Duration,
    },
    /// The supervisor could not send a termination request to the supervised process set.
    #[error("could not perform {phase} termination of the supervised process set: {source}")]
    ProcessControlFailed {
        /// Which bounded termination phase failed.
        phase: TerminationPhase,
        /// The platform failure.
        #[source]
        source: io::Error,
    },
    /// Both the cooperative and forceful process-set controls failed.
    /// Both failures are retained because either can explain members remaining in the declared process set.
    #[error(
        "could not terminate the supervised process set: gentle control failed: {gentle}; forceful control failed: {forceful}"
    )]
    ProcessControlSequenceFailed {
        /// The cooperative-control failure.
        gentle: io::Error,
        /// The forceful-control failure.
        forceful: io::Error,
    },
    /// The platform supervisor could not release its operating-system resource after the process was reaped or explicitly abandoned.
    #[error("could not release the child process supervisor: {source}")]
    SupervisorReleaseFailed {
        /// The platform failure.
        #[source]
        source: io::Error,
    },
    /// The reported first-failure ending contradicts the failure and process ending observed.
    #[error("the answered-stop decision contradicts the observed failure and process ending")]
    AnsweredStopInconsistent,
}

/// Which bounded process-set termination phase an operating-system failure interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum TerminationPhase {
    /// The cooperative termination request before the grace period.
    Gentle,
    /// The forceful termination request after the grace period.
    Forceful,
}

impl std::fmt::Display for TerminationPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Gentle => "gentle",
            Self::Forceful => "forceful",
        })
    }
}

/// Why an execution-specific monitor could not establish whether its stop request existed.
#[derive(Debug, thiserror::Error)]
pub enum MonitorError {
    /// The side-channel path existed but was not a regular file.
    #[error("the execution monitor path {path} is not a regular file")]
    InvalidType {
        /// The path inspected.
        path: PathBuf,
    },
    /// The side-channel path could not be inspected.
    #[error("could not inspect execution monitor path {path}: {source}")]
    Inspect {
        /// The path inspected.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: io::Error,
    },
}

/// How a process that exited by itself did so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExit {
    /// The process returned this status code.
    Code(i32),
    /// The process was ended by this signal.
    /// Only POSIX platforms produce it.
    Signal(i32),
    /// The operating system returned a status that exposed neither a code nor a signal.
    Unknown,
}

impl ProcessExit {
    /// Whether the process ended of a signal it raised by what it did, which a mutation can make it do, rather than one sent from outside.
    #[must_use]
    #[cfg_attr(
        windows,
        expect(
            clippy::missing_const_for_fn,
            reason = "only a POSIX signal can be one the process raised itself, so on Windows every arm is a constant, and on unix the answer reads a signal set"
        )
    )]
    pub fn raised_by_itself(self) -> bool {
        match self {
            #[cfg(unix)]
            Self::Signal(signal) => sys::raised_by_itself(signal),
            #[cfg(windows)]
            Self::Signal(_) => false,
            Self::Code(_) | Self::Unknown => false,
        }
    }
}

/// How an exit is spelled in a recording, in both directions, so an exit the runner can report is one a reader can read.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ProcessExitWire {
    Code { value: i32 },
    Signal { value: i32 },
    Unknown {},
}

impl serde::Serialize for ProcessExit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match *self {
            Self::Code(value) => ProcessExitWire::Code { value },
            Self::Signal(value) => ProcessExitWire::Signal { value },
            Self::Unknown => ProcessExitWire::Unknown {},
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ProcessExit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            match <ProcessExitWire as serde::Deserialize>::deserialize(deserializer)? {
                ProcessExitWire::Code { value } => Self::Code(value),
                ProcessExitWire::Signal { value } => Self::Signal(value),
                ProcessExitWire::Unknown {} => Self::Unknown,
            },
        )
    }
}

impl ProcessExit {
    /// The process's own status code, absent for a signal or an unclassified status.
    #[must_use]
    pub const fn code(self) -> Option<i32> {
        match self {
            Self::Code(code) => Some(code),
            Self::Signal(_) | Self::Unknown => None,
        }
    }

    /// The terminating signal, on a platform that has one.
    #[must_use]
    pub const fn signal(self) -> Option<i32> {
        match self {
            Self::Signal(signal) => Some(signal),
            Self::Code(_) | Self::Unknown => None,
        }
    }

    /// The shell-compatible status used at legacy presentation boundaries.
    #[must_use]
    pub const fn conventional_code(self) -> Option<i32> {
        match self {
            Self::Code(code) => Some(code),
            Self::Signal(signal) => Some(128i32.saturating_add(signal)),
            Self::Unknown => None,
        }
    }
}

/// The one way a supervised process ended.
#[derive(Debug)]
pub enum Termination {
    /// No program ran: the specification, launch, or initial supervision failed.
    NotStarted {
        /// Why nothing ran.
        error: RunnerError,
    },
    /// The process ended on its own.
    Exited(ProcessExit),
    /// The configured wall-clock bound expired and the supervised process set was ended.
    TimedOut,
    /// The progress file stayed unchanged for the whole quiet window and the supervised process set was ended.
    Stalled,
    /// An execution-specific monitor observed its stop request and the supervised process set was ended.
    StoppedByMonitor,
    /// The harness said a test failed, which was the whole answer asked for, and the supervised process set was ended.
    Answered,
    /// The execution-specific monitor could not establish whether a valid stop request existed.
    MonitorFailed {
        /// Why the monitor could not be trusted.
        failure: MonitorError,
    },
    /// The caller asked the run to stop.
    Cancelled {
        /// Whether a child had been started before cancellation was observed.
        started: bool,
    },
    /// A child was started, but the operating system did not yield its final status.
    WaitFailed {
        /// Why its status could not be collected.
        error: RunnerError,
    },
}

/// A borrowed, closed view of the two failure domains a supervised run can expose.
#[derive(Debug, Clone, Copy)]
pub enum RunFailure<'a> {
    /// Launch, supervision, or exit-status collection failed.
    Runner(&'a RunnerError),
    /// The execution-specific monitor could not be inspected safely.
    Monitor(&'a MonitorError),
}

impl std::fmt::Display for RunFailure<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runner(error) => std::fmt::Display::fmt(error, formatter),
            Self::Monitor(failure) => std::fmt::Display::fmt(failure, formatter),
        }
    }
}

impl Termination {
    /// The process's conventional status code, where a process supplied one.
    #[must_use]
    pub const fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Exited(exit) => exit.conventional_code(),
            Self::NotStarted { .. }
            | Self::TimedOut
            | Self::Stalled
            | Self::StoppedByMonitor
            | Self::Answered
            | Self::Cancelled { .. }
            | Self::WaitFailed { .. }
            | Self::MonitorFailed { .. } => None,
        }
    }

    /// The failure that prevented a trustworthy process result.
    #[must_use]
    pub const fn error(&self) -> Option<RunFailure<'_>> {
        match self {
            Self::NotStarted { error } | Self::WaitFailed { error } => {
                Some(RunFailure::Runner(error))
            }
            Self::MonitorFailed { failure } => Some(RunFailure::Monitor(failure)),
            Self::Exited(_)
            | Self::TimedOut
            | Self::Stalled
            | Self::StoppedByMonitor
            | Self::Answered
            | Self::Cancelled { .. } => None,
        }
    }
}

/// What one [`run`] produced.
#[derive(Debug)]
#[non_exhaustive]
pub struct RunResult {
    /// The single reason the run ended.
    pub termination: Termination,
    /// The wall-clock time the run took, supervision and killing included: the engine derives mutant timeouts from baseline durations, and a budget that excluded this overhead would be one the same work could exceed.
    pub duration: Duration,
    /// Combined stdout and stderr in the order the child wrote them, capped at the effective output limit by keeping the tail.
    /// Stderr alone when [`Spec::structured_stdout`] is set.
    pub output: Vec<u8>,
    /// The child's stdout when [`Spec::structured_stdout`] is set, head-capped at that many bytes; empty otherwise.
    pub stdout: Vec<u8>,
    /// Whether `stdout` was cut at the cap.
    pub stdout_truncated: bool,
    /// The id of the process the run started, which leads its group and is the parent of whatever it starts, or nothing where none started.
    pub leader: Option<u32>,
    /// Actual host waits on owned producer events and causally named OS backstops.
    pub waits: Vec<WaitNote>,
    /// Actual pipe-readiness attempts, observed releases and causal backstops, retained per owned reader.
    pub reader_waits: Vec<ReaderWaitCost>,
}

/// Measured physical waits on one owned output reader, including every readiness refusal.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ReaderWaitCost {
    /// The concrete output pipe whose readiness was awaited.
    pub stream: &'static str,
    /// Actual readiness wait calls, including refused calls.
    pub attempts: u64,
    /// Kernel readability or writer-EOF events actually observed.
    pub readable: u64,
    /// Retained reader-stop events actually observed.
    pub stopped: u64,
    /// Actual Windows anonymous-pipe readiness backstops.
    pub anonymous_pipe_backstops: u64,
    /// Measured monotonic nanoseconds spent in readiness waits.
    pub elapsed_ns: u64,
    /// Actual completion-channel waits made by the retained owner.
    pub completion_attempts: u64,
    /// Completion-channel deadlines that actually required a reader-stop publication.
    pub completion_backstops: u64,
    /// Measured completion phase nanoseconds, including its required stop publication.
    pub completion_elapsed_ns: Option<u64>,
    /// Actual thread joins attempted after completion was observed.
    pub join_attempts: u64,
    /// Measured monotonic nanoseconds spent joining the owned worker.
    pub join_elapsed_ns: Option<u64>,
    /// The executing host observed before the reader started.
    pub machine: crate::observation::Machine,
}

impl ReaderWaitCost {
    fn new(stream: &'static str) -> io::Result<Self> {
        Ok(Self {
            stream,
            attempts: 0,
            readable: 0,
            stopped: 0,
            anonymous_pipe_backstops: 0,
            elapsed_ns: 0,
            completion_attempts: 0,
            completion_backstops: 0,
            completion_elapsed_ns: None,
            join_attempts: 0,
            join_elapsed_ns: None,
            machine: crate::observation::Machine {
                os: std::env::consts::OS,
                cpus: thread::available_parallelism()?.get(),
            },
        })
    }

    fn elapsed(&mut self, duration: Duration) -> io::Result<()> {
        let elapsed = u64::try_from(duration.as_nanos()).map_err(io::Error::other)?;
        self.elapsed_ns = self
            .elapsed_ns
            .checked_add(elapsed)
            .ok_or_else(|| io::Error::other("owned reader wait nanoseconds overflowed"))?;
        Ok(())
    }

    fn counted(counter: &mut u64) -> io::Result<()> {
        *counter = counter
            .checked_add(1)
            .ok_or_else(|| io::Error::other("owned reader wait count overflowed"))?;
        Ok(())
    }
}

impl RunResult {
    /// Whether the process ran to completion with a zero exit status.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self.termination, Termination::Exited(ProcessExit::Code(0)))
    }

    /// The process's conventional status code, or [`EXIT_CODE_UNAVAILABLE`] at a legacy presentation boundary.
    #[must_use]
    pub const fn conventional_exit_code(&self) -> i32 {
        match self.termination.exit_code() {
            Some(code) => code,
            None => EXIT_CODE_UNAVAILABLE,
        }
    }

    /// Whether the wall-clock bound ended the run.
    #[must_use]
    pub const fn timed_out(&self) -> bool {
        matches!(self.termination, Termination::TimedOut)
    }

    /// The failure that prevented a trustworthy process result.
    #[must_use]
    pub const fn error(&self) -> Option<RunFailure<'_>> {
        self.termination.error()
    }

    /// The terminating signal, on a platform that has one.
    #[must_use]
    pub const fn signal(&self) -> Option<i32> {
        match self.termination {
            Termination::Exited(exit) => exit.signal(),
            Termination::NotStarted { .. }
            | Termination::TimedOut
            | Termination::Stalled
            | Termination::StoppedByMonitor
            | Termination::Answered
            | Termination::MonitorFailed { .. }
            | Termination::Cancelled { .. }
            | Termination::WaitFailed { .. } => None,
        }
    }
}

/// What a supervised command runs under: the flag that stops it, and who hears that it ran.
pub trait Watch {
    /// Raised when the caller should stop.
    fn cancel(&self) -> &Cancel;

    /// Records one finished process.
    fn exec(&self, spec: &Spec, result: &RunResult);

    /// Records something the caller decided that no process said, such as what an answer was read against.
    fn note(&self, kind: &str, detail: &str);
}

/// The watch the engine's own commands run under: this run's cancellation and this run's trace.
#[derive(Debug, Clone, Copy)]
pub struct Watched<'a> {
    /// Raised when the run should stop.
    pub cancel: &'a Cancel,
    /// Where the run records what it did.
    pub trace: &'a crate::trace::Recorder,
}

impl<'a> Watched<'a> {
    /// A watch over `cancel` that records into `trace`.
    #[must_use]
    pub const fn new(cancel: &'a Cancel, trace: &'a crate::trace::Recorder) -> Self {
        Self { cancel, trace }
    }
}

impl Watch for Watched<'_> {
    fn cancel(&self) -> &Cancel {
        self.cancel
    }

    fn exec(&self, spec: &Spec, result: &RunResult) {
        self.trace
            .exec_result(crate::trace::ExecRecord::of(spec, result));
        for wait in &result.waits {
            match serde_json::to_string(wait) {
                Ok(detail) => self.trace.note("host-wait", &detail),
                Err(source) => self.trace.note("host-wait-refused", &source.to_string()),
            }
        }
        for wait in &result.reader_waits {
            match serde_json::to_string(wait) {
                Ok(detail) => self.trace.note("pipe-reader-waits", &detail),
                Err(source) => self.trace.note("host-wait-refused", &source.to_string()),
            }
        }
    }

    fn note(&self, kind: &str, detail: &str) {
        self.trace.note(kind, detail);
    }
}

/// Starts the process described by `spec`, supervises the platform's declared process set, and returns when it has finished, timed out, or been cancelled.
#[must_use]
pub fn run(spec: &Spec, cancel: &Cancel) -> RunResult {
    run_with_stall_candidate(spec, cancel, |quiet| quiet.is_zero())
}

fn run_with_stall_candidate(
    spec: &Spec,
    cancel: &Cancel,
    stall_candidate: impl Fn(Duration) -> bool,
) -> RunResult {
    let started = Instant::now();
    let program = match preflight(spec, cancel, started) {
        Preflight::Ready(program) => program,
        Preflight::Done(result) => return result,
    };
    let deadline = match deadline_of(started, spec.timeout) {
        Ok(deadline) => deadline,
        Err(error) => return not_started(started, error, Vec::new()),
    };
    let observed = Observation::subscribe();
    let mut waits = Vec::new();
    let answered = Arc::new(Answered {
        named: AtomicBool::new(false),
        signal: observed.signal(),
    });
    let mut running = match start(spec, program, &answered) {
        Ok(started) => started,
        Err(Failed {
            error,
            output,
            reader_waits,
        }) => {
            let mut result = not_started(started, error, output);
            result.reader_waits = reader_waits;
            return result;
        }
    };
    let leader = running.child.id;
    if let Some(leaders) = &spec.leaders {
        leaders.started(leader, running.child.child.membership());
    }
    let outcome = await_exit(
        &mut running.child,
        Stops {
            started,
            deadline,
            cancel,
            monitor: spec.stop_file.as_deref(),
            progress: spec.progress.as_ref(),
            answered: spec.stop_at_first_failure.then_some(answered.as_ref()),
            observed: &observed,
            waits: &mut waits,
        },
        stall_candidate,
    );
    let mut completed = complete(
        started,
        running,
        (
            outcome,
            spec.stop_at_first_failure.then_some(answered.as_ref()),
        ),
    );
    completed.waits.extend(waits);
    match cancel.clock.now(started, leader) {
        Ok(now) => completed.duration = now.duration_since(started),
        Err(source) => {
            completed.termination = Termination::WaitFailed {
                error: RunnerError::ProcessWaitFailed { source },
            };
        }
    }
    if let Err(source) = cancel.clock.finished(leader) {
        completed.termination = Termination::WaitFailed {
            error: RunnerError::ProcessWaitFailed {
                source: io::Error::other(format!(
                    "logical clock cleanup failed after {:?}: {source}",
                    completed.termination
                )),
            },
        };
    }
    if let Some(leaders) = &spec.leaders {
        leaders.finished(leader);
    }
    completed
}

fn deadline_of(
    started: Instant,
    timeout: Option<Duration>,
) -> Result<Option<Instant>, RunnerError> {
    match timeout {
        Some(timeout) => started
            .checked_add(timeout)
            .map(Some)
            .ok_or(RunnerError::DeadlineOverflow { timeout }),
        None => Ok(None),
    }
}

fn complete(
    started: Instant,
    running: Started,
    (outcome, answered): (Exit, Option<&Answered>),
) -> RunResult {
    let Started {
        merged,
        head,
        mut child,
    } = running;
    let leader = Some(child.id);
    let status = child.reap_observed();
    let merged_finish = merged.finish();
    let structured_finish = head.map(JoinedReader::finish);
    let waits = std::mem::take(&mut child.waits);
    drop(child);
    let duration = started.elapsed();
    let named_a_failure = answered.is_some_and(|answered| answered.named.load(Ordering::SeqCst));
    let wait = outcome.wait();
    let answer = rust_mutants_decision::answered::answer(wait, named_a_failure);
    let process_termination = process_termination(outcome, status, answer);
    let mut capture_failure = None;
    let mut reader_waits = Vec::new();
    let output = match finished_capture(merged_finish, &mut capture_failure, &mut reader_waits) {
        Some(output) => output,
        None => Vec::new(),
    };
    let (stdout, stdout_truncated) = match structured_finish {
        None => (Vec::new(), false),
        Some(finished) => match finished_capture(finished, &mut capture_failure, &mut reader_waits)
        {
            Some((bytes, truncated, _total)) => (bytes, truncated),
            None => (Vec::new(), false),
        },
    };
    let capture_failed = capture_failure.is_some();
    let termination = match capture_failure {
        Some(mut error) => {
            if let Some(failure) = process_termination.error() {
                error = combined_reader_failure(&error, &failure);
            }
            Termination::WaitFailed { error }
        }
        None => process_termination,
    };
    let termination = checked_answered_termination(
        Observed {
            wait,
            named_a_failure,
            capture_failed,
        },
        termination,
    );
    RunResult {
        termination,
        duration,
        output,
        stdout,
        stdout_truncated,
        leader,
        waits,
        reader_waits,
    }
}

fn process_termination(
    outcome: Exit,
    status: io::Result<ExitStatus>,
    answer: Answer,
) -> Termination {
    let status = match status {
        Ok(status) => status,
        Err(source) => {
            return Termination::WaitFailed {
                error: RunnerError::ProcessWaitFailed {
                    source: io::Error::other(format!(
                        "process settlement refused after {outcome:?}: {source}"
                    )),
                },
            };
        }
    };
    match outcome {
        Exit::Exited | Exit::Answered if answer == Answer::Answered => Termination::Answered,
        Exit::TimedOut => Termination::TimedOut,
        Exit::Stalled => Termination::Stalled,
        Exit::StoppedByMonitor => Termination::StoppedByMonitor,
        Exit::Answered => Termination::WaitFailed {
            error: RunnerError::AnsweredStopInconsistent,
        },
        Exit::MonitorFailed(failure) => Termination::MonitorFailed { failure },
        Exit::Cancelled => Termination::Cancelled { started: true },
        Exit::Exited => Termination::Exited(sys::process_exit(status)),
        Exit::WaitFailed(source) => Termination::WaitFailed {
            error: RunnerError::ProcessWaitFailed { source },
        },
        Exit::SupervisionFailed(error) => Termination::WaitFailed { error },
    }
}

fn finished_capture<Output>(
    finished: Result<ReaderFinish<Output>, RunnerError>,
    failure: &mut Option<RunnerError>,
    waits: &mut Vec<ReaderWaitCost>,
) -> Option<Output> {
    let ReaderFinish {
        captured:
            CapturedReader {
                capture,
                drain,
                waits: measured,
            },
        joined,
    } = match finished {
        Ok(finished) => finished,
        Err(error) => {
            retain_reader_failure(failure, error);
            return None;
        }
    };
    waits.push(measured);
    for result in [drain, joined] {
        if let Err(error) = result {
            retain_reader_failure(failure, error);
        }
    }
    match capture {
        Ok(capture) => Some(capture),
        Err(error) => {
            retain_reader_failure(failure, error);
            None
        }
    }
}

fn retain_reader_failure(failure: &mut Option<RunnerError>, error: RunnerError) {
    *failure = Some(match failure.take() {
        Some(first) => combined_reader_failure(&first, &error),
        None => error,
    });
}

fn combined_reader_failure(
    first: &RunnerError,
    additional: &impl std::fmt::Display,
) -> RunnerError {
    RunnerError::OutputReadFailed {
        source: io::Error::other(format!(
            "{first}; additional owned process/reader failure: {additional}"
        )),
    }
}

enum Preflight<'a> {
    Ready(&'a OsString),
    Done(RunResult),
}

fn preflight<'a>(spec: &'a Spec, cancel: &Cancel, started: Instant) -> Preflight<'a> {
    let Some(program) = spec.argv.first() else {
        return Preflight::Done(not_started(
            started,
            RunnerError::SpecInvalid {
                message: "has no argument vector",
            },
            Vec::new(),
        ));
    };
    if program
        .as_encoded_bytes()
        .iter()
        .all(u8::is_ascii_whitespace)
    {
        return Preflight::Done(not_started(
            started,
            RunnerError::SpecInvalid {
                message: "has an empty executable name",
            },
            Vec::new(),
        ));
    }
    if cancel.is_cancelled() {
        return Preflight::Done(RunResult {
            termination: Termination::Cancelled { started: false },
            duration: started.elapsed(),
            output: Vec::new(),
            stdout: Vec::new(),
            stdout_truncated: false,
            leader: None,
            waits: Vec::new(),
            reader_waits: Vec::new(),
        });
    }
    Preflight::Ready(program)
}

fn not_started(started: Instant, error: RunnerError, output: Vec<u8>) -> RunResult {
    RunResult {
        termination: Termination::NotStarted { error },
        duration: started.elapsed(),
        output,
        stdout: Vec::new(),
        stdout_truncated: false,
        leader: None,
        waits: Vec::new(),
        reader_waits: Vec::new(),
    }
}

/// What [`start`] hands to the wait half of [`run`].
struct Started {
    child: SupervisedChild,
    merged: JoinedReader<Vec<u8>>,
    head: Option<JoinedReader<(Vec<u8>, bool, u64)>>,
}

type StructuredReader = Option<JoinedReader<(Vec<u8>, bool, u64)>>;

struct StartingReaders {
    merged: JoinedReader<Vec<u8>>,
    head: StructuredReader,
}

/// A child process that cannot be detached by dropping its raw handle.
#[derive(Debug)]
struct SupervisedChild {
    child: GroupChild,
    id: u32,
    reaping: Reaping,
    waits: Vec<WaitNote>,
}

impl SupervisedChild {
    fn launch(
        prepared: njutest_process::PreparedGroup,
        command: &mut Command,
        reaping: Reaping,
    ) -> Result<Self, RunnerError> {
        match prepared.launch(command) {
            njutest_process::GroupStart::Started(child) => {
                let id = match child.id() {
                    Some(id) => id,
                    None => terminal_process_ownership_failure(),
                };
                Ok(Self {
                    child,
                    id,
                    reaping,
                    waits: Vec::new(),
                })
            }
            njutest_process::GroupStart::ProcessRefused { source } => {
                Err(RunnerError::ProcessStartFailed {
                    program: command.get_program().to_os_string(),
                    source,
                })
            }
            njutest_process::GroupStart::SupervisionRefused { source } => Err(unavailable(source)),
        }
    }

    fn exit_observed(&self) -> io::Result<bool> {
        self.completion().wait(Some(Duration::ZERO))
    }

    fn completion(&self) -> Arc<event::ChildEvent> {
        self.child.completion()
    }

    fn reap_observed(&mut self) -> io::Result<ExitStatus> {
        match self.reaping {
            Reaping::Normal => self.child.wait_status(),
            #[cfg(any(test, feature = "testkit"))]
            Reaping::SimulatedUnreapable => {
                let settled = self.child.stop();
                note_ownership_failure(
                    "the injected reap refusal after actual member settlement",
                    &io::Error::other(format!("{settled:?}")),
                );
                terminal_process_ownership_failure()
            }
        }
    }
}

fn unavailable(source: io::Error) -> RunnerError {
    RunnerError::SupervisionUnavailable {
        message: source.to_string(),
        source: Some(source),
    }
}

/// A start that failed, with whatever output was captured before it did.
struct Failed {
    error: RunnerError,
    output: Vec<u8>,
    reader_waits: Vec<ReaderWaitCost>,
}

/// The first half of [`run`]: supervision, the pipes, the spawn, the reader threads, and adoption.
/// On any failure the child, if any, is dead.
fn start(spec: &Spec, program: &OsString, answered: &Arc<Answered>) -> Result<Started, Failed> {
    let failed = |error| Failed {
        error,
        output: Vec::new(),
        reader_waits: Vec::new(),
    };
    let prepared =
        njutest_process::PreparedGroup::new().map_err(|source| failed(unavailable(source)))?;
    let Wired {
        mut command,
        merged,
        structured,
    } = wire(spec, program).map_err(|source| {
        failed(RunnerError::ProcessStartFailed {
            program: program.clone(),
            source,
        })
    })?;
    let limit = match spec.output_limit {
        Some(asked) => asked,
        None => DEFAULT_OUTPUT_LIMIT,
    };
    let readers = launch_readers(
        merged,
        structured,
        limit,
        spec.stop_at_first_failure.then(|| Arc::clone(answered)),
    )
    .map_err(failed)?;
    let child = match SupervisedChild::launch(prepared, &mut command, spec.reaping) {
        Ok(child) => child,
        Err(error) => {
            drop(command);
            let mut refused = finish_readers(readers);
            refused.error = match refused.error {
                Some(reader) => Some(combined_reader_failure(&error, &reader)),
                None => Some(error),
            };
            return Err(Failed {
                error: match refused.error {
                    Some(error) => error,
                    None => terminal_process_ownership_failure(),
                },
                output: refused.output,
                reader_waits: refused.waits,
            });
        }
    };
    drop(command);
    Ok(Started {
        child,
        merged: readers.merged,
        head: readers.head,
    })
}

fn launch_readers(
    merged: io::PipeReader,
    structured: Option<(usize, io::PipeReader)>,
    output_limit: usize,
    answered: Option<Arc<Answered>>,
) -> Result<StartingReaders, RunnerError> {
    let head = match structured {
        Some((limit, reader)) => Some(JoinedReader::launch(
            reader,
            "structured stdout",
            HeadCapture(HeadBuffer::new(limit)),
        )?),
        None => None,
    };
    let tail = TailCapture(TailBuffer::new(output_limit));
    let merged = match answered {
        None => JoinedReader::launch(merged, "combined stdout/stderr", tail)?,
        Some(answered) => JoinedReader::launch(
            merged,
            "combined stdout/stderr",
            FirstFailure {
                inner: tail,
                partial: Vec::new(),
                answered,
            },
        )?,
    };
    Ok(StartingReaders { merged, head })
}

struct FinishedReaders {
    output: Vec<u8>,
    error: Option<RunnerError>,
    waits: Vec<ReaderWaitCost>,
}

fn finish_readers(readers: StartingReaders) -> FinishedReaders {
    let merged = readers.merged.finish();
    let structured = readers.head.map(JoinedReader::finish);
    let mut error = None;
    let mut waits = Vec::new();
    let output = match finished_capture(merged, &mut error, &mut waits) {
        Some(bytes) => bytes,
        None => Vec::new(),
    };
    if let Some(structured) = structured {
        let captured = finished_capture(structured, &mut error, &mut waits);
        drop(captured);
    }
    FinishedReaders {
        output,
        error,
        waits,
    }
}

/// The program to start, found on the search path the spec's own environment names.
///
/// # Errors
/// The name is bare and the environment's search path does not hold it.
fn resolved(spec: &Spec, program: &OsString) -> io::Result<OsString> {
    let Some(env) = &spec.env else {
        return Ok(program.clone());
    };
    crate::cargo::resolve_executable(Path::new(program), env.search_path())
        .map(PathBuf::into_os_string)
        .map_err(|unfound| io::Error::new(io::ErrorKind::NotFound, unfound.to_string()))
}

/// What every child is told about presenting its output, over whatever its environment says, since the engine reads what it writes.
pub const PRESENTATION: [(&str, &str); 3] = [
    ("CARGO_TERM_COLOR", "never"),
    ("CARGO_TERM_QUIET", "false"),
    ("CARGO_TERM_VERBOSE", "false"),
];

/// A command with its pipes attached: the merged reader, and the structured stdout reader with its cap when the spec asked for one.
struct Wired {
    command: Command,
    merged: io::PipeReader,
    structured: Option<(usize, io::PipeReader)>,
}

/// Builds the command and the pipes it writes to.
/// No stdin: a test binary that reads from the terminal would hang.
/// One pipe for both streams unless stdout is wanted whole, so the interleaving is the child's own.
fn wire(spec: &Spec, program: &OsString) -> io::Result<Wired> {
    let (merged, stderr) = io::pipe()?;
    let mut command = Command::new(resolved(spec, program)?);
    command.args(spec.argv.iter().skip(1));
    if let Some(dir) = &spec.dir {
        command.current_dir(dir);
    }
    if let Some(env) = &spec.env {
        command.env_clear();
        command.envs(env.for_process());
    }
    command.envs(PRESENTATION);
    if let Some(progress) = &spec.progress {
        let (name, value) = progress.told();
        command.env(name, value);
    }
    command.stdin(Stdio::null());
    let structured = if let Some(limit) = spec.structured_stdout {
        let (reader, writer) = io::pipe()?;
        command.stdout(Stdio::from(writer));
        Some((limit, reader))
    } else {
        command.stdout(Stdio::from(stderr.try_clone()?));
        None
    };
    command.stderr(Stdio::from(stderr));
    Ok(Wired {
        command,
        merged,
        structured,
    })
}

trait ReaderCapture: Send + 'static {
    type Output: Send + 'static;

    fn write(&mut self, bytes: &[u8]) -> Result<(), OutputError>;
    fn finish(self) -> Result<Self::Output, OutputError>;
}

struct TailCapture(TailBuffer);

impl ReaderCapture for TailCapture {
    type Output = Vec<u8>;

    fn write(&mut self, bytes: &[u8]) -> Result<(), OutputError> {
        self.0.write(bytes)
    }

    fn finish(self) -> Result<Self::Output, OutputError> {
        self.0.capture()
    }
}

/// A capture that also raises `answered` at the first whole line in which libtest says a test failed.
struct FirstFailure<C> {
    inner: C,
    partial: Vec<u8>,
    answered: Arc<Answered>,
}

impl<C: ReaderCapture> ReaderCapture for FirstFailure<C> {
    type Output = C::Output;

    fn write(&mut self, bytes: &[u8]) -> Result<(), OutputError> {
        self.partial.extend_from_slice(bytes);
        while let Some(end) = self.partial.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            if says_a_test_failed(&line) {
                self.answered.named.store(true, Ordering::SeqCst);
                self.answered.signal.publish(Event::Changed);
            }
        }
        self.inner.write(bytes)
    }

    fn finish(self) -> Result<Self::Output, OutputError> {
        self.inner.finish()
    }
}

/// Whether `line` is libtest's report of one test that failed: `test <name> ... FAILED`, and never its closing `test result: FAILED.`.
#[must_use]
pub fn says_a_test_failed(line: &[u8]) -> bool {
    let line = match line.strip_suffix(b"\n") {
        Some(without_newline) => without_newline,
        None => line,
    };
    let line = match line.strip_suffix(b"\r") {
        Some(without_return) => without_return,
        None => line,
    };
    line.starts_with(b"test ") && line.ends_with(b" ... FAILED")
}

struct HeadCapture(HeadBuffer);

impl ReaderCapture for HeadCapture {
    type Output = (Vec<u8>, bool, u64);

    fn write(&mut self, bytes: &[u8]) -> Result<(), OutputError> {
        self.0.write(bytes)
    }

    fn finish(self) -> Result<Self::Output, OutputError> {
        self.0.capture()
    }
}

struct CapturedReader<Output> {
    capture: Result<Output, RunnerError>,
    drain: Result<(), RunnerError>,
    waits: ReaderWaitCost,
}

struct ReaderFinish<Output> {
    captured: CapturedReader<Output>,
    joined: Result<(), RunnerError>,
}

/// A pipe reader whose bounded owner always joins the one thread it creates.
#[derive(Debug)]
struct JoinedReader<Output: Send + 'static> {
    stream: &'static str,
    stop: njutest_process::ReaderStop,
    completed: mpsc::Receiver<CapturedReader<Output>>,
    handle: Option<JoinHandle<()>>,
}

impl<Output: Send + 'static> JoinedReader<Output> {
    fn launch<C: ReaderCapture<Output = Output>>(
        mut reader: io::PipeReader,
        stream: &'static str,
        mut capture: C,
    ) -> Result<Self, RunnerError> {
        let mut waits = ReaderWaitCost::new(stream)
            .map_err(|source| RunnerError::OutputReaderConfigurationFailed { stream, source })?;
        sys::configure_reader(&reader)
            .map_err(|source| RunnerError::OutputReaderConfigurationFailed { stream, source })?;
        let (readiness, stop) = njutest_process::ReaderWait::channel()
            .map_err(|source| RunnerError::OutputReaderConfigurationFailed { stream, source })?;
        let (completion, completed) = mpsc::sync_channel::<CapturedReader<Output>>(1);
        let handle = thread::Builder::new()
            .name(format!("rust-mutants-{stream}"))
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                let drain = loop {
                    let outcome = reader.read(&mut buffer);
                    match outcome {
                        Ok(0) => match sys::stream_ended(&reader) {
                            Ok(true) => break Ok(()),
                            Ok(false) => {
                                if let Err(error) = ready_for_read(&readiness, &reader, &mut waits)
                                {
                                    break Err(error);
                                }
                            }
                            Err(source) => break Err(RunnerError::OutputReadFailed { source }),
                        },
                        Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                            if let Err(error) = ready_for_read(&readiness, &reader, &mut waits) {
                                break Err(error);
                            }
                        }
                        Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
                        Err(source) => break Err(RunnerError::OutputReadFailed { source }),
                        Ok(read) => {
                            let Some(bytes) = buffer.get(..read) else {
                                break Err(RunnerError::OutputCaptureFailed {
                                    source: OutputError::CapacityInvariant,
                                });
                            };
                            if let Err(source) = capture.write(bytes) {
                                break Err(RunnerError::OutputCaptureFailed { source });
                            }
                        }
                    }
                };
                let finished = CapturedReader {
                    capture: capture.finish().map_err(output_error),
                    drain,
                    waits,
                };
                if completion.send(finished).is_err() {
                    terminal_reader_ownership_failure();
                }
            })
            .map_err(|source| RunnerError::OutputReaderStartFailed { stream, source })?;
        Ok(Self {
            stream,
            stop,
            completed,
            handle: Some(handle),
        })
    }

    fn request_stop(&self) {
        if let Err(source) = self.stop.stop() {
            note_ownership_failure("publishing the owned pipe stop event", &source);
            terminal_reader_ownership_failure();
        }
    }

    fn finish(mut self) -> Result<ReaderFinish<Output>, RunnerError> {
        let began = Instant::now();
        let (mut finished, completion_attempts, completion_backstops) =
            match self.completed.recv_timeout(IO_DRAIN_GRACE) {
                Ok(finished) => (finished, 1, 0),
                Err(RecvTimeoutError::Timeout) => {
                    self.request_stop();
                    let mut finished = match self.completed.recv_timeout(IO_DRAIN_GRACE) {
                        Ok(finished) => finished,
                        Err(RecvTimeoutError::Timeout) => terminal_reader_ownership_failure(),
                        Err(RecvTimeoutError::Disconnected) => {
                            self.join()?;
                            return Err(RunnerError::OutputReaderDisconnected {
                                stream: self.stream,
                            });
                        }
                    };
                    let timed_out = RunnerError::OutputDrainTimedOut {
                        stream: self.stream,
                    };
                    finished.drain = Err(match finished.drain {
                        Ok(()) => timed_out,
                        Err(error) => combined_reader_failure(&timed_out, &error),
                    });
                    (finished, 2, 1)
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.join()?;
                    return Err(RunnerError::OutputReaderDisconnected {
                        stream: self.stream,
                    });
                }
            };
        finished.waits.completion_attempts = completion_attempts;
        finished.waits.completion_backstops = completion_backstops;
        let elapsed = u64::try_from(began.elapsed().as_nanos());
        match elapsed {
            Ok(elapsed) => finished.waits.completion_elapsed_ns = Some(elapsed),
            Err(source) => {
                let measurement = RunnerError::OutputReadFailed {
                    source: io::Error::other(source),
                };
                finished.drain = Err(match finished.drain {
                    Ok(()) => measurement,
                    Err(error) => combined_reader_failure(&error, &measurement),
                });
            }
        }
        let began = Instant::now();
        let joining = self.handle.is_some();
        let joined = self.join();
        finished.waits.join_attempts = u64::from(joining);
        let joined = match u64::try_from(began.elapsed().as_nanos()) {
            Ok(elapsed) => {
                finished.waits.join_elapsed_ns = Some(elapsed);
                joined
            }
            Err(source) => {
                let measurement = RunnerError::OutputReadFailed {
                    source: io::Error::other(source),
                };
                Err(match joined {
                    Ok(()) => measurement,
                    Err(error) => combined_reader_failure(&error, &measurement),
                })
            }
        };
        Ok(ReaderFinish {
            captured: finished,
            joined,
        })
    }

    fn join(&mut self) -> Result<(), RunnerError> {
        let handle = self
            .handle
            .take()
            .ok_or(RunnerError::OutputReaderOwnershipLost {
                stream: self.stream,
            })?;
        handle
            .join()
            .map_err(|_panic| RunnerError::OutputReaderPanicked {
                stream: self.stream,
            })
    }
}

impl<Output: Send + 'static> Drop for JoinedReader<Output> {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        self.request_stop();
        match self.completed.recv_timeout(IO_DRAIN_GRACE) {
            Ok(finished) => {
                if let Err(error) = finished.capture {
                    eprintln!(
                        "owned {} capture refused during cleanup: {error}",
                        self.stream
                    );
                }
                if let Err(error) = finished.drain {
                    eprintln!("owned {} drain ended during cleanup: {error}", self.stream);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                let joined = handle.join();
                eprintln!(
                    "owned {} reader disconnected during cleanup; joined: {joined:?}",
                    self.stream
                );
                terminal_reader_ownership_failure();
            }
            Err(RecvTimeoutError::Timeout) => {
                eprintln!("owned {} reader exceeded its cleanup backstop", self.stream);
                terminal_reader_ownership_failure();
            }
        }
        if handle.join().is_err() {
            terminal_reader_ownership_failure();
        }
    }
}

fn ready_for_read(
    wait: &njutest_process::ReaderWait,
    reader: &io::PipeReader,
    waits: &mut ReaderWaitCost,
) -> Result<(), RunnerError> {
    ReaderWaitCost::counted(&mut waits.attempts)
        .map_err(|source| RunnerError::OutputReadFailed { source })?;
    let began = Instant::now();
    let observed = wait
        .wait(reader)
        .map_err(|source| RunnerError::OutputReadFailed { source });
    let measured = waits
        .elapsed(began.elapsed())
        .map_err(|source| RunnerError::OutputReadFailed { source });
    let ready = match (observed, measured) {
        (Ok(ready), Ok(())) => ready,
        (Err(error), Ok(())) | (Ok(_), Err(error)) => return Err(error),
        (Err(error), Err(additional)) => return Err(combined_reader_failure(&error, &additional)),
    };
    let counted = match ready {
        njutest_process::ReaderReady::Readable => ReaderWaitCost::counted(&mut waits.readable),
        njutest_process::ReaderReady::Stopped => ReaderWaitCost::counted(&mut waits.stopped),
        #[cfg(windows)]
        njutest_process::ReaderReady::AnonymousPipeBackstop => {
            ReaderWaitCost::counted(&mut waits.anonymous_pipe_backstops)
        }
    };
    counted.map_err(|source| RunnerError::OutputReadFailed { source })?;
    match ready {
        njutest_process::ReaderReady::Readable => Ok(()),
        njutest_process::ReaderReady::Stopped => Err(RunnerError::OutputDrainTimedOut {
            stream: waits.stream,
        }),
        #[cfg(windows)]
        njutest_process::ReaderReady::AnonymousPipeBackstop => Ok(()),
    }
}

#[cold]
fn terminal_reader_ownership_failure() -> ! {
    std::process::abort();
}

const fn output_error(source: OutputError) -> RunnerError {
    RunnerError::OutputCaptureFailed { source }
}

/// How the wait half of [`run`] ended.
#[derive(Debug)]
enum Exit {
    /// The child exited and remains waitable until its declared process set is forcefully signalled.
    Exited,
    /// Observing the child status failed before the supervised process set was ended.
    WaitFailed(io::Error),
    /// The wall-clock deadline expired and the declared process set was ended.
    TimedOut,
    /// The progress file went unchanged for its quiet window and the declared process set was ended.
    Stalled,
    /// The caller asked the declared process set to stop.
    Cancelled,
    /// An execution-specific monitor asked the declared process set to stop.
    StoppedByMonitor,
    /// The harness said a test failed, and the declared process set was ended there.
    Answered,
    /// The execution-specific monitor could not be inspected safely.
    MonitorFailed(MonitorError),
    /// Stopping or reaping the child failed, so the triggering event cannot be reported as a trustworthy termination.
    SupervisionFailed(RunnerError),
}

impl Exit {
    /// How the wait ended, as far as a stop at the first failing test reads it.
    const fn wait(&self) -> Wait {
        match self {
            Self::Exited => Wait::Exited,
            Self::Answered => Wait::Answered,
            Self::WaitFailed(_)
            | Self::TimedOut
            | Self::Stalled
            | Self::Cancelled
            | Self::StoppedByMonitor
            | Self::MonitorFailed(_)
            | Self::SupervisionFailed(_) => Wait::Other,
        }
    }
}

/// `reported`, where it agrees with what was observed of the run, and otherwise the inconsistency, so an ending the answered stop cannot stand on is never a result.
fn checked_answered_termination(observed: Observed, reported: Termination) -> Termination {
    if rust_mutants_decision::answered::agrees(observed, matches!(reported, Termination::Answered))
    {
        reported
    } else {
        Termination::WaitFailed {
            error: RunnerError::AnsweredStopInconsistent,
        }
    }
}

/// The inherited signal-handler compatibility bound for an exposed raw atomic.
/// Owned cancellation subscriptions publish events directly and never use this backstop.
const RAW_SIGNAL_BACKSTOP: Duration = Duration::from_millis(25);

#[derive(Debug)]
struct Answered {
    named: AtomicBool,
    signal: Signal,
}

struct Stops<'a> {
    started: Instant,
    deadline: Option<Instant>,
    cancel: &'a Cancel,
    monitor: Option<&'a Path>,
    progress: Option<&'a Progress>,
    answered: Option<&'a Answered>,
    observed: &'a Observation,
    waits: &'a mut Vec<WaitNote>,
}

/// What the wait loop actually observed of each progress file and the logical time of that observation.
struct Watching<'a> {
    progress: &'a Progress,
    seen: [Option<Vec<u8>>; 2],
    started: Instant,
    observation: ProgressObservation,
    stillness: Stillness,
}

/// Whether the watch has established its first observation-relative quiet window.
enum ProgressObservation {
    Pending,
    Seen,
}

impl<'a> Watching<'a> {
    const fn of(progress: &'a Progress, started: Instant) -> Self {
        Self {
            progress,
            seen: [None, None],
            started,
            observation: ProgressObservation::Pending,
            stillness: Stillness::new(progress.quiet),
        }
    }

    /// How long after the watch began `now` is.
    fn since(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started)
    }

    /// Samples the signals at their actual logical observation time, preserving a failed read as a refusal.
    fn look(&mut self, now: Instant) -> Result<Instant, MonitorError> {
        let changed = self.sample()? || matches!(self.observation, ProgressObservation::Pending);
        self.observation = ProgressObservation::Seen;
        self.stillness = self.stillness.looked(self.since(now), changed);
        let deadline = self
            .stillness
            .stalls_at()
            .and_then(|stalls| self.started.checked_add(stalls))
            .ok_or_else(|| MonitorError::Inspect {
                path: self.progress.path.clone(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the progress quiet window exceeds the supervision clock",
                ),
            })?;
        Ok(deadline)
    }

    /// Reads each bounded signal once, distinguishing a missing publication from an unreadable resource.
    fn sample(&mut self) -> Result<bool, MonitorError> {
        let mut changed = false;
        for (path, seen) in self.progress.signals().into_iter().zip(&mut self.seen) {
            let content = match read_side_channel(path) {
                Ok(content) => Some(content),
                Err(missing) if missing.kind() == io::ErrorKind::NotFound => None,
                Err(source) => {
                    return Err(MonitorError::Inspect {
                        path: path.to_path_buf(),
                        source,
                    });
                }
            };
            if *seen != content {
                *seen = content;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Confirms a full observed window only after a second successful signal observation.
    fn confirms_stall(&mut self, now: Instant) -> Result<bool, MonitorError> {
        if matches!(self.observation, ProgressObservation::Pending)
            || !self.stillness.still_for_the_window(self.since(now))
        {
            return Ok(false);
        }
        if self.sample()? {
            self.stillness = self.stillness.looked(self.since(now), true);
            return Ok(false);
        }
        Ok(true)
    }
}

/// The most a side-channel file the supervised process writes may hold before reading it is refused.
pub(crate) const SIDE_CHANNEL_LIMIT: u64 = 16 * 1024;

/// Reads a file the supervised process can replace, refusing a link, anything but a regular file, and more than [`SIDE_CHANNEL_LIMIT`] bytes, and never blocking to open it.
///
/// # Errors
/// Returns the operating system's failure, or `InvalidData` for a file that is not regular or is too large.
pub(crate) fn read_side_channel(path: &Path) -> io::Result<Vec<u8>> {
    let file = open_side_channel(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the side channel is not a regular file",
        ));
    }
    let mut content = Vec::new();
    let read = file
        .take(SIDE_CHANNEL_LIMIT.saturating_add(1))
        .read_to_end(&mut content)?;
    let within = match u64::try_from(read) {
        Ok(length) => length <= SIDE_CHANNEL_LIMIT,
        Err(_wider_than_any_file) => false,
    };
    if !within {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the side channel is larger than it may be",
        ));
    }
    Ok(content)
}

#[cfg(unix)]
fn open_side_channel(path: &Path) -> io::Result<std::fs::File> {
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    Ok(std::fs::File::from(descriptor))
}

#[cfg(windows)]
fn open_side_channel(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// The stop a harness's first failing test asks for, where the run asked to end there and it has.
fn answered(child: &mut SupervisedChild, answered: Option<&Answered>) -> Option<Exit> {
    answered
        .is_some_and(|answered| answered.named.load(Ordering::SeqCst))
        .then(|| match terminate(child) {
            Ok(()) => Exit::Answered,
            Err(error) => Exit::SupervisionFailed(error),
        })
}

/// Waits on retained exit, cancellation and filesystem events in the declared clock domain.
fn await_exit(
    child: &mut SupervisedChild,
    stops: Stops<'_>,
    stall_candidate: impl Fn(Duration) -> bool,
) -> Exit {
    let _resources = match event::Resources::subscribe(stops.observed, &stops) {
        Ok(resources) => resources,
        Err(source) => return stopped_after(child, Exit::WaitFailed(source)),
    };
    let _exit = match event::ProcessWake::launch(&child.completion(), stops.observed.signal()) {
        Ok(exit) => exit,
        Err(source) => return stopped_after(child, Exit::WaitFailed(source)),
    };
    let mut waiting = Waiting {
        watching: stops
            .progress
            .map(|progress| Watching::of(progress, stops.started)),
        previous: stops.started,
        acknowledged: None,
        stops,
    };
    loop {
        match child.exit_observed() {
            Ok(true) => return Exit::Exited,
            Ok(false) => {}
            Err(source) => return stopped_after(child, Exit::WaitFailed(source)),
        }
        let sample = match waiting.sample(child.id) {
            Ok(sample) => sample,
            Err(exit) => return stopped_after(child, exit),
        };
        if let Some(exit) = waiting.decided(child, &sample, &stall_candidate) {
            return exit;
        }
        if let Err(source) = waiting.wait(child.id, sample) {
            return stopped_after(child, Exit::WaitFailed(source));
        }
    }
}

struct Sample {
    now: Instant,
    tick: Option<Vec<u8>>,
    remaining: Option<Duration>,
    quiet: Option<Duration>,
}

struct Waiting<'a> {
    stops: Stops<'a>,
    watching: Option<Watching<'a>>,
    previous: Instant,
    acknowledged: Option<Vec<u8>>,
}

impl Waiting<'_> {
    fn sample(&mut self, id: u32) -> Result<Sample, Exit> {
        let (now, tick) = self
            .stops
            .cancel
            .clock
            .read(self.stops.started, id)
            .map_err(Exit::WaitFailed)?;
        if now < self.previous {
            return Err(Exit::WaitFailed(io::Error::new(
                io::ErrorKind::InvalidData,
                "the logical supervision clock regressed",
            )));
        }
        self.previous = now;
        let remaining = self.stops.deadline.map(|deadline| until(deadline, now));
        let quiet = self
            .watching
            .as_mut()
            .map(|watching| watching.look(now))
            .transpose()
            .map_err(Exit::MonitorFailed)?
            .map(|stalled| until(stalled, now));
        Ok(Sample {
            now,
            tick,
            remaining,
            quiet,
        })
    }

    fn decided(
        &mut self,
        child: &mut SupervisedChild,
        sample: &Sample,
        stall_candidate: &impl Fn(Duration) -> bool,
    ) -> Option<Exit> {
        if self.stops.cancel.is_cancelled() {
            return Some(stopped_after(child, Exit::Cancelled));
        }
        if let Some(exit) = answered(child, self.stops.answered) {
            return Some(exit);
        }
        if let Some(path) = self.stops.monitor
            && let Some(exit) = monitored(child, path)
        {
            return Some(exit);
        }
        if sample
            .remaining
            .is_some_and(|remaining| remaining.is_zero())
        {
            return Some(stopped_after(child, Exit::TimedOut));
        }
        if sample.quiet.is_some_and(stall_candidate) {
            match self
                .watching
                .as_mut()
                .map(|watching| watching.confirms_stall(sample.now))
                .transpose()
            {
                Ok(Some(true)) => return Some(stopped_after(child, Exit::Stalled)),
                Ok(Some(false) | None) => {}
                Err(error) => return Some(stopped_after(child, Exit::MonitorFailed(error))),
            }
        }
        None
    }

    fn wait(&mut self, id: u32, sample: Sample) -> io::Result<()> {
        if sample.tick != self.acknowledged {
            self.stops
                .cancel
                .clock
                .acknowledged(id, sample.tick.as_deref())?;
            self.acknowledged = sample.tick;
        }
        let left = match (sample.remaining, sample.quiet) {
            (Some(bound), Some(quiet)) => Some(bound.min(quiet)),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        };
        let deadline = self.stops.cancel.clock.host_deadline(left)?;
        let (deadline, cause) = if self.stops.cancel.raw() {
            let signal = Instant::now()
                .checked_add(RAW_SIGNAL_BACKSTOP)
                .ok_or_else(|| io::Error::other("the raw signal backstop exceeds the clock"))?;
            (
                Some(match deadline {
                    Some(deadline) => deadline.min(signal),
                    None => signal,
                }),
                "owned exit/resource/cancellation event or raw atomic signal backstop",
            )
        } else {
            (
                deadline,
                "owned exit/resource/cancellation event or semantic supervision deadline",
            )
        };
        let waited = self
            .stops
            .observed
            .wait(&format!("process-group:{id}"), cause, deadline)?;
        self.stops.waits.push(waited.note);
        match waited.event? {
            Event::Changed | Event::Completed | Event::Cancelled | Event::Deadline => Ok(()),
        }
    }
}

/// Settles the declared process set before returning an observed stop or refusal.
fn stopped_after(child: &mut SupervisedChild, exit: Exit) -> Exit {
    match terminate(child) {
        Ok(()) => exit,
        Err(error) => Exit::SupervisionFailed(error),
    }
}

fn monitored(child: &mut SupervisedChild, path: &Path) -> Option<Exit> {
    let exit = match inspect_monitor(path) {
        MonitorState::Absent => return None,
        MonitorState::PresentRegular => Exit::StoppedByMonitor,
        MonitorState::InvalidType => Exit::MonitorFailed(MonitorError::InvalidType {
            path: path.to_path_buf(),
        }),
        MonitorState::InspectFailed(source) => Exit::MonitorFailed(MonitorError::Inspect {
            path: path.to_path_buf(),
            source,
        }),
    };
    Some(match terminate(child) {
        Ok(()) => exit,
        Err(error) => Exit::SupervisionFailed(error),
    })
}

/// How long from `now` until `moment`, and nothing once it has passed.
fn until(moment: Instant, now: Instant) -> Duration {
    moment.saturating_duration_since(now)
}

enum MonitorState {
    Absent,
    PresentRegular,
    InvalidType,
    InspectFailed(io::Error),
}

fn inspect_monitor(path: &Path) -> MonitorState {
    classify_monitor(std::fs::symlink_metadata(path))
}

fn classify_monitor(inspected: io::Result<std::fs::Metadata>) -> MonitorState {
    match inspected {
        Ok(metadata) if metadata.file_type().is_file() => MonitorState::PresentRegular,
        Ok(_metadata) => MonitorState::InvalidType,
        Err(error) if error.kind() == io::ErrorKind::NotFound => MonitorState::Absent,
        Err(error) => MonitorState::InspectFailed(error),
    }
}

/// Completes cooperative and forceful cleanup through the one mandatory group owner.
fn terminate(child: &mut SupervisedChild) -> Result<(), RunnerError> {
    let started = Instant::now();
    let settled = child.child.stop_with_grace(TERMINATION_GRACE);
    let note = measured_wait(
        child.id,
        "owned group cancellation, member exit and leader reap",
        started,
    );
    match (settled, note) {
        (Ok(()), Ok(note)) => {
            child.waits.push(note);
            Ok(())
        }
        (Err(source), Ok(note)) => {
            child.waits.push(note);
            Err(RunnerError::ProcessControlFailed {
                phase: TerminationPhase::Forceful,
                source,
            })
        }
        (Ok(()), Err(source)) => Err(RunnerError::ProcessWaitFailed { source }),
        (Err(cleanup), Err(measurement)) => Err(RunnerError::ProcessWaitFailed {
            source: io::Error::other(format!(
                "group cancellation: {cleanup}; wait measurement: {measurement}"
            )),
        }),
    }
}

fn measured_wait(id: u32, cause: &str, started: Instant) -> io::Result<WaitNote> {
    Ok(WaitNote {
        owner: format!("process-group:{id}"),
        cause: cause.to_owned(),
        elapsed_ns: u64::try_from(started.elapsed().as_nanos()).map_err(io::Error::other)?,
        machine: crate::observation::Machine {
            os: std::env::consts::OS,
            cpus: thread::available_parallelism()?.get(),
        },
    })
}

/// Says why the process set could not be owned, before the abort that says nothing.
///
/// `SIGABRT` names no place, and the site that fires on a machine the author does not have is the one worth naming.
#[cold]
fn note_ownership_failure(step: &str, why: &io::Error) {
    use io::Write as _;
    match writeln!(
        io::stderr(),
        "rust-mutants: {step}: {why}: the supervised process set could not be owned to the end, so this process ends rather than leave it running"
    ) {
        Ok(()) | Err(_) => {}
    }
}

#[cold]
fn terminal_process_ownership_failure() -> ! {
    std::process::abort();
}

#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use windows as sys;

pub use njutest_process::Membership;

/// How a process group is asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupStop {
    /// Asked to end, which a process may answer by cleaning up.
    Ask,
    /// Ended.
    Kill,
}

pub use rust_mutants_decision::group::{Delivered, Others, StopDecision, Stopped, decide_stop};

/// Stops every process of the group `leader` leads, which a [`GroupChild`] started and has not reaped, and says how much of it the stop reached.
///
/// A group already gone, or one whose members have all ended while its leader waits to be reaped, is reached whole: on macOS that group refuses a group signal with `EPERM`, and a look at the group finds nobody besides the leader.
///
/// # Errors
/// The kernel refuses the leader too, or fails for a reason other than its being gone.
#[cfg(unix)]
pub fn stop_group(leader: Leader<'_>, how: GroupStop) -> io::Result<Stopped> {
    unix::stop_group(leader, how)
}

/// Ends the one process `pid` at once, where it is still there: a process a run started that left every group it supervised.
///
/// # Errors
/// The process could not be signalled for a reason other than having ended.
pub fn stop_process(pid: u32) -> io::Result<()> {
    sys::stop_process(pid)
}

/// How long a forceful end waits to see the child reaped before aborting the supervising process.
pub const REAPING_GRACE: Duration = Duration::from_secs(10);

/// The mechanism this platform supervises with: `process-group` or `job-object`.
/// Diagnostic, for traces and `doctor`.
pub const SUPERVISOR_KIND: &str = sys::SUPERVISOR_KIND;

/// The containment boundary this platform supervisor enforces.
pub const SUPERVISION_BOUNDARY: SupervisionBoundary = sys::SUPERVISION_BOUNDARY;

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use njutest_devkit::result::ResultState::Refused;
    use njutest_devkit::result::{ResultState::Returned, result_state};

    use std::time::Duration;

    #[cfg(unix)]
    use super::{Bound, Cancel, RunResult, SIDE_CHANNEL_LIMIT, Spec, read_side_channel, run};
    use super::{
        MonitorState, ProcessExit, Progress, Termination, classify_monitor, inspect_monitor,
    };

    #[cfg(unix)]
    #[test]
    fn an_overdue_declared_wait_is_decided_before_the_ack_releases_it() {
        let events = tempfile::tempdir().expect("clock events");
        let monitor = tempfile::tempdir().expect("monitor events");

        let cancel = Cancel::new().with_clock(super::Clock::events(events.path().to_path_buf()));
        let prepared = njutest_process::PreparedGroup::new().expect("a supervisor");
        let mut command = logical_fixture_command(events.path(), "overdue");
        command.env("NJUTEST_LOGICAL_PROGRESS_STOP", monitor.path().join("stop"));
        let mut child =
            super::SupervisedChild::launch(prepared, &mut command, super::Reaping::Normal)
                .expect("starts");
        publish_logical_fixture(&events.path().join(child.id.to_string()), b"60000");
        let started = std::time::Instant::now();
        let observed = crate::observation::Observation::subscribe();
        let mut waits = Vec::new();
        let exit = super::await_exit(
            &mut child,
            super::Stops {
                started,
                deadline: Some(started + Duration::from_millis(200)),
                cancel: &cancel,
                monitor: Some(&monitor.path().join("stop")),
                progress: None,
                answered: None,
                observed: &observed,
                waits: &mut waits,
            },
            |quiet| quiet.is_zero(),
        );
        let acknowledged = std::fs::read(events.path().join(format!("{}.ack", child.id))).is_ok();
        let released_child = std::fs::read(monitor.path().join("stop")).is_ok();
        let status = child.reap_observed();
        assert!(
            status.is_ok(),
            "the complete producer set is settled and reaped: {status:?}"
        );
        drop(child);
        assert!(
            matches!(exit, super::Exit::TimedOut),
            "the overdue declaration must end the run at the bound: {status:?}"
        );
        assert!(
            !acknowledged,
            "an overdue declaration was acknowledged before the bound decided it, so the blocked child could have become a clean exit"
        );
        assert!(
            !released_child,
            "the blocked child was released by the acknowledgement and published a monitor stop"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_declared_wait_within_the_bound_is_acknowledged_and_the_child_finishes() {
        let events = tempfile::tempdir().expect("clock events");

        let cancel = Cancel::new().with_clock(super::Clock::events(events.path().to_path_buf()));
        let prepared = njutest_process::PreparedGroup::new().expect("a supervisor");
        let mut command = logical_fixture_command(events.path(), "within");
        let mut child =
            super::SupervisedChild::launch(prepared, &mut command, super::Reaping::Normal)
                .expect("starts");
        let started = std::time::Instant::now();
        let observed = crate::observation::Observation::subscribe();
        let mut waits = Vec::new();
        let exit = super::await_exit(
            &mut child,
            super::Stops {
                started,
                deadline: Some(started + Duration::from_secs(5)),
                cancel: &cancel,
                monitor: None,
                progress: None,
                answered: None,
                observed: &observed,
                waits: &mut waits,
            },
            |quiet| quiet.is_zero(),
        );
        let status = child.reap_observed();
        assert!(
            status.is_ok(),
            "the complete producer set is settled and reaped: {status:?}"
        );
        drop(child);
        assert!(
            matches!(exit, super::Exit::Exited),
            "an in-bound declaration is acknowledged and the child finishes: {status:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn progress_first_seen_under_an_advanced_clock_does_not_invent_earlier_quiet_time() {
        let events = tempfile::tempdir().expect("clock events");
        let signals = tempfile::tempdir().expect("progress signals");
        let monitor = tempfile::tempdir().expect("monitor events");

        let cancel = Cancel::new().with_clock(super::Clock::events(events.path().to_path_buf()));
        let prepared = njutest_process::PreparedGroup::new().expect("a supervisor");
        let mut command = logical_fixture_command(events.path(), "advanced");
        command.env("NJUTEST_LOGICAL_PROGRESS_STOP", monitor.path().join("stop"));
        let mut child =
            super::SupervisedChild::launch(prepared, &mut command, super::Reaping::Normal)
                .expect("starts");
        std::fs::write(signals.path().join("step"), b"1").expect("a step before the wait");
        std::fs::write(signals.path().join("beat"), b"1").expect("a beat before the wait");
        publish_logical_fixture(&events.path().join(child.id.to_string()), b"60000");
        let progress = Progress {
            path: signals.path().join("step"),
            beat: signals.path().join("beat"),
            quiet: Duration::from_millis(300),
        };
        let started = std::time::Instant::now();
        let observed = crate::observation::Observation::subscribe();
        let mut waits = Vec::new();
        let exit = super::await_exit(
            &mut child,
            super::Stops {
                started,
                deadline: None,
                cancel: &cancel,
                monitor: Some(&monitor.path().join("stop")),
                progress: Some(&progress),
                answered: None,
                observed: &observed,
                waits: &mut waits,
            },
            |quiet| quiet.is_zero(),
        );
        let acknowledged = std::fs::read(events.path().join(format!("{}.ack", child.id))).is_ok();
        let released_child = std::fs::read(monitor.path().join("stop")).is_ok();
        let status = child.reap_observed();
        assert!(
            status.is_ok(),
            "the complete producer set is settled and reaped: {status:?}"
        );
        drop(child);
        assert!(
            matches!(exit, super::Exit::StoppedByMonitor),
            "newly observed progress gets a full window and the actual child publishes its stop: {status:?}"
        );
        assert!(
            acknowledged,
            "the actual logical observation is acknowledged before its quiet window passes"
        );
        assert!(
            released_child,
            "the child publishes its real monitor stop before any complete quiet window"
        );
    }

    #[test]
    fn newly_observed_progress_gets_its_full_window_at_the_observed_logical_time() {
        let signals = tempfile::tempdir().expect("actual progress files");
        let progress = Progress {
            path: signals.path().join("step"),
            beat: signals.path().join("beat"),
            quiet: Duration::from_millis(300),
        };
        std::fs::write(&progress.path, b"1").expect("the actual progress");
        std::fs::write(&progress.beat, b"1").expect("the actual heartbeat");
        let started = std::time::Instant::now();
        let observed = started + Duration::from_secs(60);
        let mut watching = super::Watching::of(&progress, started);
        assert_eq!(
            watching.look(observed).expect("the actual observation"),
            observed + progress.quiet,
            "newly observed progress cannot be dated before the event that observed it"
        );
        assert!(
            !watching
                .confirms_stall(observed)
                .expect("the actual confirmation"),
            "the first observation proves no earlier quiet window"
        );
        assert!(
            watching
                .confirms_stall(observed + progress.quiet)
                .expect("the actual complete window"),
            "a complete observation-relative window still establishes stillness"
        );
    }

    #[test]
    fn an_invalid_progress_resource_does_not_establish_stillness() {
        let signals = tempfile::tempdir().expect("actual progress resources");
        let progress = Progress {
            path: signals.path().join("step"),
            beat: signals.path().join("beat"),
            quiet: Duration::from_millis(300),
        };
        std::fs::create_dir_all(&progress.path).expect("an actual invalid progress resource");
        let started = std::time::Instant::now();
        let observed = started + progress.quiet;
        let mut watching = super::Watching::of(&progress, started);
        assert!(
            matches!(
                watching.look(observed),
                Err(super::MonitorError::Inspect { .. })
            ),
            "an unreadable progress resource refuses a candidate rather than establishing stillness"
        );
        assert!(
            !watching
                .confirms_stall(observed)
                .expect("no first observation was established"),
            "an unreadable first observation proves no quiet window"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_gentle_stop_of_a_group_whose_leader_has_already_exited_is_no_failure() {
        let mut command = std::process::Command::new("true");
        let prepared = njutest_process::PreparedGroup::new().expect("a supervisor");
        let mut child =
            super::SupervisedChild::launch(prepared, &mut command, super::Reaping::Normal)
                .expect("true starts");
        assert!(
            child
                .child
                .completion()
                .wait(Some(super::REAPING_GRACE))
                .expect("the actual leader event")
        );
        let stopped = super::terminate(&mut child);
        let reaped = child.reap_observed();
        assert!(
            stopped.is_ok(),
            "a test process that printed its failure and exited before the stop arrived leaves a \
             group of one process that has ended and is not reaped yet, and macOS refuses a \
             signal to it with EPERM; read as a failure, it turned a kill the harness had \
             already named into an errored mutant: {stopped:?} {reaped:?}"
        );
    }

    #[test]
    fn monitor_inspection_distinguishes_absence_regular_files_and_invalid_types() {
        let directory = tempfile::tempdir();
        assert_eq!(result_state(&directory), Returned, "tempdir: {directory:?}");
        let Ok(directory) = directory else { return };
        let path = directory.path().join("notice");
        assert!(matches!(inspect_monitor(&path), MonitorState::Absent));

        let written = std::fs::write(&path, "notice");
        assert_eq!(result_state(&written), Returned, "notice: {written:?}");
        assert!(matches!(
            inspect_monitor(&path),
            MonitorState::PresentRegular
        ));

        let removed = std::fs::remove_file(&path);
        assert_eq!(
            result_state(&removed),
            Returned,
            "remove notice: {removed:?}"
        );
        let created = std::fs::create_dir_all(&path);
        assert_eq!(
            result_state(&created),
            Returned,
            "notice directory: {created:?}"
        );
        assert!(matches!(inspect_monitor(&path), MonitorState::InvalidType));
    }

    #[cfg(unix)]
    #[test]
    fn monitor_inspection_never_follows_a_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir();
        assert_eq!(result_state(&directory), Returned, "tempdir: {directory:?}");
        let Ok(directory) = directory else { return };
        let target = directory.path().join("target");
        let path = directory.path().join("notice");
        let written = std::fs::write(&target, "notice");
        assert_eq!(result_state(&written), Returned, "target: {written:?}");
        let linked = symlink(target, &path);
        assert_eq!(result_state(&linked), Returned, "symlink: {linked:?}");
        assert!(matches!(inspect_monitor(&path), MonitorState::InvalidType));
    }

    #[test]
    fn monitor_inspection_preserves_operating_system_failures() {
        let failure = classify_monitor(Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refused",
        )));
        assert!(matches!(failure, MonitorState::InspectFailed(_)));
    }

    #[test]
    fn a_planted_exit_after_a_named_failure_is_refused() {
        let termination = super::checked_answered_termination(
            super::Observed {
                wait: super::Exit::Exited.wait(),
                named_a_failure: true,
                capture_failed: false,
            },
            Termination::Exited(ProcessExit::Code(101)),
        );
        assert!(matches!(
            termination,
            Termination::WaitFailed {
                error: super::RunnerError::AnsweredStopInconsistent
            }
        ));
    }

    #[test]
    fn a_planted_answer_without_failure_evidence_is_refused() {
        let termination = super::checked_answered_termination(
            super::Observed {
                wait: super::Exit::Answered.wait(),
                named_a_failure: false,
                capture_failed: false,
            },
            Termination::Answered,
        );
        assert!(matches!(
            termination,
            Termination::WaitFailed {
                error: super::RunnerError::AnsweredStopInconsistent
            }
        ));
    }

    #[test]
    fn answered_stop_self_check_accepts_both_failure_endings() {
        for outcome in [super::Exit::Exited, super::Exit::Answered] {
            let termination = super::checked_answered_termination(
                super::Observed {
                    wait: outcome.wait(),
                    named_a_failure: true,
                    capture_failed: false,
                },
                Termination::Answered,
            );
            assert!(matches!(termination, Termination::Answered));
        }
    }

    #[test]
    fn a_failed_capture_cannot_be_overridden_by_a_named_test_failure() {
        let termination = super::checked_answered_termination(
            super::Observed {
                wait: super::Exit::Exited.wait(),
                named_a_failure: true,
                capture_failed: true,
            },
            Termination::WaitFailed {
                error: super::RunnerError::OutputReaderDisconnected { stream: "output" },
            },
        );
        assert!(matches!(
            termination,
            Termination::WaitFailed {
                error: super::RunnerError::OutputReaderDisconnected { stream: "output" }
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_named_its_failure_is_answered_whichever_ending_arrives_first() {
        let mut endings = Vec::new();
        for _ in 0..30 {
            let mut spec = Spec::new(
                [
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "printf 'test planted ... FAILED\\n'; exit 101".to_owned(),
                ],
                Bound::After(Duration::from_secs(30)),
            );
            spec.stop_at_first_failure = true;
            let ended = run(&spec, &Cancel::new());
            endings.push(format!("{:?}", ended.termination));
        }
        assert!(
            endings.iter().all(|ending| ending == "Answered"),
            "a run that asked to end at the first failure has its answer once that failure is \
             read, and whether the process then exited on its own or was stopped is a race whose \
             winner a report recorded as an exit code: {endings:?}"
        );
    }

    #[cfg(unix)]
    fn logical_fixture_command(root: &std::path::Path, mode: &str) -> std::process::Command {
        let mut command = std::process::Command::new(
            std::env::current_exe().expect("the actual compiled test fixture"),
        );
        command
            .args([
                "--exact",
                "runner::tests::logical_progress_fixture",
                "--nocapture",
            ])
            .env("NJUTEST_LOGICAL_PROGRESS_MODE", mode)
            .env("NJUTEST_LOGICAL_PROGRESS_ROOT", root);
        command
    }

    #[cfg(unix)]
    fn logical_fixture_spec(root: &std::path::Path, mode: &str, bound: Bound) -> Spec {
        let executable = std::env::current_exe().expect("the actual compiled test fixture");
        let mut spec = Spec::new(
            [
                executable.into_os_string(),
                "--exact".into(),
                "runner::tests::logical_progress_fixture".into(),
                "--nocapture".into(),
            ],
            bound,
        );
        spec.env = Some(crate::vars::Variables::of([
            ("NJUTEST_LOGICAL_PROGRESS_MODE".into(), mode.into()),
            (
                "NJUTEST_LOGICAL_PROGRESS_ROOT".into(),
                root.as_os_str().into(),
            ),
        ]));
        spec
    }

    #[cfg(unix)]
    fn publish_logical_fixture(path: &std::path::Path, bytes: &[u8]) {
        let pending = path.with_extension("pending");
        std::fs::write(&pending, bytes).expect("the actual fixture publication");
        std::fs::rename(pending, path).expect("the atomic fixture publication");
    }

    #[cfg(unix)]
    fn await_logical_fixture_ack(
        observed: &crate::observation::Observation,
        path: &std::path::Path,
        value: &[u8],
    ) {
        loop {
            match read_side_channel(path) {
                Ok(actual) if actual == value => return,
                Ok(_) => {}
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => panic!("the actual clock acknowledgement was refused: {source}"),
            }
            observed
                .wait(
                    "actual logical progress child",
                    "clock acknowledgement",
                    None,
                )
                .expect("the actual filesystem event")
                .event
                .expect("the acknowledgement producer did not fail");
        }
    }

    #[cfg(unix)]
    #[test]
    fn logical_progress_fixture() {
        let Ok(mode) = std::env::var("NJUTEST_LOGICAL_PROGRESS_MODE") else {
            return;
        };
        let root = std::path::PathBuf::from(
            std::env::var_os("NJUTEST_LOGICAL_PROGRESS_ROOT").expect("the actual clock directory"),
        );
        let observed = crate::observation::Observation::filesystem(&root, false)
            .expect("the acknowledgement subscription precedes every publication");
        let pid = std::process::id();
        let clock = root.join(pid.to_string());
        let acknowledged = root.join(format!("{pid}.ack"));
        if matches!(mode.as_str(), "overdue" | "within" | "advanced") {
            let value = if mode == "within" {
                b"100".as_slice()
            } else {
                b"60000".as_slice()
            };
            publish_logical_fixture(&clock, value);
            await_logical_fixture_ack(&observed, &acknowledged, value);
            if mode == "within" {
                return;
            }
            let stopped = std::env::var_os("NJUTEST_LOGICAL_PROGRESS_STOP")
                .expect("the real monitor publication path");
            publish_logical_fixture(std::path::Path::new(&stopped), b"stopped");
            loop {
                observed
                    .wait("actual released logical child", "owned cancellation", None)
                    .expect("the owned fixture wait")
                    .event
                    .expect("the actual fixture producer");
            }
        }
        let progress = root.join("progress");
        let beat = root.join("beat");
        let mut millis = 0_u64;
        let mut step = 0_u64;
        loop {
            match mode.as_str() {
                "beat" => {
                    publish_logical_fixture(&progress, b"0");
                    publish_logical_fixture(&beat, step.to_string().as_bytes());
                }
                "moving" | "ceiling" => {
                    publish_logical_fixture(&progress, step.to_string().as_bytes());
                }
                "quiet" | "unchanged" => publish_logical_fixture(&progress, b"1"),
                other => panic!("unknown actual progress fixture mode: {other}"),
            }
            let value = millis.to_string();
            publish_logical_fixture(&clock, value.as_bytes());
            await_logical_fixture_ack(&observed, &acknowledged, value.as_bytes());
            if matches!(mode.as_str(), "beat" | "moving") && step == 29 {
                return;
            }
            step = step
                .checked_add(1)
                .expect("the finite logical control step");
            millis = millis
                .checked_add(50)
                .expect("the logical observation width");
        }
    }

    #[cfg(unix)]
    fn watched(script: &str, quiet: Duration, ceiling: Duration) -> Option<RunResult> {
        watched_with(script, quiet, ceiling, |quiet| quiet.is_zero())
    }

    #[cfg(unix)]
    fn watched_with(
        script: &str,
        quiet: Duration,
        ceiling: Duration,
        stall_candidate: impl Fn(Duration) -> bool,
    ) -> Option<RunResult> {
        let directory = tempfile::tempdir();
        assert_eq!(result_state(&directory), Returned, "{directory:?}");
        let Ok(directory) = directory else {
            return None;
        };
        let file = directory.path().join("progress");
        let beat = directory.path().join("beat");
        let mut spec = logical_fixture_spec(directory.path(), script, Bound::After(ceiling));
        let cancel = Cancel::new().with_clock(super::Clock::events(directory.path().to_path_buf()));
        spec.progress = Some(Progress {
            path: file,
            beat,
            quiet,
        });
        Some(super::run_with_stall_candidate(
            &spec,
            &cancel,
            stall_candidate,
        ))
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_moves_only_its_beat_outlives_its_quiet_window() {
        let Some(result) = watched("beat", Duration::from_millis(500), Duration::from_secs(20))
        else {
            return;
        };
        assert!(
            matches!(
                result.termination,
                Termination::Exited(ProcessExit::Code(0))
            ),
            "the real child advances 50 logical milliseconds per acknowledged beat through the same quiet window: {:?} after {:?}",
            result.termination,
            result.duration
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_stall_cannot_stop_a_child_that_keeps_beating() {
        let planted = std::sync::atomic::AtomicBool::new(false);
        let result = watched_with(
            "beat",
            Duration::from_millis(500),
            Duration::from_secs(20),
            |_| {
                planted.store(true, std::sync::atomic::Ordering::SeqCst);
                true
            },
        );
        let Some(result) = result else { return };
        assert!(planted.load(std::sync::atomic::Ordering::SeqCst));
        assert!(
            matches!(
                result.termination,
                Termination::Exited(ProcessExit::Code(0))
            ),
            "a false stall decision cannot replace the exit of a child whose beat continues: {:?}",
            result.termination
        );
    }

    #[test]
    fn a_child_is_told_to_beat_well_inside_every_window() {
        for millis in [1, 2, 3, 4, 7, 100, 500, 5_000, 30_000, 3_600_000] {
            let quiet = Duration::from_millis(millis);
            let progress = Progress {
                path: std::path::PathBuf::from("state"),
                beat: std::path::PathBuf::from("beat"),
                quiet,
            };
            let every = progress.beat_every();
            assert!(
                every >= Duration::from_millis(1) && (every * 2 <= quiet || every == quiet),
                "a beat every {every:?} leaves a child moving in a {quiet:?} window time to be seen"
            );
            let (name, value) = progress.told();
            assert_eq!(name, crate::instrument::STEP_BEAT_ENV);
            assert_eq!(
                value,
                std::ffi::OsString::from(format!("{}@beat", every.as_millis())),
                "the child is told the interval in whole milliseconds and the file it rewrites"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_keeps_moving_outlives_its_quiet_window() {
        let Some(result) = watched(
            "moving",
            Duration::from_millis(500),
            Duration::from_secs(20),
        ) else {
            return;
        };
        assert!(
            matches!(
                result.termination,
                Termination::Exited(ProcessExit::Code(0))
            ),
            "the real child advances 50 logical milliseconds per acknowledged progress event without a quiet window: {:?} after {:?}",
            result.termination,
            result.duration
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_goes_quiet_is_stalled_long_before_its_ceiling() {
        let Some(result) = watched("quiet", Duration::from_millis(300), Duration::from_secs(20))
        else {
            return;
        };
        assert!(
            matches!(result.termination, Termination::Stalled),
            "{:?}",
            result.termination
        );
        assert!(
            result.duration < Duration::from_secs(10),
            "{:?}",
            result.duration
        );
    }

    #[cfg(unix)]
    #[test]
    fn rewriting_the_same_progress_is_not_moving() {
        let Some(result) = watched(
            "unchanged",
            Duration::from_millis(300),
            Duration::from_secs(20),
        ) else {
            return;
        };
        assert!(
            matches!(result.termination, Termination::Stalled),
            "{:?}",
            result.termination
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_never_stops_moving_is_ended_by_its_ceiling() {
        let Some(result) = watched(
            "ceiling",
            Duration::from_secs(5),
            Duration::from_millis(800),
        ) else {
            return;
        };
        assert!(
            matches!(result.termination, Termination::TimedOut),
            "{:?}",
            result.termination
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_swaps_its_progress_for_a_fifo_is_refused_without_blocking() {
        let directory = tempfile::tempdir().expect("the actual progress directory");
        let path = directory.path().join("progress");
        let script = format!(
            "printf 'fifo-ready\\n'; rm -f {0}; mkfifo {0}; cat < {0}",
            path.display()
        );
        let mut spec = Spec::new(["sh", "-c", &script], Bound::After(Duration::from_secs(20)));
        spec.progress = Some(Progress {
            path: path.clone(),
            beat: directory.path().join("beat"),
            quiet: Duration::from_millis(300),
        });
        let result = run(&spec, &Cancel::new());
        assert!(
            matches!(&result.termination,
            Termination::MonitorFailed { failure: super::MonitorError::Inspect { path: actual, source } }
            if actual == &path && source.kind() == std::io::ErrorKind::InvalidData),
            "an unreadable FIFO refuses the exact observation after owned group/pipe settlement: {:?}",
            result.termination
        );
        assert!(
            result.duration < Duration::from_secs(10),
            "{:?}",
            result.duration
        );
        assert_eq!(
            result.output, b"fifo-ready\n",
            "the real producer's complete pipe was drained before returning its refusal"
        );
        assert!(!result.stdout_truncated, "no output evidence was discarded");
    }

    #[cfg(unix)]
    #[test]
    fn a_side_channel_is_read_only_as_a_small_regular_file() {
        let directory = tempfile::tempdir();
        assert_eq!(result_state(&directory), Returned, "{directory:?}");
        let Ok(directory) = directory else {
            return;
        };
        let regular = directory.path().join("regular");
        let large = directory.path().join("large");
        assert_eq!(
            SIDE_CHANNEL_LIMIT,
            16 * 1024,
            "the large file is one byte over it"
        );
        let link = directory.path().join("link");
        let written = std::fs::write(&regular, b"state")
            .and_then(|()| std::fs::write(&large, vec![b'x'; 16 * 1024 + 1]))
            .and_then(|()| std::os::unix::fs::symlink(&regular, &link));
        assert_eq!(result_state(&written), Returned, "{written:?}");

        assert_eq!(
            read_side_channel(&regular).map_err(|error| error.kind()),
            Ok(b"state".to_vec())
        );
        assert_eq!(result_state(&read_side_channel(&large)), Refused);
        assert_eq!(result_state(&read_side_channel(&link)), Refused);
        assert_eq!(result_state(&read_side_channel(directory.path())), Refused);
    }
}
