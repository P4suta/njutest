// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Work in a process group of its own, stopped whole when a budget or a signal says so, and reaped on every path.

use std::process::{Child, Command, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;

/// How often running work is looked at.
const POLL: Duration = Duration::from_millis(200);

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
}

impl crate::error::Coded for WorkError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Start { .. } | Self::Watch { .. } | Self::Signals { .. } => {
                crate::error::XtCode::WorkUnrun
            }
        }
    }
}

/// The signals that make this process stop its work before it ends: `SIGINT`, `SIGTERM` and `SIGHUP`.
#[derive(Debug)]
pub struct Stops {
    raised: Arc<AtomicUsize>,
    #[expect(
        dead_code,
        reason = "the registrations are held for what dropping them does: the handlers are removed"
    )]
    registrations: Registrations,
}

impl Stops {
    /// Arms the signals; each one records itself rather than ending the process, so the work can be stopped first.
    ///
    /// # Errors
    /// Returns [`WorkError::Signals`] when a handler cannot be installed.
    pub fn arm() -> Result<Self, WorkError> {
        let raised = Arc::new(AtomicUsize::new(0));
        let mut registrations = Registrations(Vec::new());
        for signal in STOPPING {
            let recorded = usize::try_from(signal).map_err(|source| WorkError::Signals {
                source: std::io::Error::other(source),
            })?;
            let id = signal_hook::flag::register_usize(signal, Arc::clone(&raised), recorded)
                .map_err(|source| WorkError::Signals { source })?;
            registrations.0.push(id);
        }
        Ok(Self {
            raised,
            registrations,
        })
    }

    /// The signal that asked this process to stop, once one has.
    #[must_use]
    pub fn raised(&self) -> Option<i32> {
        match i32::try_from(self.raised.load(Ordering::SeqCst)) {
            Ok(0) | Err(_) => None,
            Ok(signal) => Some(signal),
        }
    }
}

/// The handler registrations one [`Stops`] owns, removed when it goes.
#[derive(Debug)]
struct Registrations(Vec<signal_hook::SigId>);

impl Drop for Registrations {
    fn drop(&mut self) {
        while let Some(id) = self.0.pop() {
            if !signal_hook::low_level::unregister(id) {
                std::process::abort();
            }
        }
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

/// Runs `command` in a process group of its own until it exits, its `bound` is passed, or one of `stops` is raised.
/// `started` hears the process id of the group's leader as soon as there is one.
///
/// # Errors
/// Returns a [`WorkError`] when the work cannot be started, watched, or stopped, or when `started` cannot record it.
pub fn run<F>(
    command: &mut Command,
    mut bound: Option<&mut Bound<'_>>,
    stops: &Stops,
    started: F,
) -> Result<Ended, WorkError>
where
    F: FnOnce(u32) -> std::io::Result<()>,
{
    let mut group = Group::launch(command)?;
    if let Some(leader) = group.leader() {
        started(leader).map_err(|source| WorkError::Watch { source })?;
    }
    let began = Instant::now();
    let mut last_heard = began;
    loop {
        let exited = group.try_wait()?;
        if let Some(bound) = bound.as_deref_mut()
            && (bound.heard)().map_err(|source| WorkError::Watch { source })?
        {
            last_heard = Instant::now();
        }
        if let Some(status) = exited {
            return Ok(Ended::Exited(status));
        }
        if let Some(signal) = stops.raised() {
            group.stop()?;
            return Ok(Ended::Interrupted { signal });
        }
        if let Some(bound) = bound.as_deref() {
            if last_heard.elapsed() >= bound.quiet {
                group.stop()?;
                return Ok(Ended::Quiet {
                    silent: last_heard.elapsed(),
                });
            }
            if began.elapsed() >= bound.ceiling {
                group.stop()?;
                return Ok(Ended::OverBudget {
                    elapsed: began.elapsed(),
                });
            }
        }
        std::thread::sleep(POLL);
    }
}

/// Work started in a process group of its own; it is stopped whole and reaped on every path.
#[derive(Debug)]
struct Group {
    child: Option<Child>,
}

impl Group {
    fn launch(command: &mut Command) -> Result<Self, WorkError> {
        grouped(command);
        let program = command.get_program().display().to_string();
        command
            .spawn()
            .map(|child| Self { child: Some(child) })
            .map_err(|source| WorkError::Start { program, source })
    }

    fn leader(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, WorkError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        let status = child
            .try_wait()
            .map_err(|source| WorkError::Watch { source })?;
        if status.is_some() {
            self.child = None;
        }
        Ok(status)
    }

    fn stop(&mut self) -> Result<(), WorkError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        signal(child, Sent::Ask)?;
        let asked = Instant::now();
        while asked.elapsed() < GRACE && !exited(child)? {
            std::thread::sleep(POLL);
        }
        signal(child, Sent::Kill)?;
        child.wait().map_err(|source| WorkError::Watch { source })?;
        self.child = None;
        Ok(())
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if self.stop().is_err() {
            std::process::abort();
        }
    }
}

/// How hard work is asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sent {
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

/// Whether the leader has exited, without reaping it, so the group's id stays reserved while the group is signalled.
#[cfg(unix)]
fn exited(child: &Child) -> Result<bool, WorkError> {
    use rustix::process::{WaitId, WaitIdOptions, waitid};

    let Some(leader) = leader_pid(child) else {
        return Ok(true);
    };
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG;
    match waitid(WaitId::Pid(leader), options) {
        Ok(observed) => Ok(observed.is_some()),
        Err(errno) => Err(WorkError::Watch {
            source: std::io::Error::from(errno),
        }),
    }
}

#[cfg(not(unix))]
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "only unix can watch a leader exit without reaping it, so elsewhere the answer is no, in the signature the unix watch needs"
)]
fn exited(_child: &Child) -> Result<bool, WorkError> {
    Ok(false)
}

#[cfg(unix)]
fn leader_pid(child: &Child) -> Option<rustix::process::Pid> {
    match i32::try_from(child.id()) {
        Ok(raw) => rustix::process::Pid::from_raw(raw),
        Err(_beyond_a_pid) => None,
    }
}

/// Signals the leader's whole group; a group the kernel will not let this process signal whole gets its leader signalled by name, and a group already gone is success.
#[cfg(unix)]
fn signal(child: &mut Child, sent: Sent) -> Result<(), WorkError> {
    use rustix::io::Errno;
    use rustix::process::{Signal, kill_process, kill_process_group};

    let Some(leader) = leader_pid(child) else {
        return match sent {
            Sent::Ask => Ok(()),
            Sent::Kill => child.kill().map_err(|source| WorkError::Watch { source }),
        };
    };
    let signal = match sent {
        Sent::Ask => Signal::TERM,
        Sent::Kill => Signal::KILL,
    };
    match kill_process_group(leader, signal) {
        Ok(()) | Err(Errno::SRCH) => Ok(()),
        Err(Errno::PERM) => match kill_process(leader, signal) {
            Ok(()) | Err(Errno::SRCH) => Ok(()),
            Err(errno) => Err(WorkError::Watch {
                source: std::io::Error::from(errno),
            }),
        },
        Err(errno) => Err(WorkError::Watch {
            source: std::io::Error::from(errno),
        }),
    }
}

#[cfg(not(unix))]
fn signal(child: &mut Child, sent: Sent) -> Result<(), WorkError> {
    match sent {
        Sent::Ask => Ok(()),
        Sent::Kill => child.kill().map_err(|source| WorkError::Watch { source }),
    }
}
