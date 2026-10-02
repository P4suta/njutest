// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Work in a process group of its own, stopped whole when a budget or a signal says so, and reaped on every path.

use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};
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
    /// The kernel refused the work's group whole, and a process besides its leader is still running or could not be seen.
    #[cfg(unix)]
    #[error(
        "the work's process group refused the stop, and a process besides its leader is still \
         running or could not be seen, so the work is not stopped"
    )]
    Outlived,
}

impl crate::error::Coded for WorkError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Start { .. } | Self::Watch { .. } | Self::Signals { .. } => {
                crate::error::XtCode::WorkUnrun
            }
            #[cfg(unix)]
            Self::Outlived => crate::error::XtCode::WorkUnrun,
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
    observers: Mutex<Vec<std::sync::Weak<crate::observation::Signal>>>,
    heard: Mutex<Option<Instant>>,
    waits: Mutex<Vec<crate::observation::WaitNote>>,
}

impl StopState {
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
                observer.publish(event);
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

    fn record(&self, note: crate::observation::WaitNote) {
        match self.state.waits.lock() {
            Ok(mut waits) => waits.push(note),
            Err(poisoned) => {
                drop(poisoned);
                std::process::abort();
            }
        }
    }

    fn subscribe(&self, observation: &crate::observation::Observation) -> StopSubscription {
        let signal = Arc::new(observation.signal());
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
            signal.publish(crate::observation::Event::Cancelled);
        }
        StopSubscription { _signal: signal }
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
struct StopSubscription {
    _signal: Arc<crate::observation::Signal>,
}

/// An owned endpoint through which the output producer publishes its actual arrival time.
#[derive(Debug, Clone)]
pub struct WorkEvents {
    state: Arc<StopState>,
}

impl WorkEvents {
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
    std::thread::scope(|scope| {
        let observation = crate::observation::Observation::subscribe();
        let _subscription = stops.subscribe(&observation);
        let mut group = Group::launch(command)?;
        let completion = Arc::new(Completion::default());
        let waiter = ProcessWaiter::launch(
            scope,
            group.child()?,
            Arc::clone(&completion),
            observation.signal(),
        );
        let watched = started(group.leader()?)
            .map_err(|source| WorkError::Watch { source })
            .and_then(|()| decided(&completion, &observation, bound, stops));
        let stopped = group.stop(&completion, stops);
        let joined = waiter.join();
        let reaped = group.reap();
        let ended = watched?;
        stopped?;
        joined?;
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
        if let Some(arrived) = stops.heard() {
            last_heard = arrived;
        }
        if let Some(bound) = bound.as_deref_mut() {
            (bound.heard)().map_err(|source| WorkError::Watch { source })?;
        }
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
    told: Condvar,
}

impl Completion {
    fn publish(&self, result: std::io::Result<()>) {
        *self.locked() = Some(result);
        self.told.notify_all();
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

    fn by(&self, deadline: Instant) -> Result<(), WorkError> {
        let mut ended = self.locked();
        while ended.is_none() {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(());
            };
            ended = match self.told.wait_timeout(ended, left) {
                Ok((ended, _timeout)) => ended,
                Err(poisoned) => {
                    drop(poisoned);
                    std::process::abort();
                }
            };
        }
        drop(ended);
        self.done().map(|_done| ())
    }
}

#[derive(Debug)]
struct ProcessWaiter<'scope> {
    handle: Option<std::thread::ScopedJoinHandle<'scope, ()>>,
}

impl<'scope> ProcessWaiter<'scope> {
    fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        child: Arc<shared_child::SharedChild>,
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

#[cfg(unix)]
fn observe_completion(child: &shared_child::SharedChild) -> std::io::Result<()> {
    use rustix::process::{WaitId, WaitIdOptions, waitid};
    let raw = i32::try_from(child.id()).map_err(std::io::Error::other)?;
    let pid = rustix::process::Pid::from_raw(raw)
        .ok_or_else(|| std::io::Error::other("the process leader has no positive pid"))?;
    loop {
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        ) {
            Ok(Some(_ended)) => return Ok(()),
            Ok(None) => {
                return Err(std::io::Error::other(
                    "a blocking process observation returned no completion",
                ));
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(source) => return Err(source.into()),
        }
    }
}

#[cfg(not(unix))]
fn observe_completion(child: &shared_child::SharedChild) -> std::io::Result<()> {
    child.wait().map(|_status| ())
}

/// A producer group whose leader remains retained through signalling, joining and final reaping.
#[derive(Debug)]
struct Group {
    child: Option<Arc<shared_child::SharedChild>>,
}

impl Group {
    fn launch(command: &mut Command) -> Result<Self, WorkError> {
        grouped(command);
        let program = command.get_program().display().to_string();
        shared_child::SharedChild::spawn(command)
            .map(|child| Self {
                child: Some(Arc::new(child)),
            })
            .map_err(|source| WorkError::Start { program, source })
    }

    fn child(&self) -> Result<Arc<shared_child::SharedChild>, WorkError> {
        self.child
            .as_ref()
            .map(Arc::clone)
            .ok_or_else(|| WorkError::Watch {
                source: std::io::Error::other("the process leader has already been reaped"),
            })
    }

    fn leader(&self) -> Result<u32, WorkError> {
        Ok(self.child()?.id())
    }

    fn stop(&self, completion: &Completion, stops: &Stops) -> Result<(), WorkError> {
        let child = self.child()?;
        let began = Instant::now();
        let asked = match completion.done() {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => signal(&child, Sent::Ask),
        };
        let deadline = Instant::now()
            .checked_add(GRACE)
            .ok_or_else(|| WorkError::Watch {
                source: std::io::Error::other("the stop grace deadline is not representable"),
            })?;
        let observed = completion.by(deadline);
        let killed = signal(&child, Sent::Kill);
        if let Err(unstopped) = killed {
            let said = std::io::Write::write_all(
                &mut std::io::stderr(),
                format!("xtask: the producer group could not be settled: {unstopped}\n").as_bytes(),
            );
            match said {
                Ok(()) | Err(_) => std::process::abort(),
            }
        }
        stops.record(crate::observation::WaitNote {
            owner: format!("process-group:{}", child.id()),
            cause: "group-termination-or-stop-grace".to_owned(),
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
        asked.and(observed)
    }

    fn reap(&mut self) -> Result<ExitStatus, WorkError> {
        let child = self.child()?;
        let status = child.wait().map_err(|source| WorkError::Watch { source })?;
        self.child = None;
        Ok(status)
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if let Some(child) = self.child.take()
            && let Err(unstopped) = signal(&child, Sent::Kill).and_then(|()| {
                child
                    .wait()
                    .map(|_status| ())
                    .map_err(|source| WorkError::Watch { source })
            })
        {
            let said = std::io::Write::write_all(
                &mut std::io::stderr(),
                format!("xtask: the producer group could not be settled: {unstopped}\n").as_bytes(),
            );
            match said {
                Ok(()) | Err(_) => std::process::abort(),
            }
        }
    }
}

/// How hard work is asked to stop, in the order the asking escalates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub(crate) enum Sent {
    /// `SIGTERM`, the chance to stop cleanly.
    Ask,
    /// `SIGKILL`, after the grace a hung member ignores.
    Kill,
}

#[cfg(unix)]
fn grouped(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(not(unix))]
const fn grouped(_command: &mut Command) {}

#[cfg(unix)]
fn leader_pid(child: &shared_child::SharedChild) -> Option<rustix::process::Pid> {
    match i32::try_from(child.id()) {
        Ok(raw) => rustix::process::Pid::from_raw(raw),
        Err(_beyond_a_pid) => None,
    }
}

/// Signals the leader's whole group; a group the kernel will not let this process signal whole gets its leader signalled by name, and a group already gone is success.
#[cfg(unix)]
fn signal(child: &shared_child::SharedChild, sent: Sent) -> Result<(), WorkError> {
    let Some(leader) = leader_pid(child) else {
        return match sent {
            Sent::Ask => Ok(()),
            Sent::Kill => child.kill().map_err(|source| WorkError::Watch { source }),
        };
    };
    signal_group(leader, sent)
}

/// Signals the group `leader` leads, as [`decide_stop`] decides: a group the kernel will not let this process signal whole gets its leader signalled by name, and is stopped only if a look at it finds nobody else.
#[cfg(unix)]
pub(crate) fn signal_group(leader: rustix::process::Pid, sent: Sent) -> Result<(), WorkError> {
    use rustix::io::Errno;
    use rustix::process::{Signal, kill_process, kill_process_group};

    let signal = match sent {
        Sent::Ask => Signal::TERM,
        Sent::Kill => Signal::KILL,
    };
    let grouped = kill_process_group(leader, signal);
    let (alone, others) = match grouped {
        Err(Errno::PERM) => (
            kill_process(leader, signal),
            others_than(leader.as_raw_nonzero().get()),
        ),
        Ok(()) | Err(_) => (Ok(()), Others::Unseen),
    };
    match decide_stop(delivered(grouped), delivered(alone), others) {
        StopDecision::Reached(Stopped::Group) => Ok(()),
        StopDecision::Reached(Stopped::LeaderOnly) => Err(WorkError::Outlived),
        StopDecision::Failed => Err(WorkError::Watch {
            source: match (grouped, alone) {
                (Err(Errno::PERM), Err(errno)) | (Err(errno), _) => std::io::Error::from(errno),
                (Ok(()), Ok(()) | Err(_)) => {
                    std::io::Error::other("a stop the kernel answered failed")
                }
            },
        }),
    }
}

/// What the kernel's answer to one signal comes to.
#[cfg(unix)]
const fn delivered(answer: rustix::io::Result<()>) -> Delivered {
    match answer {
        Ok(()) => Delivered::Sent,
        Err(rustix::io::Errno::SRCH) => Delivered::Gone,
        Err(rustix::io::Errno::PERM) => Delivered::Refused,
        Err(_) => Delivered::Failed,
    }
}

/// Who besides `leader` its group holds, as the machine's processes are listed.
#[cfg(unix)]
fn others_than(leader: i32) -> Others {
    let Ok(leader) = u32::try_from(leader) else {
        return Others::Unseen;
    };
    match listed() {
        None => Others::Unseen,
        Some(processes)
            if processes
                .iter()
                .any(|one| one.pid != leader && one.group == leader && !one.ended) =>
        {
            Others::Somebody
        }
        Some(_) => Others::Nobody,
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

#[cfg(unix)]
pub use rust_mutants_decision::group::{Delivered, Others, StopDecision, Stopped, decide_stop};

#[cfg(not(unix))]
fn signal(child: &shared_child::SharedChild, sent: Sent) -> Result<(), WorkError> {
    match sent {
        Sent::Ask => Ok(()),
        Sent::Kill => child.kill().map_err(|source| WorkError::Watch { source }),
    }
}
