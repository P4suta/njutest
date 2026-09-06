// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Starts one child process, supervises its whole process tree, and returns what happened.

pub mod output;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::io::{self, Read as _};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use output::TailBuffer;

pub use output::{DEFAULT_OUTPUT_LIMIT, HeadBuffer, MIN_OUTPUT_LIMIT, OUTPUT_TRUNCATED_PREFIX};

/// [`RunResult::exit_code`] when there is no exit status to report.
pub const EXIT_CODE_UNAVAILABLE: i32 = -1;

/// How long a POSIX process group is given to shut down after SIGTERM before it is sent SIGKILL. Windows has no equivalent phase.
pub const TERMINATION_GRACE: Duration = Duration::from_secs(2);

/// How long [`run`] waits for the output pipe to reach EOF after the child itself has exited.
pub const IO_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// A cooperative cancellation flag shared between the caller and a run.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A flag that is not yet cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Idempotent.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// The flag itself, so a composition root can raise it from a signal handler. This crate never installs one: a signal is the process's business, not a library's.
    #[must_use]
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
}

/// One process to run.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Spec {
    /// The argument vector, executable first. Each element becomes exactly one argument to the child. A bare program name is resolved through `PATH`; anything with a separator is used as given.
    pub argv: Vec<OsString>,
    /// The child's working directory. `None` means this process's directory.
    pub dir: Option<PathBuf>,
    /// The child's complete environment. `None` inherits this process's environment, which is convenient for one-shot probes; the engine composes the full set explicitly for mutant executions.
    pub env: Option<Vec<(OsString, OsString)>>,
    /// Bounds the child's wall-clock run time. `None` means no timeout.
    pub timeout: Option<Duration>,
    /// Caps the retained combined output in bytes. `None` selects [`DEFAULT_OUTPUT_LIMIT`]; anything below [`MIN_OUTPUT_LIMIT`] is raised to it so the truncation notice still fits inside the budget.
    pub output_limit: Option<usize>,
    /// Captures stdout on its own, head-capped at this many bytes, for a child that writes structured data (JSON lines) to stdout and chatter to stderr — `cargo metadata`, `cargo check --message-format=json`. `None` merges stdout into [`RunResult::output`] with stderr.
    pub structured_stdout: Option<usize>,
}

impl Spec {
    /// A spec for `argv` with every default.
    #[must_use]
    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }
}

/// A failure to start or supervise a process — never a process that ran and failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunnerError {
    /// The process tree could not be placed under supervision. Always fatal to the run: the engine does not execute a test binary it cannot guarantee it can kill.
    #[error("could not supervise the child process tree: {message}")]
    SupervisionUnavailable {
        /// What failed.
        message: String,
        /// The underlying failure, if any.
        #[source]
        source: Option<io::Error>,
    },
    /// The child could not be started at all.
    #[error("could not start {program}: {source}")]
    ProcessStartFailed {
        /// The program that was asked for.
        program: String,
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
}

/// What one [`run`] produced.
#[derive(Debug)]
#[non_exhaustive]
pub struct RunResult {
    /// The child's exit status, or [`EXIT_CODE_UNAVAILABLE`]. On POSIX a death by signal is reported as 128 + N.
    pub exit_code: i32,
    /// Whether [`Spec::timeout`] expired and the tree was killed. The only field that distinguishes a timeout from a cancellation.
    pub timed_out: bool,
    /// The wall-clock time the run took, supervision and killing included: the engine derives mutant timeouts from baseline durations, and a budget that excluded this overhead would be one the same work could exceed.
    pub duration: Duration,
    /// Combined stdout and stderr in the order the child wrote them, capped at the effective output limit by keeping the tail. Stderr alone when [`Spec::structured_stdout`] is set.
    pub output: Vec<u8>,
    /// The child's stdout when [`Spec::structured_stdout`] is set, head-capped at that many bytes; empty otherwise.
    pub stdout: Vec<u8>,
    /// Whether `stdout` was cut at the cap.
    pub stdout_truncated: bool,
    /// Set only when the process could not be started or supervised.
    pub error: Option<RunnerError>,
    /// The signal the process died from, on the platforms that have them. A process that exited normally, and every process on Windows, has none.
    pub signal: Option<i32>,
}

impl RunResult {
    /// Whether the process ran to completion with a zero exit status.
    #[must_use]
    pub const fn ok(&self) -> bool {
        self.error.is_none() && !self.timed_out && self.exit_code == 0
    }
}

/// What a supervised command runs under: the flag that stops it, and who hears that it ran.
///
/// A caller that keeps no record implements [`Watch::exec`] as nothing, so
/// the code that runs commands never branches on whether anybody is listening.
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
        self.trace.exec(crate::trace::ExecRecord::of(spec, result));
    }
}

/// Starts the process described by `spec`, supervises its whole process tree, and returns when it has finished, timed out, or been cancelled.
#[must_use]
pub fn run(spec: &Spec, cancel: &Cancel) -> RunResult {
    let started = Instant::now();
    let unavailable = |error: Option<RunnerError>, output: Vec<u8>| RunResult {
        exit_code: EXIT_CODE_UNAVAILABLE,
        timed_out: false,
        duration: started.elapsed(),
        output,
        stdout: Vec::new(),
        stdout_truncated: false,
        error,
        signal: None,
    };
    let Some(program) = spec.argv.first() else {
        return unavailable(
            Some(RunnerError::SpecInvalid {
                message: "has no argument vector",
            }),
            Vec::new(),
        );
    };
    if program.to_string_lossy().trim().is_empty() {
        return unavailable(
            Some(RunnerError::SpecInvalid {
                message: "has an empty executable name",
            }),
            Vec::new(),
        );
    }
    if cancel.is_cancelled() {
        return unavailable(None, Vec::new());
    }
    let Started {
        mut supervisor,
        tail,
        eof,
        head,
        mut child,
    } = match start(spec, program) {
        Ok(started) => started,
        Err(Failed { error, output }) => return unavailable(Some(error), output),
    };

    let (exit_sender, exited) = mpsc::channel::<io::Result<ExitStatus>>();
    let _wait_thread = thread::spawn(move || {
        let _sent = exit_sender.send(child.wait());
    });
    let deadline = spec
        .timeout
        .map(|timeout| started.checked_add(timeout).unwrap_or(started));
    let outcome = await_exit(&supervisor, &exited, deadline, cancel);
    let _drained = eof.recv_timeout(IO_DRAIN_GRACE);
    let (stdout, stdout_truncated) = head.map_or((Vec::new(), false), |(head, eof)| {
        let _drained = eof.recv_timeout(IO_DRAIN_GRACE);
        let (bytes, truncated, _total) = head.capture();
        (bytes, truncated)
    });
    supervisor.release();
    let output = tail.capture();
    let duration = started.elapsed();
    let (exit_code, timed_out, error, signal) = match outcome {
        Exit::Killed { timed_out } => (EXIT_CODE_UNAVAILABLE, timed_out, None, None),
        Exit::Status(Ok(status)) => (sys::exit_code(status), false, None, sys::signal(status)),
        Exit::Status(Err(source)) => (
            EXIT_CODE_UNAVAILABLE,
            false,
            Some(RunnerError::ProcessWaitFailed { source }),
            None,
        ),
    };
    RunResult {
        exit_code,
        timed_out,
        duration,
        output,
        stdout,
        stdout_truncated,
        error,
        signal,
    }
}

/// What [`start`] hands to the wait half of [`run`].
struct Started {
    supervisor: sys::Supervisor,
    tail: Arc<TailBuffer>,
    eof: mpsc::Receiver<()>,
    /// The separate stdout capture and its EOF signal, when requested.
    head: Option<(Arc<HeadBuffer>, mpsc::Receiver<()>)>,
    child: Child,
}

/// A start that failed, with whatever output was captured before it did.
struct Failed {
    error: RunnerError,
    output: Vec<u8>,
}

/// The first half of [`run`]: supervision, the pipes, the spawn, the reader threads, and adoption. On any failure the child, if any, is dead.
fn start(spec: &Spec, program: &OsString) -> Result<Started, Failed> {
    let failed = |error: RunnerError| Failed {
        error,
        output: Vec::new(),
    };
    let start_failed = |source: io::Error| {
        failed(RunnerError::ProcessStartFailed {
            program: program.to_string_lossy().into_owned(),
            source,
        })
    };
    let mut supervisor = sys::Supervisor::new().map_err(failed)?;
    let tail = Arc::new(TailBuffer::new(
        spec.output_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT),
    ));
    let Wired {
        mut command,
        merged,
        structured,
    } = match wire(spec, program) {
        Ok(wired) => wired,
        Err(source) => {
            supervisor.release();
            return Err(start_failed(source));
        }
    };
    supervisor.configure(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            supervisor.release();
            return Err(start_failed(source));
        }
    };
    drop(command);
    let head = structured.map(|(limit, reader)| {
        let head = Arc::new(HeadBuffer::new(limit));
        let capture = Arc::clone(&head);
        let eof = spawn_reader(reader, move |bytes| capture.write(bytes));
        (head, eof)
    });
    let capture = Arc::clone(&tail);
    let eof = spawn_reader(merged, move |bytes| capture.write(bytes));

    if let Err(error) = supervisor.adopt(&child) {
        let _killed = child.kill();
        let _reaped = child.wait();
        supervisor.release();
        let _drained = eof.recv_timeout(IO_DRAIN_GRACE);
        if let Some((_, eof)) = &head {
            let _drained = eof.recv_timeout(IO_DRAIN_GRACE);
        }
        return Err(Failed {
            error,
            output: tail.capture(),
        });
    }
    Ok(Started {
        supervisor,
        tail,
        eof,
        head,
        child,
    })
}

/// A command with its pipes attached: the merged reader, and the structured stdout reader with its cap when the spec asked for one.
struct Wired {
    command: Command,
    merged: io::PipeReader,
    structured: Option<(usize, io::PipeReader)>,
}

/// Builds the command and the pipes it writes to. No stdin: a test binary that reads from the terminal would hang. One pipe for both streams unless stdout is wanted whole, so the interleaving is the child's own.
fn wire(spec: &Spec, program: &OsString) -> io::Result<Wired> {
    let (merged, stderr) = io::pipe()?;
    let mut command = Command::new(program);
    command.args(spec.argv.iter().skip(1));
    if let Some(dir) = &spec.dir {
        command.current_dir(dir);
    }
    if let Some(env) = &spec.env {
        command.env_clear();
        command.envs(env.iter().map(|(key, value)| (key, value)));
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

/// Reads a pipe to EOF on its own thread, handing every chunk to `sink`, and signals EOF through the returned receiver.
fn spawn_reader(
    reader: io::PipeReader,
    sink: impl Fn(&[u8]) + Send + 'static,
) -> mpsc::Receiver<()> {
    let (eof_sender, eof) = mpsc::channel::<()>();
    let _reader_thread = thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => sink(buffer.get(..read).unwrap_or_default()),
            }
        }
        let _sent = eof_sender.send(());
    });
    eof
}

/// How the wait half of [`run`] ended.
enum Exit {
    /// The child was reaped on its own.
    Status(io::Result<ExitStatus>),
    /// The tree was killed, on a timeout or a cancellation.
    Killed {
        /// Whether the timeout, rather than the cancellation, did it.
        timed_out: bool,
    },
}

/// How often the wait loop looks at the cancellation flag.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Waits for the child to exit, the deadline to pass, or the cancellation flag to be raised — and in the latter two cases ends the tree.
fn await_exit(
    supervisor: &sys::Supervisor,
    exited: &mpsc::Receiver<io::Result<ExitStatus>>,
    deadline: Option<Instant>,
    cancel: &Cancel,
) -> Exit {
    loop {
        let remaining = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let poll = remaining.map_or(POLL_INTERVAL, |remaining| remaining.min(POLL_INTERVAL));
        match exited.recv_timeout(poll) {
            Ok(status) => return Exit::Status(status),
            Err(RecvTimeoutError::Disconnected) => {
                return Exit::Status(Err(io::Error::other(
                    "the wait thread ended without a status",
                )));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let expired = remaining.is_some_and(|remaining| remaining.is_zero());
        if expired || cancel.is_cancelled() {
            terminate(supervisor, exited);
            return Exit::Killed { timed_out: expired };
        }
    }
}

/// Ends the tree, politely first where the platform has a polite phase, and waits for the child to be reaped.
fn terminate(supervisor: &sys::Supervisor, exited: &mpsc::Receiver<io::Result<ExitStatus>>) {
    supervisor.terminate_gently();
    if exited.recv_timeout(TERMINATION_GRACE).is_ok() {
        return;
    }
    supervisor.terminate_forcefully();
    let _reaped = exited.recv();
}

#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use windows as sys;

/// The mechanism this platform supervises with: `process-group` or `job-object`. Diagnostic, for traces and `doctor`.
pub const SUPERVISOR_KIND: &str = sys::SUPERVISOR_KIND;
