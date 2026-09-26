// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Starts one child process, supervises the platform's declared process set, and returns what happened.

pub mod output;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use output::{OutputError, TailBuffer};

pub use output::{DEFAULT_OUTPUT_LIMIT, HeadBuffer, MIN_OUTPUT_LIMIT, OUTPUT_TRUNCATED_PREFIX};

/// The conventional stand-in used only by legacy report projections when there is no exit status to report.
pub const EXIT_CODE_UNAVAILABLE: i32 = -1;

/// How much stdout a short probe may retain: version banners and one-line paths are bounded well below this.
pub const PROBE_OUTPUT_LIMIT: usize = 64 * 1024;

/// The containment guarantee the platform supervisor can actually provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum SupervisionBoundary {
    /// POSIX process-group inheritance: members are forcefully signalled unless they deliberately leave the group with `setsid` or `setpgid`; the kernel need not finish an uninterruptible member before the runner returns.
    InheritedProcessGroup,
    /// An operating-system container that descendants cannot leave on their own.
    ContainedTree,
}

/// What the non-reaping leader observation established before a forceful process-set signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeaderObservation {
    /// The leader may still execute.
    Running,
    /// The leader has exited but remains waitable, pinning its numeric process identity.
    ExitedWaitable,
}

/// How long a POSIX process group is given to shut down after SIGTERM before it is sent SIGKILL.
/// Windows has no equivalent phase.
pub const TERMINATION_GRACE: Duration = Duration::from_secs(2);

/// How long [`run`] waits for the output pipe to reach EOF after the child itself has exited.
pub const IO_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// A cooperative cancellation flag shared between the caller and a run.
#[derive(Debug, Clone)]
pub struct Cancel {
    own: Arc<AtomicBool>,
    above: Vec<Arc<AtomicBool>>,
}

impl Cancel {
    /// A flag that is not yet cancelled.
    #[must_use]
    #[expect(
        clippy::new_without_default,
        reason = "an execution-control state must be constructed explicitly, never by a semantic Default"
    )]
    pub fn new() -> Self {
        Self {
            own: Arc::new(AtomicBool::new(false)),
            above: Vec::new(),
        }
    }

    /// A flag cancelled whenever this one is, whose own cancellation this one never sees: what a run that stops its own work raises, so a caller does not read that stop as having been interrupted.
    #[must_use]
    pub fn child(&self) -> Self {
        let mut above = self.above.clone();
        above.push(Arc::clone(&self.own));
        Self {
            own: Arc::new(AtomicBool::new(false)),
            above,
        }
    }

    /// Requests cancellation.
    /// Idempotent.
    pub fn cancel(&self) {
        self.own.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested, of this flag or of any it is a child of.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.own.load(Ordering::SeqCst) || self.above.iter().any(|flag| flag.load(Ordering::SeqCst))
    }

    /// The flag itself, so a composition root can raise it from a signal handler.
    /// This crate never installs one: a signal is the process's business, not a library's.
    #[must_use]
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.own)
    }
}

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
    pub env: Option<Vec<(OsString, OsString)>>,
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

/// How many beats the child is told to fit into one quiet window.
const BEATS_PER_WINDOW: u32 = 4;

impl Progress {
    /// How long the child may spend a reservation before it rewrites [`Progress::beat`]: a quarter of the window, and never less than a millisecond.
    pub(crate) fn beat_every(&self) -> Duration {
        let share = match self.quiet.checked_div(BEATS_PER_WINDOW) {
            Some(share) => share,
            None => self.quiet,
        };
        share.max(Duration::from_millis(1))
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
pub enum MonitorFailure {
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
        failure: MonitorFailure,
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
    Monitor(&'a MonitorFailure),
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
    }
}

/// Starts the process described by `spec`, supervises the platform's declared process set, and returns when it has finished, timed out, or been cancelled.
#[must_use]
pub fn run(spec: &Spec, cancel: &Cancel) -> RunResult {
    let started = Instant::now();
    let program = match preflight(spec, cancel, started) {
        Preflight::Ready(program) => program,
        Preflight::Done(result) => return result,
    };
    let deadline = match deadline_of(started, spec.timeout) {
        Ok(deadline) => deadline,
        Err(error) => return not_started(started, error, Vec::new()),
    };
    let answered = Arc::new(AtomicBool::new(false));
    let running = match start(spec, program, &answered) {
        Ok(started) => started,
        Err(Failed { error, output }) => return not_started(started, error, output),
    };
    let leader = running.child.handle().id();
    if let Some(leaders) = &spec.leaders {
        leaders.started(leader, running.supervisor.membership());
    }
    let outcome = await_exit(
        &running.supervisor,
        &running.child,
        Stops {
            deadline,
            cancel,
            monitor: spec.stop_file.as_deref(),
            progress: spec.progress.as_ref(),
            answered: spec.stop_at_first_failure.then_some(answered.as_ref()),
        },
    );
    let completed = complete(
        started,
        running,
        (
            outcome,
            spec.stop_at_first_failure.then_some(answered.as_ref()),
        ),
    );
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
    (outcome, answered): (Exit, Option<&AtomicBool>),
) -> RunResult {
    let Started {
        mut supervisor,
        merged,
        head,
        mut child,
    } = running;
    let leader = Some(child.handle().id());
    force_signal_or_abort(&supervisor, LeaderObservation::ExitedWaitable);
    let status = child.reap_observed();
    let released = release_supervisor(&mut supervisor);
    let merged_finish = merged.finish();
    let structured_finish = head.map(JoinedReader::finish);
    child.finish();
    let duration = started.elapsed();
    let named_a_failure = answered.is_some_and(|answered| answered.load(Ordering::SeqCst));
    let process_termination = match outcome {
        Exit::Exited if named_a_failure => Termination::Answered,
        Exit::TimedOut => Termination::TimedOut,
        Exit::Stalled => Termination::Stalled,
        Exit::StoppedByMonitor => Termination::StoppedByMonitor,
        Exit::Answered => Termination::Answered,
        Exit::MonitorFailed(failure) => Termination::MonitorFailed { failure },
        Exit::Cancelled => Termination::Cancelled { started: true },
        Exit::Exited => Termination::Exited(sys::process_exit(status)),
        Exit::WaitFailed(source) => Termination::WaitFailed {
            error: RunnerError::ProcessWaitFailed { source },
        },
        Exit::SupervisionFailed(error) => Termination::WaitFailed { error },
    };
    let mut capture_failure = match released {
        Ok(()) => None,
        Err(error) => Some(error),
    };
    let output = match finished_capture(merged_finish, &mut capture_failure) {
        Some(output) => output,
        None => Vec::new(),
    };
    let (stdout, stdout_truncated) = match structured_finish {
        None => (Vec::new(), false),
        Some(finished) => match finished_capture(finished, &mut capture_failure) {
            Some((bytes, truncated, _total)) => (bytes, truncated),
            None => (Vec::new(), false),
        },
    };
    let termination = capture_failure.map_or(process_termination, |error| {
        Termination::WaitFailed { error }
    });
    RunResult {
        termination,
        duration,
        output,
        stdout,
        stdout_truncated,
        leader,
    }
}

fn finished_capture<Output>(
    finished: Result<ReaderFinish<Output>, RunnerError>,
    failure: &mut Option<RunnerError>,
) -> Option<Output> {
    let ReaderFinish { capture, drain } = match finished {
        Ok(finished) => finished,
        Err(error) => {
            if failure.is_none() {
                *failure = Some(error);
            }
            return None;
        }
    };
    if let Err(error) = drain
        && failure.is_none()
    {
        *failure = Some(error);
    }
    match capture {
        Ok(capture) => Some(capture),
        Err(error) => {
            if failure.is_none() {
                *failure = Some(error);
            }
            None
        }
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
    }
}

/// What [`start`] hands to the wait half of [`run`].
struct Started {
    supervisor: sys::Supervisor,
    merged: JoinedReader<Vec<u8>>,
    /// The separate stdout capture, when requested.
    head: Option<JoinedReader<(Vec<u8>, bool, u64)>>,
    child: SupervisedChild,
}

type StructuredReader = Option<JoinedReader<(Vec<u8>, bool, u64)>>;

struct StartingReaders {
    merged: JoinedReader<Vec<u8>>,
    head: StructuredReader,
}

/// A child process that cannot be detached by dropping its raw handle.
#[derive(Debug)]
struct SupervisedChild {
    child: Child,
    reaping: Reaping,
}

impl SupervisedChild {
    fn launch(command: &mut Command, reaping: Reaping) -> io::Result<Self> {
        let child = command.spawn()?;
        Ok(Self { child, reaping })
    }

    const fn handle(&self) -> &Child {
        &self.child
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    fn exit_observed(&self) -> io::Result<bool> {
        sys::exit_observed(&self.child)
    }

    fn reap_observed(&mut self) -> ExitStatus {
        match self.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) | Err(_) => terminal_process_ownership_failure(),
        }
    }

    fn terminate_unadopted(&mut self) {
        let kill = self.child.kill();
        if !reap_or_abort(self, REAPING_GRACE) {
            terminal_process_ownership_failure();
        }
        match kill {
            Ok(()) | Err(_) => {}
        }
    }

    fn finish(self) {
        drop(self);
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_status)) => {}
            Ok(None) | Err(_) => terminal_process_ownership_failure(),
        }
    }
}

/// A start that failed, with whatever output was captured before it did.
struct Failed {
    error: RunnerError,
    output: Vec<u8>,
}

/// The first half of [`run`]: supervision, the pipes, the spawn, the reader threads, and adoption.
/// On any failure the child, if any, is dead.
fn start(spec: &Spec, program: &OsString, answered: &Arc<AtomicBool>) -> Result<Started, Failed> {
    let failed = |error: RunnerError| Failed {
        error,
        output: Vec::new(),
    };
    let start_failed = |source: io::Error| {
        failed(RunnerError::ProcessStartFailed {
            program: program.clone(),
            source,
        })
    };
    let mut supervisor = sys::Supervisor::new().map_err(failed)?;
    let Wired {
        mut command,
        merged,
        structured,
    } = match wire(spec, program) {
        Ok(wired) => wired,
        Err(source) => {
            let primary = start_failed(source);
            return match release_supervisor(&mut supervisor) {
                Ok(()) => Err(primary),
                Err(error) => Err(Failed {
                    error,
                    output: Vec::new(),
                }),
            };
        }
    };
    supervisor.configure(&mut command);
    let readers = match launch_readers(
        merged,
        structured,
        spec.output_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT),
        spec.stop_at_first_failure.then(|| Arc::clone(answered)),
    ) {
        Ok(readers) => readers,
        Err(error) => {
            drop(command);
            return match release_supervisor(&mut supervisor) {
                Ok(()) => Err(Failed {
                    error,
                    output: Vec::new(),
                }),
                Err(cleanup) => Err(Failed {
                    error: cleanup,
                    output: Vec::new(),
                }),
            };
        }
    };
    let child = match SupervisedChild::launch(&mut command, spec.reaping) {
        Ok(child) => child,
        Err(source) => {
            drop(command);
            let readers_finished = finish_readers(readers);
            let released = release_supervisor(&mut supervisor);
            let output = readers_finished.map_err(|error| Failed {
                error,
                output: Vec::new(),
            })?;
            if let Err(error) = released {
                return Err(Failed { error, output });
            }
            let mut failed = start_failed(source);
            failed.output = output;
            return Err(failed);
        }
    };
    drop(command);
    adopt(supervisor, child, readers)
}

fn adopt(
    mut supervisor: sys::Supervisor,
    mut child: SupervisedChild,
    readers: StartingReaders,
) -> Result<Started, Failed> {
    if let Err(error) = supervisor.adopt(child.handle()) {
        child.terminate_unadopted();
        let released = release_supervisor(&mut supervisor);
        let output = finish_readers(readers).map_err(|error| Failed {
            error,
            output: Vec::new(),
        })?;
        if let Err(error) = released {
            return Err(Failed { error, output });
        }
        return Err(Failed { error, output });
    }
    Ok(Started {
        supervisor,
        merged: readers.merged,
        head: readers.head,
        child,
    })
}

fn launch_readers(
    merged: io::PipeReader,
    structured: Option<(usize, io::PipeReader)>,
    output_limit: usize,
    answered: Option<Arc<AtomicBool>>,
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

fn finish_readers(readers: StartingReaders) -> Result<Vec<u8>, RunnerError> {
    let merged = readers.merged.finish();
    let structured = readers.head.map(JoinedReader::finish);
    let merged = merged?;
    if let Some(structured) = structured {
        structured?.drain?;
    }
    merged.drain?;
    merged.capture
}

/// The program to start, found on the search path the spec's own environment names.
///
/// # Errors
/// The name is bare and the environment's search path does not hold it.
fn resolved(spec: &Spec, program: &OsString) -> io::Result<OsString> {
    let Some(env) = &spec.env else {
        return Ok(program.clone());
    };
    crate::cargo::resolve_executable(Path::new(program), crate::vars::search_path(env).as_deref())
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
        command.envs(env.iter().map(|(key, value)| (key, value)));
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

/// How often a cancellable, nonblocking pipe reader checks whether its owner has stopped waiting.
const READER_POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy)]
enum ReaderInstruction {
    Stop,
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
    answered: Arc<AtomicBool>,
}

impl<C: ReaderCapture> ReaderCapture for FirstFailure<C> {
    type Output = C::Output;

    fn write(&mut self, bytes: &[u8]) -> Result<(), OutputError> {
        self.partial.extend_from_slice(bytes);
        while let Some(end) = self.partial.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            if says_a_test_failed(&line) {
                self.answered.store(true, Ordering::SeqCst);
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
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
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

struct ReaderFinish<Output> {
    capture: Result<Output, RunnerError>,
    drain: Result<(), RunnerError>,
}

/// A pipe reader whose bounded owner always joins the one thread it creates.
#[derive(Debug)]
struct JoinedReader<Output: Send + 'static> {
    stream: &'static str,
    stop: SyncSender<ReaderInstruction>,
    completed: mpsc::Receiver<ReaderFinish<Output>>,
    handle: Option<JoinHandle<()>>,
}

impl<Output: Send + 'static> JoinedReader<Output> {
    fn launch<C: ReaderCapture<Output = Output>>(
        mut reader: io::PipeReader,
        stream: &'static str,
        mut capture: C,
    ) -> Result<Self, RunnerError> {
        sys::configure_reader(&reader)
            .map_err(|source| RunnerError::OutputReaderConfigurationFailed { stream, source })?;
        let (stop, instructions) = mpsc::sync_channel::<ReaderInstruction>(1);
        let (completion, completed) = mpsc::sync_channel::<ReaderFinish<Output>>(1);
        let handle = thread::Builder::new()
            .name(format!("rust-mutants-{stream}"))
            .spawn(move || {
                let mut buffer = [0u8; 8192];
                let drain = loop {
                    match instructions.try_recv() {
                        Ok(ReaderInstruction::Stop) | Err(TryRecvError::Disconnected) => {
                            break Ok(());
                        }
                        Err(TryRecvError::Empty) => {}
                    }
                    let outcome = reader.read(&mut buffer);
                    match outcome {
                        Ok(0) => match sys::stream_ended(&reader) {
                            Ok(true) => break Ok(()),
                            Ok(false) => thread::sleep(READER_POLL_INTERVAL),
                            Err(source) => break Err(RunnerError::OutputReadFailed { source }),
                        },
                        Err(source) if source.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(READER_POLL_INTERVAL);
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
                let finished = ReaderFinish {
                    capture: capture.finish().map_err(output_error),
                    drain,
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
        match self.stop.try_send(ReaderInstruction::Stop) {
            Ok(())
            | Err(
                TrySendError::Full(ReaderInstruction::Stop)
                | TrySendError::Disconnected(ReaderInstruction::Stop),
            ) => {}
        }
    }

    fn finish(mut self) -> Result<ReaderFinish<Output>, RunnerError> {
        let finished = match self.completed.recv_timeout(IO_DRAIN_GRACE) {
            Ok(finished) => finished,
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
                finished.drain = Err(RunnerError::OutputDrainTimedOut {
                    stream: self.stream,
                });
                finished
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.join()?;
                return Err(RunnerError::OutputReaderDisconnected {
                    stream: self.stream,
                });
            }
        };
        self.join()?;
        Ok(finished)
    }

    fn join(&mut self) -> Result<(), RunnerError> {
        let handle = self
            .handle
            .take()
            .ok_or(RunnerError::OutputReaderOwnershipLost {
                stream: self.stream,
            })?;
        await_reader_exit_or_abort(&handle);
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
            Ok(finished) => drop(finished),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                terminal_reader_ownership_failure();
            }
        }
        await_reader_exit_or_abort(&handle);
        if handle.join().is_err() {
            terminal_reader_ownership_failure();
        }
    }
}

fn await_reader_exit_or_abort(handle: &JoinHandle<()>) {
    let started = Instant::now();
    while !handle.is_finished() {
        if started.elapsed() >= IO_DRAIN_GRACE {
            terminal_reader_ownership_failure();
        }
        thread::sleep(READER_POLL_INTERVAL);
    }
}

#[cold]
fn terminal_reader_ownership_failure() -> ! {
    std::process::abort();
}

const fn output_error(source: OutputError) -> RunnerError {
    RunnerError::OutputCaptureFailed { source }
}

fn release_supervisor(supervisor: &mut sys::Supervisor) -> Result<(), RunnerError> {
    supervisor
        .release()
        .map_err(|source| RunnerError::SupervisorReleaseFailed { source })
}

/// How the wait half of [`run`] ended.
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
    MonitorFailed(MonitorFailure),
    /// Stopping or reaping the child failed, so the triggering event cannot be reported as a trustworthy termination.
    SupervisionFailed(RunnerError),
}

/// How often the wait loop looks at the cancellation flag.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Copy)]
struct Stops<'a> {
    deadline: Option<Instant>,
    cancel: &'a Cancel,
    monitor: Option<&'a Path>,
    progress: Option<&'a Progress>,
    answered: Option<&'a AtomicBool>,
}

/// What the wait loop last saw of each progress file, and when it last saw any of them change.
struct Watching<'a> {
    progress: &'a Progress,
    seen: [Option<Vec<u8>>; 2],
    moved: Instant,
}

impl<'a> Watching<'a> {
    const fn of(progress: &'a Progress, started: Instant) -> Self {
        Self {
            progress,
            seen: [None, None],
            moved: started,
        }
    }

    /// Looks at the files and returns the moment they count as stalled; a failed read is not a change, since a child that is not writing never causes one.
    fn look(&mut self, now: Instant) -> Option<Instant> {
        for (path, seen) in self.progress.signals().into_iter().zip(&mut self.seen) {
            match read_between_writes(path) {
                Ok(content) if seen.as_ref() != Some(&content) => {
                    *seen = Some(content);
                    self.moved = now;
                }
                Ok(_unchanged) => {}
                Err(_a_failed_read_is_not_a_change) => {}
            }
        }
        self.moved.checked_add(self.progress.quiet)
    }
}

/// The most a side-channel file the supervised process writes may hold before reading it is refused.
pub(crate) const SIDE_CHANNEL_LIMIT: u64 = 16 * 1024;

/// How many times a read the writer's lock refused is tried again before it counts as failed.
const LOCKED_READ_ATTEMPTS: u32 = 16;

/// Reads a side-channel file, trying again while the writer's lock refuses it, since the lock is only held for one write.
fn read_between_writes(path: &Path) -> io::Result<Vec<u8>> {
    for _refused in 1..LOCKED_READ_ATTEMPTS {
        match read_side_channel(path) {
            Err(error) if held_by_writer(&error) => thread::sleep(Duration::from_millis(1)),
            read => return read,
        }
    }
    read_side_channel(path)
}

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

#[cfg(unix)]
const fn held_by_writer(_error: &io::Error) -> bool {
    false
}

#[cfg(windows)]
fn held_by_writer(error: &io::Error) -> bool {
    match (
        error.raw_os_error(),
        i32::try_from(windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION),
    ) {
        (Some(code), Ok(violation)) => code == violation,
        (None, _) => false,
        (Some(_), Err(_no_such_code_fits)) => false,
    }
}

/// The stop a harness's first failing test asks for, where the run asked to end there and it has.
fn answered(
    supervisor: &sys::Supervisor,
    child: &SupervisedChild,
    answered: Option<&AtomicBool>,
) -> Option<Exit> {
    answered
        .is_some_and(|answered| answered.load(Ordering::SeqCst))
        .then(|| match terminate(supervisor, child) {
            Ok(()) => Exit::Answered,
            Err(error) => Exit::SupervisionFailed(error),
        })
}

/// Waits for the child to exit, the deadline to pass, or the cancellation flag to be raised — and in the latter two cases ends the declared process set.
fn await_exit(supervisor: &sys::Supervisor, child: &SupervisedChild, stops: Stops<'_>) -> Exit {
    let mut watching = stops
        .progress
        .map(|progress| Watching::of(progress, Instant::now()));
    loop {
        match child.exit_observed() {
            Ok(true) => return Exit::Exited,
            Ok(false) => {}
            Err(source) => {
                return match terminate(supervisor, child) {
                    Ok(()) => Exit::WaitFailed(source),
                    Err(error) => Exit::SupervisionFailed(error),
                };
            }
        }
        let now = Instant::now();
        let remaining = stops.deadline.map(|deadline| until(deadline, now));
        let quiet = watching
            .as_mut()
            .and_then(|watching| watching.look(now))
            .map(|stalled| until(stalled, now));
        let poll = match (remaining, quiet) {
            (Some(remaining), Some(quiet)) => remaining.min(quiet),
            (Some(sooner), None) | (None, Some(sooner)) => sooner,
            (None, None) => POLL_INTERVAL,
        }
        .min(POLL_INTERVAL);
        if stops.cancel.is_cancelled() {
            return match terminate(supervisor, child) {
                Ok(()) => Exit::Cancelled,
                Err(error) => Exit::SupervisionFailed(error),
            };
        }
        if let Some(answered) = answered(supervisor, child, stops.answered) {
            return answered;
        }
        if let Some(path) = stops.monitor {
            match inspect_monitor(path) {
                MonitorState::Absent => {}
                MonitorState::PresentRegular => {
                    return match terminate(supervisor, child) {
                        Ok(()) => Exit::StoppedByMonitor,
                        Err(error) => Exit::SupervisionFailed(error),
                    };
                }
                MonitorState::InvalidType => {
                    return match terminate(supervisor, child) {
                        Ok(()) => Exit::MonitorFailed(MonitorFailure::InvalidType {
                            path: path.to_path_buf(),
                        }),
                        Err(error) => Exit::SupervisionFailed(error),
                    };
                }
                MonitorState::InspectFailed(source) => {
                    return match terminate(supervisor, child) {
                        Ok(()) => Exit::MonitorFailed(MonitorFailure::Inspect {
                            path: path.to_path_buf(),
                            source,
                        }),
                        Err(error) => Exit::SupervisionFailed(error),
                    };
                }
            }
        }
        if remaining.is_some_and(|remaining| remaining.is_zero()) {
            return match terminate(supervisor, child) {
                Ok(()) => Exit::TimedOut,
                Err(error) => Exit::SupervisionFailed(error),
            };
        }
        if quiet.is_some_and(|quiet| quiet.is_zero()) {
            return match terminate(supervisor, child) {
                Ok(()) => Exit::Stalled,
                Err(error) => Exit::SupervisionFailed(error),
            };
        }
        thread::sleep(poll);
    }
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

/// Signals the supervised process set, politely first where the platform has a polite phase, and waits a bounded time for the leader to become waitable.
///
/// The wait after the forceful signal is about the leader only.
/// Exhausting its bound is a terminal ownership failure because this owner cannot drop a live leader.
/// On POSIX, another inherited group member may remain in an uninterruptible kernel wait after receiving SIGKILL; the process-group boundary promises signal delivery, not kernel quiescence.
fn terminate(supervisor: &sys::Supervisor, child: &SupervisedChild) -> Result<(), RunnerError> {
    let gentle = supervisor.terminate_gently();
    let leader_exited_during_grace = gentle.is_ok() && reap_or_abort(child, TERMINATION_GRACE);

    let leader = if leader_exited_during_grace {
        LeaderObservation::ExitedWaitable
    } else {
        LeaderObservation::Running
    };
    let forceful = supervisor.terminate_forcefully(leader);
    if !leader_exited_during_grace && !reap_or_abort(child, REAPING_GRACE) {
        terminal_process_ownership_failure();
    }
    match (gentle, forceful) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(source), Ok(())) => Err(RunnerError::ProcessControlFailed {
            phase: TerminationPhase::Gentle,
            source,
        }),
        (Ok(()), Err(source)) => Err(RunnerError::ProcessControlFailed {
            phase: TerminationPhase::Forceful,
            source,
        }),
        (Err(gentle), Err(forceful)) => {
            Err(RunnerError::ProcessControlSequenceFailed { gentle, forceful })
        }
    }
}

fn force_signal_or_abort(supervisor: &sys::Supervisor, leader: LeaderObservation) {
    if let Err(why) = supervisor.terminate_forcefully(leader) {
        note_ownership_failure(
            &format!(
                "signalling the process group forcefully, {}",
                supervisor.state()
            ),
            &why,
        );
        terminal_process_ownership_failure();
    }
}

fn reap_or_abort(child: &SupervisedChild, bound: Duration) -> bool {
    let started = Instant::now();
    loop {
        match child.reaping {
            Reaping::Normal => {}
            #[cfg(any(test, feature = "testkit"))]
            Reaping::SimulatedUnreapable => return false,
        }
        match child.exit_observed() {
            Ok(true) => return true,
            Ok(false) => {}
            Err(_source) => terminal_process_ownership_failure(),
        }
        let elapsed = started.elapsed();
        if elapsed >= bound {
            return false;
        }
        let Some(remaining) = bound.checked_sub(elapsed) else {
            return false;
        };
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
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

pub use sys::Membership;

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
    use super::{
        Bound, Cancel, ProcessExit, RunResult, SIDE_CHANNEL_LIMIT, Spec, Termination,
        read_side_channel, run,
    };
    use super::{MonitorState, Progress, classify_monitor, inspect_monitor};

    #[cfg(unix)]
    #[test]
    fn a_gentle_stop_of_a_group_whose_leader_has_already_exited_is_no_failure() {
        let mut command = std::process::Command::new("true");
        let mut supervisor = super::sys::Supervisor::new().expect("a supervisor");
        supervisor.configure(&mut command);
        let mut child = super::SupervisedChild::launch(&mut command, super::Reaping::Normal)
            .expect("true starts");
        supervisor
            .adopt(child.handle())
            .expect("the group is adopted");
        let started = std::time::Instant::now();
        while !child.exit_observed().expect("the leader can be observed") {
            assert!(started.elapsed() < Duration::from_secs(10), "true exits");
            std::thread::yield_now();
        }
        let stopped = supervisor.terminate_gently();
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
    fn watched(script: &str, quiet: Duration, ceiling: Duration) -> Option<RunResult> {
        let directory = tempfile::tempdir();
        assert_eq!(result_state(&directory), Returned, "{directory:?}");
        let Ok(directory) = directory else {
            return None;
        };
        let file = directory.path().join("progress");
        let beat = directory.path().join("beat");
        let mut spec = Spec::new(
            [
                "sh".to_owned(),
                "-c".to_owned(),
                script
                    .replace("PROGRESS", &file.display().to_string())
                    .replace("BEAT", &beat.display().to_string()),
            ],
            Bound::After(ceiling),
        );
        spec.progress = Some(Progress {
            path: file,
            beat,
            quiet,
        });
        Some(run(&spec, &Cancel::new()))
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_moves_only_its_beat_outlives_its_quiet_window() {
        let Some(result) = watched(
            "echo 0 > PROGRESS; i=0; while [ $i -lt 30 ]; do echo $i > BEAT; i=$((i+1)); sleep 0.05; done",
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
            "a child spending one reservation for a second and a half leaves its state alone and \
             rewrites its beat every fifty milliseconds, so it is never quiet for half of one: \
             {:?} after {:?}",
            result.termination,
            result.duration
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
            "i=0; while [ $i -lt 30 ]; do echo $i > PROGRESS; i=$((i+1)); sleep 0.05; done",
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
            "a child rewriting its progress every fifty milliseconds for a second and a half \
             is never quiet for half of one: {:?} after {:?}",
            result.termination,
            result.duration
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_goes_quiet_is_stalled_long_before_its_ceiling() {
        let Some(result) = watched(
            "echo 1 > PROGRESS; sleep 30",
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
            "while true; do echo 1 > PROGRESS.next; mv PROGRESS.next PROGRESS; sleep 0.05; done",
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
            "i=0; while true; do echo $i > PROGRESS; i=$((i+1)); sleep 0.05; done",
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
    fn a_child_that_swaps_its_progress_for_a_fifo_is_stalled_rather_than_waited_on() {
        let Some(result) = watched(
            "rm -f PROGRESS; mkfifo PROGRESS; sleep 30",
            Duration::from_millis(300),
            Duration::from_secs(20),
        ) else {
            return;
        };
        assert!(
            matches!(result.termination, Termination::Stalled),
            "opening a FIFO to read it would block the supervisor: {:?}",
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
