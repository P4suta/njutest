// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! POSIX supervision: a process group per child.

use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::process::{Child, Command, ExitStatus};

use rustix::process::{Pid, Signal, kill_process_group};

use super::{EXIT_CODE_UNAVAILABLE, RunnerError};

/// The mechanism this platform supervises with.
pub(super) const SUPERVISOR_KIND: &str = "process-group";

/// Owns the process group of one child.
#[derive(Debug, Default)]
pub(super) struct Supervisor {
    /// The group id, which is the child's pid: a new group has the child as its leader.
    pgid: Option<Pid>,
}

#[expect(
    clippy::unnecessary_wraps,
    clippy::unused_self,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_ref_mut,
    reason = "the same signatures as the Windows supervisor, which can fail and holds a handle"
)]
impl Supervisor {
    /// Nothing to allocate up front: the group is created by the kernel as part of starting the child.
    pub(super) fn new() -> Result<Self, RunnerError> {
        Ok(Self::default())
    }

    /// Asks the kernel to put the child in a new process group of its own. Any descendant it later creates inherits that group, which is what makes a single kill reach the tree.
    pub(super) fn configure(&self, command: &mut Command) {
        command.process_group(0);
    }

    /// Records the group id. Nothing can fail: had the group not been set up the child would not have started at all.
    pub(super) fn adopt(&mut self, child: &Child) -> Result<(), RunnerError> {
        self.pgid = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
        Ok(())
    }

    /// SIGTERM to the whole group: the chance to run deferred cleanup and flush the output that is the evidence for why the mutant timed out.
    pub(super) fn terminate_gently(&self) {
        if let Some(pgid) = self.pgid {
            let _sent = kill_process_group(pgid, Signal::TERM);
        }
    }

    /// SIGKILL to the whole group, after the grace period a hung test ignores. The kill goes to the group while the child is still un-reaped, so the pid the group is named after cannot yet have been recycled.
    pub(super) fn terminate_forcefully(&self) {
        if let Some(pgid) = self.pgid {
            let _sent = kill_process_group(pgid, Signal::KILL);
        }
    }

    /// Nothing to free: a process group is a number, not a handle.
    pub(super) fn release(&mut self) {}
}

/// The child's status, mapping a signal death to the shell's 128 + N convention: 137 for a SIGKILL is both distinguishable from "no status at all" and what every other tool on the machine prints.
pub(super) fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128i32.saturating_add(signal)))
        .unwrap_or(EXIT_CODE_UNAVAILABLE)
}

/// The signal the child died from, which is what a report says when a mutation turned a failure into an abort.
pub(super) fn signal(status: ExitStatus) -> Option<i32> {
    status.signal()
}
