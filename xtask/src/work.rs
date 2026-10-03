// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Work in a process group of its own, stopped whole when a budget or a signal says so, and reaped on every path.

use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use thiserror::Error;

/// How long work that was asked to stop has before its whole group is killed.
const GRACE: Duration = Duration::from_secs(5);

/// How a piece of work ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// It exited by itself.
    Exited(ExitStatus),
    /// It outlived its ceiling and was stopped with everything in its group.
    OverBudget {
        /// How long it had run when it was stopped.
        elapsed: Duration,
    },
    /// It said nothing for longer than its bound allows and was stopped with everything in its group.
    Quiet {
        /// How long it had been silent when it was stopped.
        silent: Duration,
    },
    /// This process was asked to stop, and stopped the work first.
    Interrupted {
        /// The signal that asked.
        signal: i32,
    },
}

/// Why work could not be started, watched, or stopped.
#[derive(Debug, Error)]
pub enum WorkError {
    /// The program could not be started.
    #[error("{program} could not be started: {source}")]
    Start {
        /// The program.
        program: String,
        /// The operating system's refusal.
        source: std::io::Error,
    },
    /// The work could not be looked at or stopped.
    #[error("the work could not be watched or stopped: {source}")]
    Watch {
        /// The operating system's refusal.
        source: std::io::Error,
    },
    /// The signals that stop the work could not be armed.
    #[error("the signals that stop the work could not be armed: {source}")]
    Signals {
        /// Why.
        source: std::io::Error,
    },
    /// The original native leader was reached while another retained member refused cancellation.
    #[cfg(unix)]
    #[error("the retained process group was reached only in part: {source}")]
    Outlived {
        /// Every actual native member refusal.
        source: std::io::Error,
    },
}

impl crate::error::Coded for WorkError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Start { .. } | Self::Watch { .. } | Self::Signals { .. } => {
                crate::error::XtCode::WorkUnrun
            }
            #[cfg(unix)]
            Self::Outlived { .. } => crate::error::XtCode::WorkUnrun,
        }
    }
}

impl WorkError {
    fn kind(&self) -> std::io::ErrorKind {
        match self {
            Self::Start { source, .. } | Self::Watch { source } | Self::Signals { source } => {
                source.kind()
            }
            #[cfg(unix)]
            Self::Outlived { source } => source.kind(),
        }
    }
}

/// The signals that publish a stop observation before this process ends its work.
#[derive(Debug)]
pub struct Stops {
    state: Arc<StopState>,
    worker: SignalThread,
}

#[derive(Debug, Default)]
struct StopState {
    raised: std::sync::atomic::AtomicI32,
    observers: Mutex<Vec<std::sync::Weak<StopObserver>>>,
    heard: Mutex<Option<Instant>>,
    waits: Mutex<Vec<crate::observation::WaitNote>>,
    failure: Mutex<Option<Arc<std::io::Error>>>,
}

#[derive(Debug)]
struct StopObserver {
    signal: crate::observation::Signal,
    invalidation: crate::observation::Invalidation,
}

impl StopState {
    fn publish_failure(&self, source: &Arc<std::io::Error>) {
        let mut observers = match self.observers.lock() {
            Ok(observers) => observers,
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        };
        observers.retain(|observer| match observer.upgrade() {
            Some(observer) => {
                observer
                    .signal
                    .failed(std::io::Error::new(source.kind(), Arc::clone(source)));
                true
            }
            None => false,
        });
    }

    fn publish(&self, event: crate::observation::Event) {
        let mut observers = match self.observers.lock() {
            Ok(observers) => observers,
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        };
        observers.retain(|observer| match observer.upgrade() {
            Some(observer) => {
                match event {
                    crate::observation::Event::Changed => observer.invalidation.changed(),
                    crate::observation::Event::Cancelled
                    | crate::observation::Event::Completed
                    | crate::observation::Event::Deadline => observer.signal.publish(event),
                }
                true
            }
            None => false,
        });
    }
}

impl Stops {
    /// Arms producer-owned signal observations and retains their blocking worker until disposal.
    ///
    /// # Errors
    /// The signal subscription or its owned worker could not be started.
    pub fn arm() -> Result<Self, WorkError> {
        let state = Arc::new(StopState::default());
        let signals = signal_hook::iterator::Signals::new(STOPPING)
            .map_err(|source| WorkError::Signals { source })?;
        let worker = SignalThread::launch(signals, Arc::clone(&state))
            .map_err(|source| WorkError::Signals { source })?;
        Ok(Self { state, worker })
    }

    /// The actual signal most recently published by the operating system.
    #[must_use]
    pub fn raised(&self) -> Option<i32> {
        let signal = self.state.raised.load(Ordering::SeqCst);
        (signal != 0).then_some(signal)
    }

    /// The output producer endpoint retained before a command's output capture starts.
    #[must_use]
    pub fn events(&self) -> WorkEvents {
        WorkEvents {
            state: Arc::clone(&self.state),
        }
    }

    /// Takes every measured wait for the executing parent's command receipt.
    #[must_use]
    pub fn take_waits(&self) -> Vec<crate::observation::WaitNote> {
        match self.state.waits.lock() {
            Ok(mut waits) => std::mem::take(&mut *waits),
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
    }

    pub(crate) fn record(&self, note: crate::observation::WaitNote) {
        match self.state.waits.lock() {
            Ok(mut waits) => waits.push(note),
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
    }

    pub(crate) fn subscribe(
        &self,
        observation: &crate::observation::Observation,
    ) -> StopSubscription {
        let signal = Arc::new(StopObserver {
            signal: observation.signal(),
            invalidation: observation.invalidation(),
        });
        match self.state.observers.lock() {
            Ok(mut observers) => {
                observers.retain(|observer| observer.strong_count() != 0);
                observers.push(Arc::downgrade(&signal));
            }
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
        if self.raised().is_some() {
            signal.signal.publish(crate::observation::Event::Cancelled);
        }
        match self.state.failure.lock() {
            Ok(failure) => {
                if let Some(source) = failure.as_ref() {
                    signal
                        .signal
                        .failed(std::io::Error::new(source.kind(), Arc::clone(source)));
                }
            }
            Err(source) => {
                eprintln!("work subscription failure ownership was poisoned: {source}");
                std::process::abort();
            }
        }
        StopSubscription { _signal: signal }
    }

    fn failed(&self) -> Result<(), WorkError> {
        match self.state.failure.lock() {
            Ok(failure) => match failure.as_ref() {
                Some(source) => Err(WorkError::Watch {
                    source: std::io::Error::new(source.kind(), Arc::clone(source)),
                }),
                None => Ok(()),
            },
            Err(source) => {
                eprintln!("work failure ownership was poisoned: {source}");
                std::process::abort();
            }
        }
    }

    fn heard(&self) -> Option<Instant> {
        match self.state.heard.lock() {
            Ok(mut heard) => heard.take(),
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
    }
}

impl Drop for Stops {
    fn drop(&mut self) {
        self.worker.finish();
    }
}

#[derive(Debug)]
pub(crate) struct StopSubscription {
    _signal: Arc<StopObserver>,
}

/// An owned endpoint through which the output producer publishes its actual arrival time.
#[derive(Debug, Clone)]
pub struct WorkEvents {
    state: Arc<StopState>,
}

impl WorkEvents {
    /// Retains the producer's first actual refusal before waking every active observer.
    pub fn failed(&self, source: std::io::Error) {
        let retained = match self.state.failure.lock() {
            Ok(mut failure) => Arc::clone(failure.get_or_insert_with(|| Arc::new(source))),
            Err(source) => {
                eprintln!("work producer failure publication was poisoned: {source}");
                std::process::abort();
            }
        };
        self.state.publish_failure(&retained);
    }

    /// Publishes actual output before waking the work observer.
    pub fn heard(&self) {
        match self.state.heard.lock() {
            Ok(mut heard) => *heard = Some(Instant::now()),
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
        self.state.publish(crate::observation::Event::Changed);
    }
}

#[derive(Debug)]
struct SignalThread {
    handle: Option<std::thread::JoinHandle<()>>,
    close: signal_hook::iterator::Handle,
}

impl SignalThread {
    fn launch(
        mut signals: signal_hook::iterator::Signals,
        state: Arc<StopState>,
    ) -> std::io::Result<Self> {
        let close = signals.handle();
        let handle = std::thread::Builder::new()
            .name("xtask-stop-observations".to_owned())
            .spawn(move || {
                for signal in signals.forever() {
                    state.raised.store(signal, Ordering::SeqCst);
                    state.publish(crate::observation::Event::Cancelled);
                }
            })?;
        Ok(Self {
            handle: Some(handle),
            close,
        })
    }

    fn finish(&mut self) {
        self.close.close();
        if let Some(handle) = self.handle.take()
            && let Err(panic) = handle.join()
        {
            drop(panic);
            std::process::abort();
        }
    }
}

impl Drop for SignalThread {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(unix)]
const STOPPING: [i32; 3] = [
    signal_hook::consts::SIGINT,
    signal_hook::consts::SIGTERM,
    signal_hook::consts::SIGHUP,
];

#[cfg(not(unix))]
const STOPPING: [i32; 2] = [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM];

/// How long work may run, bounded by quiet first (ADR 0026) and by a ceiling behind it.
pub struct Bound<'a> {
    /// The longest the work may run at all, however much it says.
    pub ceiling: Duration,
    /// The longest the work may go without saying anything.
    pub quiet: Duration,
    /// Passes on whatever the work has said since it was last asked, and answers whether it said anything.
    pub heard: &'a mut dyn FnMut() -> std::io::Result<bool>,
}

impl std::fmt::Debug for Bound<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Bound")
            .field("ceiling", &self.ceiling)
            .field("quiet", &self.quiet)
            .finish_non_exhaustive()
    }
}

/// Runs, stops the complete producer group, joins its completion observer and reaps its leader before returning any result.
///
/// # Errors
/// The command, output callback, process observation or complete group cleanup failed.
pub fn run<F>(
    command: &mut Command,
    bound: Option<&mut Bound<'_>>,
    stops: &Stops,
    started: F,
) -> Result<Ended, WorkError>
where
    F: FnOnce(u32) -> std::io::Result<()>,
{
    run_with_custody(
        Request {
            command,
            bound,
            stops,
            custody: Custody::Direct,
        },
        started,
    )
}

/// The actual command, semantic bounds and one consumed native custody transition.
#[derive(Debug)]
pub struct Request<'run, 'bound> {
    /// The exact configured command that is launched once.
    pub command: &'run mut Command,
    /// The original semantic deadline and progress observation.
    pub bound: Option<&'run mut Bound<'bound>>,
    /// The original retained cancellation and failure events.
    pub stops: &'run Stops,
    /// The actual original recipient and output endpoints consumed at group completion.
    pub custody: Custody,
}

/// The actual recipient that retains a naturally completed producer scope.
#[derive(Debug)]
pub enum Custody {
    /// Every member settles before this command returns.
    Direct,
    /// The original native launching session owns the complete nested scope.
    #[cfg(unix)]
    Original {
        /// The actual acknowledged original launching session.
        parent: njutest_process::ParentSession,
        /// The original standard-output descriptor retained before its reader starts.
        stdout: njutest_process::OutputEndpoint,
        /// The original standard-error descriptor retained before its reader starts.
        stderr: njutest_process::OutputEndpoint,
    },
}

/// Runs with an explicitly acknowledged original native recipient for natural completion.
///
/// # Errors
/// Every original observation, recipient, cancellation and cleanup refusal remains reported.
pub fn run_with_custody<F>(request: Request<'_, '_>, started: F) -> Result<Ended, WorkError>
where
    F: FnOnce(u32) -> std::io::Result<()>,
{
    let Request {
        command,
        bound,
        stops,
        custody,
    } = request;
    std::thread::scope(|scope| {
        let observation = crate::observation::Observation::subscribe();
        let subscription = stops.subscribe(&observation);
        let mut group = Group::launch(command)?;
        match &custody {
            Custody::Direct => {}
            #[cfg(unix)]
            Custody::Original { .. } => {
                command
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
            }
        }
        let completion = Arc::new(Completion::default());
        let waiter = ProcessWaiter::launch(
            scope,
            group.child()?,
            Arc::clone(&completion),
            observation.signal(),
        );
        let cleanup = GroupCleanup { group: &mut group };
        let watched = started(cleanup.group.leader()?)
            .map_err(|source| WorkError::Watch { source })
            .and_then(|()| decided(&completion, &observation, bound, stops));
        let stopped = cleanup.group.close(&watched, stops, custody);
        let joined = waiter.join();
        let reaped = cleanup.group.reap();
        drop(subscription);
        let mut failures = Vec::new();
        if let Err(source) = &watched {
            failures.push((source.kind(), format!("observation: {source}")));
        }
        if let Err(source) = &stopped {
            failures.push((source.kind(), format!("settlement: {source}")));
        }
        if let Err(source) = &joined {
            failures.push((source.kind(), format!("observer join: {source}")));
        }
        if let Err(source) = &reaped {
            failures.push((source.kind(), format!("reap: {source}")));
        }
        if let Some((kind, _first)) = failures.first() {
            return Err(WorkError::Watch {
                source: std::io::Error::new(
                    *kind,
                    failures
                        .iter()
                        .map(|(_kind, cause)| cause.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                ),
            });
        }
        let ended = watched?;
        let status = reaped?;
        Ok(match ended {
            Some(ended) => ended,
            None => Ended::Exited(status),
        })
    })
}

fn decided(
    completion: &Completion,
    observation: &crate::observation::Observation,
    mut bound: Option<&mut Bound<'_>>,
    stops: &Stops,
) -> Result<Option<Ended>, WorkError> {
    let began = Instant::now();
    let mut last_heard = began;
    loop {
        stops.failed()?;
        if let Some(arrived) = stops.heard() {
            last_heard = arrived;
        }
        if let Some(bound) = bound.as_deref_mut() {
            (bound.heard)().map_err(|source| WorkError::Watch { source })?;
        }
        stops.failed()?;
        if completion.done()? {
            return Ok(None);
        }
        if let Some(signal) = stops.raised() {
            return Ok(Some(Ended::Interrupted { signal }));
        }
        let deadline =
            match bound.as_deref() {
                Some(bound) => {
                    if last_heard.elapsed() >= bound.quiet {
                        return Ok(Some(Ended::Quiet {
                            silent: last_heard.elapsed(),
                        }));
                    }
                    if began.elapsed() >= bound.ceiling {
                        return Ok(Some(Ended::OverBudget {
                            elapsed: began.elapsed(),
                        }));
                    }
                    Some(
                        last_heard
                            .checked_add(bound.quiet)
                            .ok_or_else(|| WorkError::Watch {
                                source: std::io::Error::other(
                                    "the quiet deadline is not representable",
                                ),
                            })?
                            .min(began.checked_add(bound.ceiling).ok_or_else(|| {
                                WorkError::Watch {
                                    source: std::io::Error::other(
                                        "the work deadline is not representable",
                                    ),
                                }
                            })?),
                    )
                }
                None => None,
            };
        let waited = observation
            .wait(
                "xtask-work",
                "process-output-signal-or-semantic-deadline",
                deadline,
            )
            .map_err(|source| WorkError::Watch { source })?;
        stops.record(waited.note);
        stops.failed()?;
        match waited.event.map_err(|source| WorkError::Watch { source })? {
            crate::observation::Event::Changed
            | crate::observation::Event::Completed
            | crate::observation::Event::Cancelled
            | crate::observation::Event::Deadline => {}
        }
    }
}

#[derive(Debug, Default)]
struct Completion {
    ended: Mutex<Option<std::io::Result<()>>>,
}

impl Completion {
    fn publish(&self, result: std::io::Result<()>) {
        *self.locked() = Some(result);
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Option<std::io::Result<()>>> {
        match self.ended.lock() {
            Ok(ended) => ended,
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
    }

    fn done(&self) -> Result<bool, WorkError> {
        match self.locked().as_ref() {
            Some(Ok(())) => Ok(true),
            None => Ok(false),
            Some(Err(source)) => Err(WorkError::Watch {
                source: std::io::Error::new(source.kind(), source.to_string()),
            }),
        }
    }
}

#[derive(Debug)]
struct ProcessWaiter<'scope> {
    handle: Option<std::thread::ScopedJoinHandle<'scope, ()>>,
}

impl<'scope> ProcessWaiter<'scope> {
    fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        child: Arc<njutest_process::ChildEvent>,
        completion: Arc<Completion>,
        signal: crate::observation::Signal,
    ) -> Self {
        let handle = scope.spawn(move || {
            let completed = ProcessCompletion { completion, signal };
            completed.publish(observe_completion(&child));
        });
        Self {
            handle: Some(handle),
        }
    }

    fn join(mut self) -> Result<(), WorkError> {
        let handle = self.handle.take().ok_or_else(|| WorkError::Watch {
            source: std::io::Error::other("the process observer was already joined"),
        })?;
        handle.join().map_err(|panic| {
            drop(panic);
            WorkError::Watch {
                source: std::io::Error::other("the process completion observer panicked"),
            }
        })
    }
}

impl Drop for ProcessWaiter<'_> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            eprintln!("the owned work completion observer panicked while joining");
            std::process::abort();
        }
    }
}

struct GroupCleanup<'a> {
    group: &'a mut Group,
}

impl Drop for GroupCleanup<'_> {
    fn drop(&mut self) {
        if let Some(child) = self.group.child.as_mut()
            && let Err(source) = child.stop()
        {
            eprintln!("the work producer settled with a retained cleanup refusal: {source}");
        }
    }
}

struct ProcessCompletion {
    completion: Arc<Completion>,
    signal: crate::observation::Signal,
}

impl ProcessCompletion {
    fn publish(self, result: std::io::Result<()>) {
        self.completion.publish(result);
    }
}

impl Drop for ProcessCompletion {
    fn drop(&mut self) {
        if self.completion.locked().is_none() {
            self.completion.publish(Err(std::io::Error::other(
                "the process observer ended without publishing completion",
            )));
        }
        self.signal.publish(crate::observation::Event::Completed);
    }
}

fn observe_completion(child: &njutest_process::ChildEvent) -> std::io::Result<()> {
    if child.wait(None)? {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "a blocking process event returned no completion",
        ))
    }
}

/// A producer group whose leader remains retained through signalling, joining and final reaping.
#[derive(Debug)]
struct Group {
    child: Option<njutest_process::GroupChild>,
}

impl Group {
    fn launch(command: &mut Command) -> Result<Self, WorkError> {
        let program = command.get_program().display().to_string();
        njutest_process::GroupChild::start(command)
            .map(|child| Self { child: Some(child) })
            .map_err(|source| WorkError::Start { program, source })
    }

    fn child(&self) -> Result<Arc<njutest_process::ChildEvent>, WorkError> {
        self.child
            .as_ref()
            .map(njutest_process::GroupChild::completion)
            .ok_or_else(|| WorkError::Watch {
                source: std::io::Error::other("the process leader was already reaped"),
            })
    }

    fn leader(&self) -> Result<u32, WorkError> {
        self.child
            .as_ref()
            .and_then(njutest_process::GroupChild::id)
            .ok_or_else(|| WorkError::Watch {
                source: std::io::Error::other("the owned process has no live identity"),
            })
    }

    fn close(
        &mut self,
        watched: &Result<Option<Ended>, WorkError>,
        stops: &Stops,
        custody: Custody,
    ) -> Result<(), WorkError> {
        match custody {
            Custody::Direct => self.stop(stops),
            #[cfg(unix)]
            Custody::Original {
                parent,
                stdout,
                stderr,
            } => {
                let outcome = if matches!(watched, Ok(None)) && stops.raised().is_none() {
                    stops.failed()?;
                    let began = Instant::now();
                    let deadline = began
                        .checked_add(njutest_process::REAPING_GRACE)
                        .ok_or_else(|| WorkError::Watch {
                            source: std::io::Error::other("the natural output backstop overflowed"),
                        })?;
                    let mut attempts = 0_u64;
                    let closed = stdout.closed(deadline, &mut attempts).and_then(|first| {
                        if first {
                            stderr.closed(deadline, &mut attempts)
                        } else {
                            Ok(false)
                        }
                    });
                    self.note(
                        stops,
                        began,
                        &format!("natural-independent-output-eof; observation-calls={attempts}"),
                    )?;
                    stops.failed()?;
                    if closed.map_err(|source| WorkError::Watch { source })?
                        && stops.raised().is_none()
                    {
                        self.release(&parent)
                    } else {
                        self.stop(stops)
                    }
                } else {
                    self.stop(stops)
                };
                drop(parent);
                drop(stdout);
                drop(stderr);
                outcome
            }
        }
    }

    #[cfg(unix)]
    fn release(&mut self, parent: &njutest_process::ParentSession) -> Result<(), WorkError> {
        self.child
            .as_mut()
            .ok_or_else(|| WorkError::Watch {
                source: std::io::Error::other(
                    "the original producer was consumed before custody transfer",
                ),
            })?
            .release_to_parent(parent)
            .map(|_status| ())
            .map_err(|source| WorkError::Watch { source })
    }

    fn stop(&mut self, stops: &Stops) -> Result<(), WorkError> {
        let began = Instant::now();
        let leader = self.leader()?;
        let child = self.child.as_mut().ok_or_else(|| WorkError::Watch {
            source: std::io::Error::other("the process owner was consumed before settlement"),
        })?;
        let settled = child.stop_with_grace(GRACE);
        Self::note_for(stops, began, leader, "owned-group-exit-and-leader-reap")?;
        settled.map_err(|source| WorkError::Watch { source })
    }

    #[cfg(unix)]
    fn note(&self, stops: &Stops, began: Instant, cause: &str) -> Result<(), WorkError> {
        Self::note_for(stops, began, self.leader()?, cause)
    }

    fn note_for(stops: &Stops, began: Instant, leader: u32, cause: &str) -> Result<(), WorkError> {
        stops.record(crate::observation::WaitNote {
            owner: format!("process-group:{leader}"),
            cause: cause.to_owned(),
            elapsed_ns: u64::try_from(began.elapsed().as_nanos()).map_err(|source| {
                WorkError::Watch {
                    source: std::io::Error::other(source),
                }
            })?,
            machine: crate::observation::Machine {
                os: std::env::consts::OS,
                cpus: std::thread::available_parallelism()
                    .map_err(|source| WorkError::Watch { source })?
                    .get(),
            },
        });
        Ok(())
    }

    fn reap(&mut self) -> Result<ExitStatus, WorkError> {
        let child = self.child.as_mut().ok_or_else(|| WorkError::Watch {
            source: std::io::Error::other("the process owner was consumed before reap"),
        })?;
        let status = child
            .wait_status()
            .map_err(|source| WorkError::Watch { source })?;
        self.child = None;
        Ok(status)
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take()
            && child.stop().is_err()
        {
            std::process::abort();
        }
    }
}

/// One process as the machine lists it: its id, the group it belongs to, and whether it has ended and waits only to be reaped.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed {
    /// Its id.
    pub pid: u32,
    /// Its group's id.
    pub group: u32,
    /// Whether it is a zombie.
    pub ended: bool,
}

/// Every process of the machine with its group and whether it has ended, as `ps` lists them, or nothing when they could not be listed.
#[cfg(unix)]
#[must_use]
pub fn listed() -> Option<Vec<Listed>> {
    let output = match Command::new("ps")
        .args(["-A", "-o", "pid=,pgid=,stat="])
        .env("LC_ALL", "C")
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) | Err(_) => return None,
    };
    let Ok(text) = String::from_utf8(output.stdout) else {
        return None;
    };
    let mut processes = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [pid, group, state, ..] = fields.as_slice() else {
            return None;
        };
        let (Ok(pid), Ok(group)) = (pid.parse::<u32>(), group.parse::<u32>()) else {
            return None;
        };
        processes.push(Listed {
            pid,
            group,
            ended: state.starts_with('Z'),
        });
    }
    Some(processes)
}

/// The original two cancellation strengths, in their inherited escalation order.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub(crate) enum Sent {
    /// Gives every retained member its original graceful cancellation request.
    Ask,
    /// Forcefully cancels every retained native member.
    Kill,
}

/// Retained native generations acquired only after validating the complete original lane record.
#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct GroupAuthority {
    leader: u32,
    members: Vec<njutest_process::ForeignProcess>,
}

#[cfg(unix)]
impl GroupAuthority {
    pub(crate) fn capture(
        recorded: &crate::lanes::Recorded,
        start_of: impl Fn(u32) -> crate::lanes::Start,
        session_of: impl Fn(u32) -> crate::lanes::Session,
    ) -> Result<Option<Self>, WorkError> {
        let census = listed().ok_or_else(|| WorkError::Watch {
            source: std::io::Error::other("the recorded group could not be listed"),
        })?;
        match crate::lanes::group_liveness(
            recorded,
            &start_of(recorded.pid),
            Some(&census),
            &session_of,
        ) {
            crate::lanes::Liveness::Gone => return Ok(None),
            crate::lanes::Liveness::Unseen => {
                return Err(WorkError::Watch {
                    source: std::io::Error::other(
                        "the original recorded group could not be identified",
                    ),
                });
            }
            crate::lanes::Liveness::Alive => {}
        }
        let mut members = Vec::new();
        for listed in census
            .into_iter()
            .filter(|one| one.group == recorded.pid && !one.ended)
        {
            if let Some(member) = retain_group_member(listed.pid, recorded.pid)? {
                members.push(member);
            }
        }
        let confirmed = listed().ok_or_else(|| WorkError::Watch {
            source: std::io::Error::other("the retained original group could not be confirmed"),
        })?;
        match crate::lanes::group_liveness(
            recorded,
            &start_of(recorded.pid),
            Some(&confirmed),
            &session_of,
        ) {
            crate::lanes::Liveness::Gone => Ok(None),
            crate::lanes::Liveness::Unseen => Err(WorkError::Watch {
                source: std::io::Error::other(
                    "the retained original group identity became unknown",
                ),
            }),
            crate::lanes::Liveness::Alive => Ok(Some(Self {
                leader: recorded.pid,
                members,
            })),
        }
    }
}

/// Delivers cancellation only through retained native generations, never a numeric group facade.
#[cfg(unix)]
pub(crate) fn signal_group(group: &GroupAuthority, sent: Sent) -> Result<(), WorkError> {
    use njutest_process::{Delivered, Others, StopDecision, Stopped, decide_stop};

    let how = match sent {
        Sent::Ask => njutest_process::GroupStop::Ask,
        Sent::Kill => njutest_process::GroupStop::Kill,
    };
    let mut delivered = Delivered::Sent;
    let mut leader = Delivered::Gone;
    let mut others = Others::Nobody;
    let mut failures = Vec::new();
    for member in &group.members {
        let answer = member.signal(how);
        let actual = match &answer {
            Ok(()) => Delivered::Sent,
            Err(source) if source.kind() == std::io::ErrorKind::PermissionDenied => {
                Delivered::Refused
            }
            Err(_source) => Delivered::Failed,
        };
        if member.identity().pid() == group.leader {
            leader = actual;
        } else {
            others = Others::Somebody;
        }
        match actual {
            Delivered::Refused if delivered != Delivered::Failed => delivered = Delivered::Refused,
            Delivered::Sent | Delivered::Gone | Delivered::Refused => {}
            Delivered::Failed => delivered = Delivered::Failed,
        }
        if let Err(source) = answer {
            failures.push(format!("{}: {source}", member.identity().token()));
        }
    }
    match decide_stop(delivered, leader, others) {
        StopDecision::Reached(Stopped::Group) => Ok(()),
        StopDecision::Reached(Stopped::LeaderOnly) => Err(WorkError::Outlived {
            source: std::io::Error::other(failures.join("; ")),
        }),
        StopDecision::Failed => Err(WorkError::Watch {
            source: std::io::Error::other(failures.join("; ")),
        }),
    }
}

#[cfg(unix)]
fn retain_group_member(
    pid: u32,
    expected: u32,
) -> Result<Option<njutest_process::ForeignProcess>, WorkError> {
    let Some(member) = njutest_process::ForeignProcess::retain(pid)
        .map_err(|source| WorkError::Watch { source })?
    else {
        return Ok(None);
    };
    let raw = i32::try_from(member.identity().pid()).map_err(|source| WorkError::Watch {
        source: std::io::Error::other(source),
    })?;
    let pid = rustix::process::Pid::from_raw(raw).ok_or_else(|| WorkError::Watch {
        source: std::io::Error::other("the retained member has no positive native PID"),
    })?;
    match rustix::process::getpgid(Some(pid)) {
        Ok(group) => {
            let actual =
                u32::try_from(group.as_raw_nonzero().get()).map_err(|source| WorkError::Watch {
                    source: std::io::Error::other(source),
                })?;
            Ok((actual == expected).then_some(member))
        }
        Err(rustix::io::Errno::SRCH) => Ok(None),
        Err(source) => Err(WorkError::Watch {
            source: source.into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::Stops;
    use crate::observation::{Event, Observation};

    #[test]
    fn progress_wakes_coalesce_until_the_original_subscription_reads() {
        let stops = Stops::arm().expect("the owned original stop observer");
        let observed = Observation::subscribe();
        let subscription = stops.subscribe(&observed);
        for _ in 0..256 {
            stops.state.publish(Event::Changed);
        }
        assert_eq!(
            observed
                .pending()
                .expect("the complete original publication"),
            Some(Event::Changed)
        );
        assert_eq!(
            observed
                .pending()
                .expect("the complete original publication"),
            None
        );
        drop(subscription);
    }

    #[test]
    fn progress_wakes_preserve_the_original_cancellation_event() {
        let stops = Stops::arm().expect("the owned original stop observer");
        let observed = Observation::subscribe();
        let subscription = stops.subscribe(&observed);
        for _ in 0..256 {
            stops.state.publish(Event::Changed);
        }
        stops.state.publish(Event::Cancelled);
        assert_eq!(
            observed
                .pending()
                .expect("the complete original publication"),
            Some(Event::Cancelled)
        );
        assert_eq!(
            observed
                .pending()
                .expect("the complete original publication"),
            Some(Event::Changed)
        );
        assert_eq!(
            observed
                .pending()
                .expect("the complete original publication"),
            None
        );
        drop(subscription);
    }

    #[test]
    fn progress_wakes_preserve_the_first_actual_producer_refusal() {
        let stops = Stops::arm().expect("the owned original stop observer");
        let observed = Observation::subscribe();
        let subscription = stops.subscribe(&observed);
        let failure = std::sync::Arc::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the original producer refused its progress",
        ));
        stops.state.publish_failure(&failure);
        for _ in 0..256 {
            stops.state.publish(Event::Changed);
        }
        let retained = observed
            .ensure_complete()
            .expect_err("the original refusal");
        assert_eq!(retained.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(retained.to_string(), failure.to_string());
        drop(subscription);
    }

    #[test]
    fn actual_output_publishers_rearm_progress_after_the_reader_acknowledges() {
        let stops = Stops::arm().expect("the owned original stop observer");
        let observed = Observation::subscribe();
        let subscription = stops.subscribe(&observed);
        std::thread::scope(|scope| {
            let publisher = stops.events();
            njutest_devkit::thread::ScopedThread::launch(scope, move || {
                for _arrival in 0..256 {
                    publisher.heard();
                }
            })
            .join()
            .expect("the actual output publisher joins");
        });
        assert!(
            stops.heard().is_some(),
            "the real output arrival time remains available"
        );
        assert_eq!(
            observed.pending().expect("coalesced output burst"),
            Some(Event::Changed)
        );
        assert_eq!(
            observed.pending().expect("one output acknowledgement"),
            None
        );
        stops.events().heard();
        assert!(
            stops.heard().is_some(),
            "the new actual arrival time is retained"
        );
        assert_eq!(
            observed.pending().expect("a new output arrival"),
            Some(Event::Changed)
        );
        assert_eq!(observed.pending().expect("no cached output wake"), None);
        drop(subscription);
    }
}
