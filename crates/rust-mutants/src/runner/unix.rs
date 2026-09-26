// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! POSIX supervision: a process group per child.

use std::io;
#[cfg(target_os = "macos")]
use std::mem::size_of_val;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::process::{Child, Command, ExitStatus};

use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process, kill_process_group, waitid,
};

use super::{LeaderObservation, ProcessExit, RunnerError, SupervisionBoundary};

/// The mechanism this platform supervises with.
pub(super) const SUPERVISOR_KIND: &str = "process-group";

/// POSIX process groups are inherited, not inescapable containers.
pub(super) const SUPERVISION_BOUNDARY: SupervisionBoundary =
    SupervisionBoundary::InheritedProcessGroup;

/// Whether a read of no bytes was the end of the stream rather than a pipe with nothing in it yet.
///
/// An `O_NONBLOCK` pipe answers `EWOULDBLOCK` while it is merely empty, so no bytes is already every writer having closed and there is nothing further to ask.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the Windows reader, which asks the kernel and can fail"
)]
pub(super) const fn stream_ended(_reader: &io::PipeReader) -> io::Result<bool> {
    Ok(true)
}

/// Makes a pipe read cancellable by letting its owned reader poll for data and its stop instruction instead of blocking forever in the kernel.
pub(super) fn configure_reader(reader: &io::PipeReader) -> io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(reader)?;
    rustix::fs::fcntl_setfl(reader, flags | rustix::fs::OFlags::NONBLOCK).map_err(io::Error::from)
}

/// Owns the process group of one child.
#[derive(Debug)]
pub(super) struct Supervisor {
    /// The group id, which is the child's pid: a new group has the child as its leader.
    pgid: Option<Pid>,
}

/// What a process group would say about the processes it held, which on this platform is nothing: a child names its parent instead.
#[derive(Debug, Clone, Copy)]
pub enum Membership {}

impl Membership {
    /// Every process named since the last time it was asked; there is never one to ask.
    #[must_use]
    pub const fn drain(self) -> Vec<u32> {
        match self {}
    }
}

#[expect(
    clippy::unnecessary_wraps,
    clippy::unused_self,
    reason = "the same signatures as the Windows supervisor, which can fail and holds a handle"
)]
impl Supervisor {
    /// Nothing to allocate up front: the group is created by the kernel as part of starting the child.
    pub(super) const fn new() -> Result<Self, RunnerError> {
        Ok(Self { pgid: None })
    }

    /// Asks the kernel to put the child in a new process group of its own.
    /// Descendants inherit that group unless they deliberately leave it.
    pub(super) fn configure(&self, command: &mut Command) {
        command.process_group(0);
    }

    /// Records the group id.
    /// Nothing can fail: had the group not been set up the child would not have started at all.
    pub(super) fn adopt(&mut self, child: &Child) -> Result<(), RunnerError> {
        let raw =
            i32::try_from(child.id()).map_err(|error| RunnerError::SupervisionUnavailable {
                message: format!(
                    "child process id {} is outside the platform pid range: {error}",
                    child.id()
                ),
                source: None,
            })?;
        let Some(pgid) = Pid::from_raw(raw) else {
            return Err(RunnerError::SupervisionUnavailable {
                message: format!(
                    "child process id {} is not a valid process-group id",
                    child.id()
                ),
                source: None,
            });
        };
        self.pgid = Some(pgid);
        Ok(())
    }

    /// SIGTERM to the whole group: the chance to run deferred cleanup and flush the output that is the evidence for why the mutant timed out.
    pub(super) fn terminate_gently(&self) -> io::Result<()> {
        self.signal(Signal::TERM)
    }

    /// SIGKILL to the whole group, after the grace period a hung test ignores.
    /// The kill goes to the group while the child is still un-reaped, so the pid the group is named after cannot yet have been recycled.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            unused_variables,
            reason = "only macOS can see a leader that has exited and not yet been reaped, and only there does the distinction change what is signalled"
        )
    )]
    pub(super) fn terminate_forcefully(&self, leader: LeaderObservation) -> io::Result<()> {
        #[cfg(target_os = "macos")]
        if leader == LeaderObservation::ExitedWaitable && !self.has_member_besides_leader()? {
            return Ok(());
        }
        self.signal(Signal::KILL)
    }

    #[cfg(target_os = "macos")]
    fn has_member_besides_leader(&self) -> io::Result<bool> {
        let pgid = self.pgid.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the supervisor has no adopted process group",
            )
        })?;
        member_besides_leader(pgid)
    }

    /// What the supervisor can say about the group it owns, for the note before an abort.
    /// Where this group names the processes it holds: nowhere, since a child here names its parent.
    #[expect(
        clippy::unused_self,
        reason = "the same signature as the Windows supervisor, whose job names its processes"
    )]
    pub(super) const fn membership(&self) -> Option<std::sync::Arc<Membership>> {
        None
    }

    pub(super) fn state(&self) -> String {
        let pgid = match self.pgid {
            Some(pgid) => pgid.as_raw_nonzero().get(),
            None => return "holding no process group".to_owned(),
        };
        #[cfg(target_os = "macos")]
        {
            let members = match self.has_member_besides_leader() {
                Ok(true) => "a member besides its leader".to_owned(),
                Ok(false) => "no member besides its leader".to_owned(),
                Err(why) => format!("members it could not list ({why})"),
            };
            format!("group {pgid} with {members}")
        }
        #[cfg(not(target_os = "macos"))]
        format!("group {pgid}")
    }

    /// Signals the whole group, or, where the kernel refuses the group, the one process this supervisor started.
    ///
    /// A group signal is refused whole when any member is beyond this process's authority, as an Apple-signed binary a toolchain reached is on a runner, and on macOS when every member has ended and none is reaped yet, which is where a test process that printed its failure and exited is when the stop for that failure arrives.
    /// What is answerable in both is the child started here, the group's leader, so it is signalled by name, and a leader that has already gone is still success.
    fn signal(&self, signal: Signal) -> io::Result<()> {
        let pgid = self.pgid.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the supervisor has no adopted process group",
            )
        })?;
        match signal_group(pgid, signal)? {
            super::Stopped::Group | super::Stopped::LeaderOnly => Ok(()),
        }
    }

    /// Forgets the group id only after the caller has forcefully signalled the group while its leader remained waitable, then reaped that leader.
    pub(super) const fn release(&mut self) -> io::Result<()> {
        self.pgid = None;
        Ok(())
    }
}

#[cfg(any(target_os = "macos", test))]
fn snapshot_has_member_besides_leader(
    leader: i32,
    returned: usize,
    members: &[i32],
) -> io::Result<bool> {
    if returned == 0 {
        return Err(io::Error::other(
            "proc_listpgrppids omitted its known waitable leader",
        ));
    }
    if returned > members.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "proc_listpgrppids returned more PIDs than its buffer holds",
        ));
    }
    let mut found_leader = false;
    let mut found_other = false;
    for member in members.iter().take(returned).copied() {
        if member <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "proc_listpgrppids returned a non-process PID",
            ));
        }
        if member == leader {
            found_leader = true;
        } else {
            found_other = true;
        }
    }
    if !found_leader {
        return Err(io::Error::other(
            "proc_listpgrppids omitted its known waitable leader",
        ));
    }
    Ok(found_other)
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "the macOS process-group member query has no safe standard-library binding"
)]
unsafe extern "C" {
    fn proc_listpgrppids(pgrpid: i32, buffer: *mut core::ffi::c_void, buffersize: i32) -> i32;
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if self.pgid.is_some() && self.signal(Signal::KILL).is_err() {
            std::process::abort();
        }
    }
}

/// Signals every process of the group `leader` leads, and where the kernel refuses the group whole, `leader` by name, and says how much of the group that reached, as [`super::decide_stop`] decides.
///
/// This is the one place in the shipped crates that signals a group, because what the kernel answers is the same question wherever it is asked.
fn signal_group(leader: Pid, signal: Signal) -> io::Result<super::Stopped> {
    let grouped = kill_process_group(leader, signal);
    let (alone, others) = match grouped {
        Err(rustix::io::Errno::PERM) => (kill_process(leader, signal), others_than(leader)),
        Ok(()) | Err(_) => (Ok(()), super::Others::Unseen),
    };
    match super::decide_stop(delivered(grouped), delivered(alone), others) {
        super::StopDecision::Reached(stopped) => Ok(stopped),
        super::StopDecision::Failed => Err(match (grouped, alone) {
            (Err(rustix::io::Errno::PERM), Err(errno)) | (Err(errno), _) => {
                io::Error::from_raw_os_error(errno.raw_os_error())
            }
            (Ok(()), Ok(()) | Err(_)) => io::Error::other("a stop the kernel answered failed"),
        }),
    }
}

/// What the kernel's answer to one signal comes to.
const fn delivered(answer: rustix::io::Result<()>) -> super::Delivered {
    match answer {
        Ok(()) => super::Delivered::Sent,
        Err(rustix::io::Errno::SRCH) => super::Delivered::Gone,
        Err(rustix::io::Errno::PERM) => super::Delivered::Refused,
        Err(_) => super::Delivered::Failed,
    }
}

/// Who besides `leader` its group holds, as far as this platform can see.
fn others_than(leader: Pid) -> super::Others {
    #[cfg(target_os = "macos")]
    let seen = member_besides_leader(leader);
    #[cfg(target_os = "linux")]
    let seen = linux_member_besides_leader(leader.as_raw_nonzero().get());
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let seen: io::Result<bool> = {
        let _ = leader;
        Err(io::Error::other("this platform cannot list a group"))
    };
    match seen {
        Ok(true) => super::Others::Somebody,
        Ok(false) => super::Others::Nobody,
        Err(_unlisted) => super::Others::Unseen,
    }
}

/// Whether a process besides `leader` that has not ended belongs to the group `leader` leads, as `/proc` lists them.
#[cfg(target_os = "linux")]
fn linux_member_besides_leader(leader: i32) -> io::Result<bool> {
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let pid = match entry.file_name().to_str().map(str::parse::<i32>) {
            Some(Ok(pid)) => pid,
            Some(Err(_)) | None => continue,
        };
        if pid == leader {
            continue;
        }
        let stat = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(gone) if gone.kind() == io::ErrorKind::NotFound => continue,
            Err(other) => return Err(other),
        };
        let Some((_, after_name)) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = after_name.split_whitespace();
        let state = fields.next();
        let group = match fields.nth(1).map(str::parse::<i32>) {
            Some(Ok(group)) => group,
            Some(Err(_)) | None => continue,
        };
        if group == leader && state != Some("Z") {
            return Ok(true);
        }
    }
    Ok(false)
}
/// Whether the group `pgid` leads holds a process besides its leader, as the kernel lists it.
#[cfg(target_os = "macos")]
fn member_besides_leader(pgid: Pid) -> io::Result<bool> {
    let leader = pgid.as_raw_nonzero().get();
    #[expect(
        unsafe_code,
        reason = "a null proc_listpgrppids query is the macOS boundary for sizing its PID buffer"
    )]
    let estimate = unsafe { proc_listpgrppids(leader, std::ptr::null_mut(), 0) };
    if estimate < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut capacity = usize::try_from(estimate)
        .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
    if capacity == 0 {
        return Err(io::Error::other(
            "proc_listpgrppids returned no capacity for its known waitable leader",
        ));
    }
    loop {
        let mut members = Vec::new();
        members
            .try_reserve_exact(capacity)
            .map_err(io::Error::other)?;
        members.resize(capacity, 0_i32);
        let buffer_bytes = size_of_val(members.as_slice());
        let buffer_size = i32::try_from(buffer_bytes)
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
        #[expect(
            unsafe_code,
            reason = "proc_listpgrppids fills the owned macOS process-group PID buffer"
        )]
        let returned =
            unsafe { proc_listpgrppids(leader, members.as_mut_ptr().cast(), buffer_size) };
        if returned < 0 {
            return Err(io::Error::last_os_error());
        }
        let returned = usize::try_from(returned)
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
        if returned > capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "proc_listpgrppids returned more PIDs than its buffer holds",
            ));
        }
        if returned < capacity {
            return snapshot_has_member_besides_leader(leader, returned, &members);
        }
        capacity = capacity.checked_mul(2).ok_or_else(|| {
            io::Error::other("the macOS process-group PID buffer size overflowed")
        })?;
    }
}

/// Ends the one process `pid`, as [`super::stop_process`] describes.
pub(super) fn stop_process(pid: u32) -> io::Result<()> {
    let raw = match i32::try_from(pid) {
        Ok(raw) => raw,
        Err(_out_of_range) => return Ok(()),
    };
    let Some(pid) = Pid::from_raw(raw) else {
        return Ok(());
    };
    match kill_process(pid, Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(errno) => Err(io::Error::from_raw_os_error(errno.raw_os_error())),
    }
}

/// Stops the group a process started in a group of its own leads, as [`super::stop_group`] describes.
pub(super) fn stop_group(leader: u32, how: super::GroupStop) -> io::Result<super::Stopped> {
    let unled = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{leader} is not a process id a group can be led by"),
        )
    };
    let raw = match i32::try_from(leader) {
        Ok(raw) => raw,
        Err(_wider_than_a_pid) => return Err(unled()),
    };
    let pid = Pid::from_raw(raw).ok_or_else(unled)?;
    let signal = match how {
        super::GroupStop::Ask => Signal::TERM,
        super::GroupStop::Kill => Signal::KILL,
    };
    signal_group(pid, signal)
}

/// Observes leader exit without reaping it, so its PID continues to pin the process-group id until the supervisor has forcefully signalled that group.
pub(super) fn exit_observed(child: &Child) -> io::Result<bool> {
    let raw = i32::try_from(child.id())
        .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
    let pid = Pid::from_raw(raw).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the child id is not a valid waitable process id",
        )
    })?;
    waitid(
        WaitId::Pid(pid),
        WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
    )
    .map(|status| status.is_some())
    .map_err(io::Error::from)
}

/// Whether `signal` is one a process raises by what it does (an abort, a bad access, a bad instruction, a trap) rather than one another process sends it.
pub(super) fn raised_by_itself(signal: i32) -> bool {
    [
        Signal::ABORT,
        Signal::SEGV,
        Signal::BUS,
        Signal::ILL,
        Signal::FPE,
        Signal::TRAP,
        Signal::SYS,
    ]
    .iter()
    .any(|raised| raised.as_raw() == signal)
}

/// The child's status, mapping a signal death to the shell's 128 + N convention: 137 for a SIGKILL is both distinguishable from "no status at all" and what every other tool on the machine prints.
pub(super) fn process_exit(status: ExitStatus) -> ProcessExit {
    status.code().map_or_else(
        || {
            status
                .signal()
                .map_or(ProcessExit::Unknown, ProcessExit::Signal)
        },
        ProcessExit::Code,
    )
}

#[cfg(test)]
mod tests {
    use super::{delivered, snapshot_has_member_besides_leader};

    #[test]
    fn an_absent_group_and_a_forbidden_group_are_distinct_answers() {
        assert_eq!(delivered(Ok(())), super::super::Delivered::Sent);
        assert_eq!(
            delivered(Err(rustix::io::Errno::SRCH)),
            super::super::Delivered::Gone
        );
        assert_eq!(
            delivered(Err(rustix::io::Errno::PERM)),
            super::super::Delivered::Refused,
            "a refusal is its own answer, never read as the group being gone"
        );
        assert_eq!(
            delivered(Err(rustix::io::Errno::INVAL)),
            super::super::Delivered::Failed
        );
    }

    #[test]
    fn the_macos_group_query_counts_pids_rather_than_bytes() {
        assert!(matches!(
            snapshot_has_member_besides_leader(41, 2, &[41, 42]),
            Ok(true)
        ));
        assert!(matches!(
            snapshot_has_member_besides_leader(41, 1, &[41, 0]),
            Ok(false)
        ));
        let missing_snapshot = snapshot_has_member_besides_leader(41, 0, &[]);
        assert!(
            matches!(
                missing_snapshot,
                Err(ref error) if error.kind() == std::io::ErrorKind::Other
            ),
            "{missing_snapshot:?}"
        );
        let missing_leader = snapshot_has_member_besides_leader(41, 1, &[42]);
        assert!(
            matches!(
                missing_leader,
                Err(ref error) if error.kind() == std::io::ErrorKind::Other
            ),
            "{missing_leader:?}"
        );
    }
}
