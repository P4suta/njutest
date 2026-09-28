// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A child started at the head of a process group of its own, which is the only thing a group stop can be asked of.

use std::io;
#[cfg(unix)]
use std::marker::PhantomData;
use std::process::{Child, ChildStdin, ChildStdout, Command};
use std::time::{Duration, Instant};

/// A child process started at the head of a process group of its own where the platform has groups, stopped whole and reaped on every path.
#[derive(Debug)]
pub struct GroupChild {
    owned: Owned,
}

/// The child a [`GroupChild`] started, and whether it has been reaped.
#[derive(Debug)]
struct Owned {
    child: Child,
    reaped: bool,
}

/// The leader of a group a [`GroupChild`] started and has not reaped, whose id therefore still names that group and nothing else.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
pub struct Leader<'a> {
    pid: rustix::process::Pid,
    owner: PhantomData<&'a Owned>,
}

#[cfg(unix)]
impl Leader<'_> {
    /// The id the group is named by.
    pub(super) const fn pid(self) -> rustix::process::Pid {
        self.pid
    }
}

/// How often a child given time to end by itself is looked at.
const POLL: Duration = Duration::from_millis(10);

impl GroupChild {
    /// Starts `command` at the head of a process group of its own, where the platform has groups.
    ///
    /// # Errors
    /// The operating system could not start it.
    pub fn start(command: &mut Command) -> io::Result<Self> {
        lead(command);
        Owned::launch(command).map(|owned| Self { owned })
    }

    /// Its standard input, the first time it is asked for.
    pub const fn stdin(&mut self) -> Option<ChildStdin> {
        self.owned.child.stdin.take()
    }

    /// Its standard output, the first time it is asked for.
    pub const fn stdout(&mut self) -> Option<ChildStdout> {
        self.owned.child.stdout.take()
    }

    /// Whether it has ended, reaping it if it has.
    ///
    /// # Errors
    /// The operating system could not say.
    pub fn try_wait(&mut self) -> io::Result<bool> {
        self.owned.try_reap()
    }

    /// Waits for it to end, and reaps it.
    ///
    /// # Errors
    /// The operating system could not wait for it.
    pub fn wait(&mut self) -> io::Result<()> {
        self.owned.reap()
    }

    /// The group it leads, for as long as it has not been reaped, which is for as long as the group's id can name nothing else.
    #[cfg(unix)]
    #[must_use]
    pub fn leader(&self) -> Option<Leader<'_>> {
        self.owned.leader()
    }

    /// Ends every process of its group, then reaps it.
    ///
    /// # Errors
    /// The group could not be stopped whole, or it could not be reaped.
    pub fn stop(&mut self) -> io::Result<()> {
        self.owned.end_and_reap()
    }

    /// Waits up to `timeout` for it to end by itself, and stops it when it has not.
    ///
    /// # Errors
    /// It could not be looked at, stopped, or reaped, or `timeout` reaches past what a clock can hold.
    pub fn finish(&mut self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the deadline for the child to end is outside Instant's range",
            )
        })?;
        loop {
            match self.owned.try_reap() {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(looked) => {
                    return match self.owned.end_and_reap() {
                        Ok(()) => Err(looked),
                        Err(cleanup) => Err(io::Error::new(
                            looked.kind(),
                            format!(
                                "cannot look at the child: {looked}; stopping it failed too: {cleanup}"
                            ),
                        )),
                    };
                }
            }
            if Instant::now() >= deadline {
                return self.owned.end_and_reap();
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Owned {
    fn launch(command: &mut Command) -> io::Result<Self> {
        command.spawn().map(|child| Self {
            child,
            reaped: false,
        })
    }

    fn try_reap(&mut self) -> io::Result<bool> {
        if self.reaped {
            return Ok(true);
        }
        let ended = self.child.try_wait()?.is_some();
        self.reaped = ended;
        Ok(ended)
    }

    fn reap(&mut self) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        self.child.wait().map(|_status| {
            self.reaped = true;
        })
    }

    #[cfg(unix)]
    fn leader(&self) -> Option<Leader<'_>> {
        if self.reaped {
            return None;
        }
        let raw = match i32::try_from(self.child.id()) {
            Ok(raw) => raw,
            Err(_wider_than_a_pid) => return None,
        };
        rustix::process::Pid::from_raw(raw).map(|pid| Leader {
            pid,
            owner: PhantomData,
        })
    }

    fn end_and_reap(&mut self) -> io::Result<()> {
        let ended = self.end();
        let reaped = self.reap();
        match (ended, reaped) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(ending), Ok(())) => Err(ending),
            (Ok(()), Err(reaping)) => Err(reaping),
            (Err(ending), Err(reaping)) => Err(io::Error::new(
                reaping.kind(),
                format!("cannot stop the group: {ending}; cannot reap its leader: {reaping}"),
            )),
        }
    }

    #[cfg(unix)]
    fn end(&self) -> io::Result<()> {
        let Some(leader) = self.leader() else {
            return Ok(());
        };
        match super::stop_group(leader, super::GroupStop::Kill)? {
            super::Stopped::Group => Ok(()),
            super::Stopped::LeaderOnly => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the process group refused the stop, and a process besides its leader is still \
                 running or could not be seen, so the group is not stopped",
            )),
        }
    }

    #[cfg(not(unix))]
    fn end(&mut self) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        self.child.kill()
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        match self.end_and_reap() {
            Ok(()) | Err(_) => {}
        }
    }
}

#[cfg(unix)]
fn lead(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(not(unix))]
const fn lead(_command: &mut Command) {}

#[cfg(all(test, unix))]
mod tests {
    use super::GroupChild;

    #[test]
    fn a_child_that_has_been_reaped_leads_no_group() {
        let mut ended =
            GroupChild::start(&mut std::process::Command::new("true")).expect("true starts");
        assert!(
            ended.leader().is_some(),
            "a child not yet reaped leads the group it was started at the head of"
        );
        ended.wait().expect("true is reaped");
        assert!(
            ended.leader().is_none(),
            "once reaped, its id may be anybody's, so nothing may be stopped by it"
        );
    }

    #[test]
    fn stopping_a_group_whose_leader_has_already_exited_is_no_failure() {
        let mut ended =
            GroupChild::start(&mut std::process::Command::new("true")).expect("true starts");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let stopped = ended.stop();
        assert!(
            stopped.is_ok(),
            "a child that exited before its stop arrived, and is not reaped yet, is stopped: on \
             macOS the group signal is refused with EPERM for such a group, and that is the \
             group being gone rather than a cleanup that failed: {stopped:?}"
        );
    }

    #[test]
    fn a_child_left_running_is_stopped_and_reaped_when_it_is_dropped() {
        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        let started = std::time::Instant::now();
        let running = GroupChild::start(&mut command).expect("the child starts");
        drop(running);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "dropping a child that still runs ends it rather than waiting for it to end by itself"
        );
    }
}
