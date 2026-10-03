// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! POSIX supervision: a process group per child.

use std::io;
#[cfg(target_os = "macos")]
use std::mem::size_of_val;
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command};
use std::time::Instant;

use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process, kill_process_group, waitid,
};

use super::LeaderObservation;

/// Whether a read of no bytes was the end of the stream rather than a pipe with nothing in it yet.
///
/// An `O_NONBLOCK` pipe answers `EWOULDBLOCK` while it is merely empty, so no bytes is already every writer having closed and there is nothing further to ask.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as the Windows reader, which asks the kernel and can fail"
)]
/// Preserves the owned platform observation through this transition.
///
/// # Errors
/// The operating system refused this owned process transition.
pub const fn stream_ended(_reader: &io::PipeReader) -> io::Result<bool> {
    Ok(true)
}

/// Makes a pipe read cancellable through its actual data-readiness and stop descriptors.
///
/// # Errors
/// The kernel refused the descriptor mode transition.
pub fn configure_reader(reader: &io::PipeReader) -> io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(reader)?;
    rustix::fs::fcntl_setfl(reader, flags | rustix::fs::OFlags::NONBLOCK).map_err(io::Error::from)
}

/// Owns the process group of one child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Group,
    Session,
}

#[derive(Debug)]
pub(crate) struct Supervisor {
    /// The group id, which is the child's pid: a new group has the child as its leader.
    pgid: Option<Pid>,
    scope: Scope,
    original_session: Pid,
    retained: std::sync::Mutex<Vec<(Member, MemberExit)>>,
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
    reason = "the same signatures as the Windows supervisor, which can fail and holds a handle"
)]
impl Supervisor {
    /// Nothing to allocate up front: the group is created by the kernel as part of starting the child.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            pgid: None,
            scope: Scope::Group,
            original_session: rustix::process::getsid(None)?,
            retained: std::sync::Mutex::new(Vec::new()),
        })
    }

    pub(super) fn session() -> io::Result<Self> {
        let mut prepared = Self::new()?;
        prepared.scope = Scope::Session;
        Ok(prepared)
    }

    /// Asks the kernel to put the child in a new process group of its own.
    /// Descendants inherit that group unless they deliberately leave it.
    pub(super) fn configure(&self, command: &mut Command) {
        match self.scope {
            Scope::Group => {
                command.process_group(0);
            }
            Scope::Session => {
                command.process_group(rustix::process::getpgrp().as_raw_nonzero().get());
                #[expect(
                    unsafe_code,
                    reason = "the pre-exec closure calls only the async-signal-safe setsid syscall before child code can run"
                )]
                unsafe {
                    command.pre_exec(|| {
                        rustix::process::setsid()
                            .map(|_session| ())
                            .map_err(Into::into)
                    });
                }
            }
        }
    }

    /// Records the group id.
    /// Nothing can fail: had the group not been set up the child would not have started at all.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn adopt(&mut self, child: &Child) -> io::Result<()> {
        let raw = i32::try_from(child.id()).map_err(io::Error::other)?;
        let pgid =
            Pid::from_raw(raw).ok_or_else(|| io::Error::other("invalid owned process-group id"))?;
        self.pgid = Some(pgid);
        if self.scope == Scope::Session {
            self.original_session = pgid;
            match rustix::process::getsid(Some(pgid)) {
                Ok(actual) if actual == pgid => {}
                Ok(actual) => {
                    return Err(io::Error::other(format!(
                        "the original leader acquired session {} instead of {}",
                        actual.as_raw_nonzero(),
                        pgid.as_raw_nonzero()
                    )));
                }
                Err(rustix::io::Errno::SRCH) => {
                    use std::os::unix::process::ExitStatusExt as _;
                    let status = ExitHandle::status(child)?;
                    if status.code().is_none() && status.signal().is_none() {
                        return Err(io::Error::other(format!(
                            "the prepared native session has no supported terminal status: {status}"
                        )));
                    }
                }
                Err(source) => return Err(source.into()),
            }
        }
        Ok(())
    }

    /// SIGTERM to the whole group: the chance to run deferred cleanup and flush the output that is the evidence for why the mutant timed out.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
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
    /// Preserves the owned platform observation through this transition.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn terminate_forcefully(&self, leader: LeaderObservation) -> io::Result<()> {
        #[cfg(target_os = "macos")]
        if leader == LeaderObservation::ExitedWaitable && !self.has_member_besides_leader()? {
            return Ok(());
        }
        self.signal(Signal::KILL)
    }

    /// Preserves the owned platform observation through this transition.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
    pub(super) fn settle(&self, leader_state: LeaderObservation) -> io::Result<()> {
        let Some(leader) = self.pgid else {
            return Ok(());
        };
        if self.scope == Scope::Session {
            return self.settle_session(leader, leader_state);
        }
        let deadline = Instant::now()
            .checked_add(super::REAPING_GRACE)
            .ok_or_else(|| {
                io::Error::other("the process-group completion deadline cannot be represented")
            })?;
        let mut cancelled = self.settle_retained(leader, deadline)?;
        loop {
            if Instant::now() >= deadline {
                return Err(member_timeout());
            }
            let members = group_members(leader)?;
            if members.is_empty() {
                if !cancelled {
                    self.terminate_forcefully(leader_state)?;
                    if self.settle_retained(leader, deadline)? {
                        cancelled = true;
                        continue;
                    }
                }
                return Ok(());
            }
            let mut events = Vec::new();
            for member in members {
                if let Some(event) = MemberExit::arm(member, leader)? {
                    events.push(event);
                }
            }
            match signal_group(leader, Signal::KILL)? {
                super::Stopped::Group | super::Stopped::LeaderOnly => {}
            }
            cancelled = true;
            for event in events {
                event.wait(deadline)?;
            }
        }
    }

    pub(super) const fn original_session(&self) -> Pid {
        self.original_session
    }

    pub(super) fn settle_except_named(&self, named: &super::ForeignProcess) -> io::Result<()> {
        if self.scope != Scope::Group {
            return Err(io::Error::other(
                "a named member cannot take an original session",
            ));
        }
        let leader = self
            .pgid
            .ok_or_else(|| io::Error::other("the original group has no leader"))?;
        if !self
            .retained
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?
            .is_empty()
        {
            return Err(io::Error::other(
                "prior group cancellation prevents a named transfer",
            ));
        }
        let session = self.original_session;
        Self::confirm_named(named, leader, session)?;
        let deadline = Instant::now()
            .checked_add(super::REAPING_GRACE)
            .ok_or_else(|| io::Error::other("the non-transferred member bound overflowed"))?;
        loop {
            if Instant::now() >= deadline {
                return Err(member_timeout());
            }
            let mut others = Vec::new();
            for observed in group_members(leader)? {
                let raw =
                    u32::try_from(observed.pid.as_raw_nonzero().get()).map_err(io::Error::other)?;
                if let Some(member) = super::ForeignProcess::retain(raw)? {
                    if member.identity() == named.identity() {
                        continue;
                    }
                    if rustix::process::getpgid(Some(observed.pid))? == leader
                        && rustix::process::getsid(Some(observed.pid))? == session
                    {
                        others.push(member);
                    }
                }
            }
            if others.is_empty() {
                return Self::confirm_named(named, leader, session);
            }
            let mut failures = Vec::new();
            for member in &others {
                if let Err(source) = member.signal(super::GroupStop::Kill) {
                    failures.push(format!("{}: {source}", member.identity().token()));
                }
            }
            for member in others {
                let left = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or_else(member_timeout)?;
                match member.wait(Some(left)) {
                    Ok(true) => {}
                    Ok(false) => failures.push(format!(
                        "{}: no non-transferred exit",
                        member.identity().token()
                    )),
                    Err(source) => {
                        failures.push(format!("{}: {source}", member.identity().token()));
                    }
                }
            }
            if !failures.is_empty() {
                return Err(io::Error::other(failures.join("; ")));
            }
        }
    }

    fn confirm_named(named: &super::ForeignProcess, leader: Pid, session: Pid) -> io::Result<()> {
        let raw = i32::try_from(named.identity().pid()).map_err(io::Error::other)?;
        let pid = Pid::from_raw(raw)
            .ok_or_else(|| io::Error::other("the named member has no native PID"))?;
        if pid == leader
            || named.wait(Some(std::time::Duration::ZERO))?
            || rustix::process::getpgid(Some(pid))? != leader
            || rustix::process::getsid(Some(pid))? != session
        {
            return Err(io::Error::other(
                "the live named member does not belong to the original group and session",
            ));
        }
        let actual = super::ForeignProcess::retain(named.identity().pid())?
            .ok_or_else(|| io::Error::other("the named member generation ended during transfer"))?;
        if actual.identity() != named.identity() {
            return Err(io::Error::other(
                "the named member generation changed during transfer",
            ));
        }
        Ok(())
    }

    fn settle_session(&self, session: Pid, leader: LeaderObservation) -> io::Result<()> {
        let deadline = Instant::now()
            .checked_add(super::REAPING_GRACE)
            .ok_or_else(|| io::Error::other("the original session completion bound overflowed"))?;
        if leader == LeaderObservation::Running {
            self.signal(Signal::KILL)?;
        }
        loop {
            let members = retained_session_members(session)?;
            if members.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(member_timeout());
            }
            let mut failures = Vec::new();
            for member in &members {
                if let Err(source) = member.signal(super::GroupStop::Kill) {
                    failures.push(format!("{}: {source}", member.identity().token()));
                }
            }
            for member in members {
                let left = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or_else(member_timeout)?;
                match member.wait(Some(left)) {
                    Ok(true) => {}
                    Ok(false) => failures.push(format!(
                        "{}: no native member exit",
                        member.identity().token()
                    )),
                    Err(source) => {
                        failures.push(format!("{}: {source}", member.identity().token()));
                    }
                }
            }
            if !failures.is_empty() {
                return Err(io::Error::other(failures.join("; ")));
            }
        }
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
    /// Preserves the owned platform observation through this transition.
    pub(super) const fn membership(&self) -> Option<std::sync::Arc<Membership>> {
        None
    }

    /// Preserves the owned platform observation through this transition.
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
        match self.deliver(signal)? {
            super::Stopped::Group | super::Stopped::LeaderOnly => Ok(()),
        }
    }

    fn deliver(&self, signal: Signal) -> io::Result<super::Stopped> {
        let pgid = self.pgid.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the supervisor has no adopted process group",
            )
        })?;
        let mut retained = self
            .retained
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        for member in group_members(pgid)? {
            if !retained.iter().any(|(held, _event)| *held == member)
                && let Some(event) = MemberExit::arm(member, pgid)?
            {
                retained.push((member, event));
            }
        }
        let delivered = signal_group(pgid, signal);
        drop(retained);
        delivered
    }

    fn settle_retained(&self, leader: Pid, deadline: Instant) -> io::Result<bool> {
        let retained = {
            let mut retained = self
                .retained
                .lock()
                .map_err(|source| io::Error::other(source.to_string()))?;
            std::mem::take(&mut *retained)
        };
        if retained.is_empty() {
            return Ok(false);
        }
        match signal_group(leader, Signal::KILL)? {
            super::Stopped::Group | super::Stopped::LeaderOnly => {}
        }
        for (_member, event) in retained {
            event.wait(deadline)?;
        }
        Ok(true)
    }

    pub(super) fn stop(&self, how: super::GroupStop) -> io::Result<super::Stopped> {
        self.deliver(match how {
            super::GroupStop::Ask => Signal::TERM,
            super::GroupStop::Kill => Signal::KILL,
        })
    }

    /// Forgets the group id only after the caller has forcefully signalled the group while its leader remained waitable, then reaped that leader.
    ///
    /// # Errors
    /// The operating system refused this owned process transition.
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
        if self.pgid.is_some() {
            super::group::terminal(
                "a platform group supervisor lost its mandatory unreaped leader owner",
            );
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
    match super::checked_decide_stop(
        delivered(grouped),
        delivered(alone),
        others,
        super::decide_stop,
    )? {
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
    let leader = Pid::from_raw(leader).ok_or_else(|| io::Error::other("invalid owned leader"))?;
    group_members(leader).map(|members| !members.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Member {
    pid: Pid,
    #[cfg(target_os = "linux")]
    born: u64,
}

#[cfg(target_os = "linux")]
fn group_members(leader: Pid) -> io::Result<Vec<Member>> {
    let mut members = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let pid = match entry.file_name().to_str().map(str::parse::<i32>) {
            Some(Ok(pid)) => Pid::from_raw(pid),
            Some(Err(_)) | None => continue,
        };
        let Some(pid) = pid else {
            continue;
        };
        if pid != leader
            && let Some(member) = linux_member(pid, leader)?
        {
            members.push(member);
        }
    }
    Ok(members)
}

#[cfg(target_os = "linux")]
fn linux_member(pid: Pid, leader: Pid) -> io::Result<Option<Member>> {
    let stat = match std::fs::read_to_string(format!("/proc/{}/stat", pid.as_raw_nonzero())) {
        Ok(stat) => stat,
        Err(gone) if gone.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(source),
    };
    let (_, after_name) = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("invalid process stat"))?;
    let fields = after_name.split_whitespace().collect::<Vec<_>>();
    let state = fields
        .first()
        .ok_or_else(|| io::Error::other("process stat omitted its state"))?;
    let group = fields
        .get(2)
        .ok_or_else(|| io::Error::other("process stat omitted its group"))?
        .parse::<i32>()
        .map_err(io::Error::other)?;
    let born = fields
        .get(19)
        .ok_or_else(|| io::Error::other("process stat omitted its birth identity"))?
        .parse::<u64>()
        .map_err(io::Error::other)?;
    Ok((group == leader.as_raw_nonzero().get() && *state != "Z").then_some(Member { pid, born }))
}

/// Whether the group `pgid` leads holds a process besides its leader, as the kernel lists it.
#[cfg(target_os = "macos")]
fn member_besides_leader(pgid: Pid) -> io::Result<bool> {
    group_members(pgid).map(|members| !members.is_empty())
}

#[cfg(target_os = "macos")]
fn group_members(pgid: Pid) -> io::Result<Vec<Member>> {
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
            snapshot_has_member_besides_leader(leader, returned, &members)?;
            let mut live = Vec::new();
            for raw in members
                .into_iter()
                .take(returned)
                .filter(|pid| *pid != leader)
            {
                let member_process =
                    Pid::from_raw(raw).ok_or_else(|| io::Error::other("invalid group member"))?;
                match mac_state(raw)? {
                    Some(state) if state.group == leader && !state.exited => live.push(Member {
                        pid: member_process,
                    }),
                    Some(_settled_or_other) => {}
                    None => {
                        return Err(io::Error::other(format!(
                            "group member {raw} disappeared before its exit could be subscribed"
                        )));
                    }
                }
            }
            return Ok(live);
        }
        capacity = capacity.checked_mul(2).ok_or_else(|| {
            io::Error::other("the macOS process-group PID buffer size overflowed")
        })?;
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct MemberExit(rustix::fd::OwnedFd);

#[cfg(target_os = "linux")]
impl MemberExit {
    fn arm(member: Member, leader: Pid) -> io::Result<Option<Self>> {
        match rustix::process::pidfd_open(member.pid, rustix::process::PidfdFlags::empty()) {
            Ok(handle) => match linux_member(member.pid, leader)? {
                Some(actual) if actual == member => Ok(Some(Self(handle))),
                Some(_replaced) | None => Ok(None),
            },
            Err(rustix::io::Errno::SRCH) => Ok(None),
            Err(source) => Err(source.into()),
        }
    }

    fn wait(self, deadline: Instant) -> io::Result<()> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};

        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(member_timeout)?;
            let timeout = Timespec::try_from(left).map_err(io::Error::other)?;
            let mut events = [PollFd::new(&self.0, PollFlags::IN)];
            match poll(&mut events, Some(&timeout)) {
                Ok(0) => return Err(member_timeout()),
                Ok(_ready) if events[0].revents().contains(PollFlags::IN) => return Ok(()),
                Ok(_other) => {
                    return Err(io::Error::other("the owned pidfd refused exit observation"));
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(source) => return Err(source.into()),
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct MemberExit {
    pid: i32,
    watcher: kqueue::Watcher,
}

#[cfg(target_os = "macos")]
impl MemberExit {
    fn arm(member: Member, leader: Pid) -> io::Result<Option<Self>> {
        let pid = member.pid.as_raw_nonzero().get();
        let mut watcher = kqueue::Watcher::new()?;
        watcher.add_pid(
            pid,
            kqueue::EventFilter::EVFILT_PROC,
            kqueue::FilterFlag::NOTE_EXIT,
        )?;
        match watcher.watch() {
            Ok(()) => match mac_state(pid)? {
                Some(state) if state.group != leader.as_raw_nonzero().get() => Ok(None),
                Some(_) | None => Ok(Some(Self { pid, watcher })),
            },
            Err(source)
                if source.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()) =>
            {
                match mac_state(pid)? {
                    Some(state) if state.exited => Ok(None),
                    Some(_) | None => Err(io::Error::new(
                        source.kind(),
                        format!(
                            "member {pid} has no subscribed exit or zombie confirmation: {source}"
                        ),
                    )),
                }
            }
            Err(source) => Err(source),
        }
    }

    fn wait(self, deadline: Instant) -> io::Result<()> {
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(member_timeout)?;
            match self.watcher.poll(Some(left)) {
                Some(kqueue::Event {
                    ident: kqueue::Ident::Pid(pid),
                    data: kqueue::EventData::Proc(kqueue::Proc::Exit(_status)),
                }) if pid == self.pid => return Ok(()),
                Some(kqueue::Event {
                    data: kqueue::EventData::Error(source),
                    ..
                }) if source.kind() == io::ErrorKind::Interrupted => {}
                Some(kqueue::Event {
                    data: kqueue::EventData::Error(source),
                    ..
                }) => return Err(source),
                Some(event) => {
                    return Err(io::Error::other(format!(
                        "unexpected owned process event: {event:?}"
                    )));
                }
                None => return Err(member_timeout()),
            }
        }
    }
}

fn member_timeout() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "the owned process member has no confirmed exit event",
    )
}

fn retained_session_members(session: Pid) -> io::Result<Vec<super::ForeignProcess>> {
    let mut members = Vec::new();
    let pids = native_pids()?;
    if !pids.contains(&session) {
        return Err(io::Error::other(
            "the native inventory omitted its retained session leader",
        ));
    }
    for pid in pids {
        if pid == session {
            continue;
        }
        match rustix::process::getsid(Some(pid)) {
            Ok(actual) if actual == session => {}
            Ok(_) | Err(rustix::io::Errno::SRCH) => continue,
            Err(source) => return Err(source.into()),
        }
        let raw = u32::try_from(pid.as_raw_nonzero().get()).map_err(io::Error::other)?;
        let Some(member) = super::ForeignProcess::retain(raw)? else {
            continue;
        };
        if member.wait(Some(std::time::Duration::ZERO))? {
            continue;
        }
        match rustix::process::getsid(Some(pid)) {
            Ok(actual) if actual == session => members.push(member),
            Ok(_) | Err(rustix::io::Errno::SRCH) => {}
            Err(source) => return Err(source.into()),
        }
    }
    Ok(members)
}

#[cfg(target_os = "linux")]
fn native_pids() -> io::Result<Vec<Pid>> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let filename = entry.file_name();
        let name = filename.to_str().ok_or_else(|| {
            io::Error::other("the native process inventory contains a non-text entry")
        })?;
        if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()) {
            let raw = name.parse::<i32>().map_err(io::Error::other)?;
            let pid = Pid::from_raw(raw).ok_or_else(|| {
                io::Error::other("the native process inventory contains an invalid numeric PID")
            })?;
            pids.push(pid);
        }
    }
    Ok(pids)
}

#[cfg(target_os = "macos")]
fn native_pids() -> io::Result<Vec<Pid>> {
    #[expect(
        unsafe_code,
        reason = "proc_listallpids sizes the complete native process inventory without a buffer"
    )]
    let estimate = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if estimate <= 0 {
        return Err(io::Error::other(
            "the native process inventory returned no capacity",
        ));
    }
    let mut capacity = usize::try_from(estimate).map_err(io::Error::other)?;
    loop {
        let mut values = vec![0_i32; capacity];
        let bytes = i32::try_from(size_of_val(values.as_slice())).map_err(io::Error::other)?;
        #[expect(
            unsafe_code,
            reason = "proc_listallpids fills its bounded initialized native PID inventory"
        )]
        let returned = unsafe { proc_listallpids(values.as_mut_ptr().cast(), bytes) };
        let returned = usize::try_from(returned).map_err(io::Error::other)?;
        if returned < capacity {
            let mut pids = Vec::new();
            for raw in values.into_iter().take(returned) {
                if raw != 0 {
                    pids.push(Pid::from_raw(raw).ok_or_else(|| {
                        io::Error::other("the native process inventory contained an invalid PID")
                    })?);
                }
            }
            return Ok(pids);
        }
        capacity = capacity
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("the native PID inventory capacity overflowed"))?;
    }
}

pub(super) fn custody_peer(stream: &std::os::unix::net::UnixStream) -> io::Result<Pid> {
    use std::os::fd::AsRawFd as _;

    #[cfg(target_os = "macos")]
    let (level, option, mut value) = (0, 2, [0_i32; 1]);
    #[cfg(target_os = "linux")]
    let (level, option, mut value) = (1, 17, [0_i32; 3]);
    let mut length =
        u32::try_from(std::mem::size_of_val(value.as_slice())).map_err(io::Error::other)?;
    #[expect(
        unsafe_code,
        reason = "getsockopt reads the native peer PID or ucred into its exact initialized bounded representation"
    )]
    let result = unsafe {
        getsockopt(
            stream.as_raw_fd(),
            level,
            option,
            value.as_mut_ptr().cast(),
            std::ptr::addr_of_mut!(length),
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if usize::try_from(length).map_err(io::Error::other)? != std::mem::size_of_val(value.as_slice())
    {
        return Err(io::Error::other("the custody peer identity was incomplete"));
    }
    Pid::from_raw(value[0]).ok_or_else(|| io::Error::other("the custody peer PID is invalid"))
}

#[expect(
    unsafe_code,
    reason = "native session custody uses the existing audited Unix platform boundary"
)]
unsafe extern "C" {
    fn getsockopt(
        fd: i32,
        level: i32,
        option: i32,
        value: *mut core::ffi::c_void,
        length: *mut u32,
    ) -> i32;
    #[cfg(target_os = "macos")]
    fn proc_listallpids(buffer: *mut core::ffi::c_void, bytes: i32) -> i32;
}

#[derive(Debug)]
pub(crate) struct ExitHandle(Pid);

impl ExitHandle {
    pub(super) fn of(child: &Child) -> io::Result<Self> {
        let raw = i32::try_from(child.id()).map_err(io::Error::other)?;
        Pid::from_raw(raw)
            .map(Self)
            .ok_or_else(|| io::Error::other("the child has no waitable process identity"))
    }

    pub(super) fn status(child: &Child) -> io::Result<std::process::ExitStatus> {
        use std::os::unix::process::ExitStatusExt as _;

        let process = Self::of(child)?;
        let observed = loop {
            match waitid(
                WaitId::Pid(process.0),
                WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
            ) {
                Ok(Some(observed)) => break observed,
                Ok(None) => {
                    return Err(io::Error::other(
                        "the observed owned leader is not waitable",
                    ));
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(source) => return Err(source.into()),
            }
        };
        let status = if let Some(code) = observed.exit_status() {
            code.checked_shl(8)
                .ok_or_else(|| io::Error::other("the actual exit code exceeds its native width"))?
        } else if let Some(signal) = observed.terminating_signal() {
            signal | if observed.dumped() { 0x80 } else { 0 }
        } else {
            return Err(io::Error::other(
                "the owned leader has no terminal native status",
            ));
        };
        Ok(std::process::ExitStatus::from_raw(status))
    }

    pub(super) fn wait(self) -> io::Result<()> {
        loop {
            match waitid(
                WaitId::Pid(self.0),
                WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
            ) {
                Ok(Some(_exited)) => return Ok(()),
                Ok(None) => return Err(io::Error::other("a blocking exit wait returned no event")),
                Err(rustix::io::Errno::INTR) => {}
                Err(source) => return Err(source.into()),
            }
        }
    }
}

#[cfg(target_os = "macos")]
struct MacState {
    group: i32,
    exited: bool,
}

#[cfg(target_os = "macos")]
fn mac_state(pid: i32) -> io::Result<Option<MacState>> {
    let mut bytes = [0_u8; 64];
    let size = i32::try_from(bytes.len()).map_err(io::Error::other)?;
    #[expect(
        unsafe_code,
        reason = "the frozen 64-byte proc_bsdshortinfo API includes zombies only when its argument is one"
    )]
    let returned = unsafe { proc_pidinfo(pid, 13, 1, bytes.as_mut_ptr().cast(), size) };
    if returned == 0 {
        let source = io::Error::last_os_error();
        if source.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()) {
            return Ok(None);
        }
        return Err(source);
    }
    if returned != size {
        return Err(io::Error::other(
            "the kernel returned an incomplete process state record",
        ));
    }
    let group = i32::from_ne_bytes(bytes[8..12].try_into().map_err(io::Error::other)?);
    let status = u32::from_ne_bytes(bytes[12..16].try_into().map_err(io::Error::other)?);
    Ok(Some(MacState {
        group,
        exited: status == 5,
    }))
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

/// A kernel subscription to output readiness and the owner's stop descriptor.
#[derive(Debug)]
pub struct ReaderWait {
    stopped: io::PipeReader,
}

/// The single producer of an output reader's finite stop event.
#[derive(Debug)]
pub struct ReaderStop {
    writer: std::sync::Mutex<io::PipeWriter>,
    published: std::sync::atomic::AtomicBool,
}

impl ReaderWait {
    /// Creates the stop subscription before the output reader starts.
    ///
    /// # Errors
    /// The operating system refused its private stop pipe.
    pub fn channel() -> io::Result<(Self, ReaderStop)> {
        let (stopped, writer) = io::pipe()?;
        Ok((
            Self { stopped },
            ReaderStop {
                writer: std::sync::Mutex::new(writer),
                published: std::sync::atomic::AtomicBool::new(false),
            },
        ))
    }

    /// Waits for bytes, EOF or the owned stop descriptor without a sampling interval.
    ///
    /// # Errors
    /// The kernel refused readiness or either descriptor became invalid.
    pub fn wait(&self, reader: &io::PipeReader) -> io::Result<super::ReaderReady> {
        use rustix::event::{PollFd, PollFlags, poll};
        loop {
            let mut events = [
                PollFd::new(reader, PollFlags::IN),
                PollFd::new(&self.stopped, PollFlags::IN),
            ];
            match poll(&mut events, None) {
                Ok(_ready) => {
                    if events
                        .iter()
                        .any(|event| event.revents().intersects(PollFlags::ERR | PollFlags::NVAL))
                    {
                        return Err(io::Error::other(
                            "an owned pipe refused readiness observation",
                        ));
                    }
                    if events[1]
                        .revents()
                        .intersects(PollFlags::IN | PollFlags::HUP)
                    {
                        return Ok(super::ReaderReady::Stopped);
                    }
                    if events[0]
                        .revents()
                        .intersects(PollFlags::IN | PollFlags::HUP)
                    {
                        return Ok(super::ReaderReady::Readable);
                    }
                    return Err(io::Error::other(
                        "a blocking pipe wait returned no owned event",
                    ));
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(source) => return Err(source.into()),
            }
        }
    }
}

impl ReaderStop {
    /// Publishes one stop byte before the reader owner joins its worker.
    ///
    /// # Errors
    /// The private stop descriptor failed before its reader ended.
    pub fn stop(&self) -> io::Result<()> {
        use io::Write as _;
        if self
            .published
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Ok(());
        }
        let mut writer = self
            .writer
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        match writer.write_all(&[1]) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(source) => Err(source),
        }
    }
}

impl Drop for ReaderStop {
    fn drop(&mut self) {
        if let Err(source) = self.stop() {
            super::group::terminal(&format!(
                "the pipe owner could not publish its stop event: {source}"
            ));
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub(crate) struct ForeignHandle {
    pub(super) identity: super::ProcessIdentity,
    handle: rustix::fd::OwnedFd,
}

#[cfg(target_os = "linux")]
impl ForeignHandle {
    pub(super) fn retain(raw: u32) -> io::Result<Option<Self>> {
        let pid = Pid::from_raw(i32::try_from(raw).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other("a process subscription needs a positive PID"))?;
        let Some(identity) = linux_identity(pid)? else {
            return Ok(None);
        };
        let handle = match rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()) {
            Ok(handle) => handle,
            Err(rustix::io::Errno::SRCH) => return Ok(None),
            Err(source) => return Err(source.into()),
        };
        if linux_identity(pid)?.as_ref() != Some(&identity) {
            return Ok(None);
        }
        Ok(Some(Self { identity, handle }))
    }

    pub(super) fn wait(&self, bound: Option<std::time::Duration>) -> io::Result<bool> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        let deadline = bound
            .map(|bound| {
                Instant::now().checked_add(bound).ok_or_else(|| {
                    io::Error::other("the process observation deadline exceeds the clock")
                })
            })
            .transpose()?;
        loop {
            let timeout = deadline
                .map(|deadline| {
                    Timespec::try_from(deadline.saturating_duration_since(Instant::now()))
                        .map_err(io::Error::other)
                })
                .transpose()?;
            let mut events = [PollFd::new(&self.handle, PollFlags::IN)];
            match poll(&mut events, timeout.as_ref()) {
                Ok(0) => return Ok(false),
                Ok(_ready) if events[0].revents().contains(PollFlags::IN) => return Ok(true),
                Ok(_other) => {
                    return Err(io::Error::other(
                        "the retained pidfd refused exit observation",
                    ));
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(source) => return Err(source.into()),
            }
        }
    }

    pub(super) fn stop(&self) -> io::Result<()> {
        self.signal(super::GroupStop::Kill)
    }

    pub(super) fn signal(&self, how: super::GroupStop) -> io::Result<()> {
        let signal = match how {
            super::GroupStop::Ask => Signal::TERM,
            super::GroupStop::Kill => Signal::KILL,
        };
        match rustix::process::pidfd_send_signal(&self.handle, signal) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(source) => Err(source.into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_identity(pid: Pid) -> io::Result<Option<super::ProcessIdentity>> {
    let stat = match std::fs::read_to_string(format!("/proc/{}/stat", pid.as_raw_nonzero())) {
        Ok(stat) => stat,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(source),
    };
    let (_, after_name) = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("the process identity stat is malformed"))?;
    let fields = after_name.split_whitespace().collect::<Vec<_>>();
    if fields.first() == Some(&"Z") {
        return Ok(None);
    }
    let born = fields
        .get(19)
        .ok_or_else(|| io::Error::other("the process stat omitted its birth identity"))?
        .parse::<u64>()
        .map_err(io::Error::other)?;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim_end()
        .to_owned();
    if boot.is_empty() {
        return Err(io::Error::other("the kernel boot identity is empty"));
    }
    Ok(Some(super::ProcessIdentity {
        pid: u32::try_from(pid.as_raw_nonzero().get()).map_err(io::Error::other)?,
        born,
        boot,
    }))
}

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "generation queries and generation-bound signals remain in the existing macOS native process boundary"
)]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut core::ffi::c_void,
        size: i32,
    ) -> i32;
    fn proc_signal_with_audittoken(token: *mut AuditToken, signal: i32) -> i32;
    fn sysctlbyname(
        name: *const core::ffi::c_char,
        value: *mut core::ffi::c_void,
        size: *mut usize,
        new_value: *mut core::ffi::c_void,
        new_size: usize,
    ) -> i32;
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct AuditToken {
    words: [u32; 8],
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(crate) struct ForeignHandle {
    pub(super) identity: super::ProcessIdentity,
    watcher: Option<kqueue::Watcher>,
    version: u32,
    exited: std::sync::atomic::AtomicBool,
}

#[cfg(target_os = "macos")]
impl ForeignHandle {
    pub(super) fn retain(raw: u32) -> io::Result<Option<Self>> {
        let pid = i32::try_from(raw).map_err(io::Error::other)?;
        if pid <= 0 {
            return Err(io::Error::other(
                "a process subscription needs a positive PID",
            ));
        }
        let Some((born, version)) = mac_generation(pid)? else {
            return Ok(None);
        };
        let mut watcher = kqueue::Watcher::new()?;
        watcher.add_pid(
            pid,
            kqueue::EventFilter::EVFILT_PROC,
            kqueue::FilterFlag::NOTE_EXIT,
        )?;
        if let Err(source) = watcher.watch() {
            if source.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error())
                && mac_state(pid)?.is_some_and(|state| state.exited)
            {
                return Ok(None);
            }
            return Err(source);
        }
        let after = mac_generation(pid)?;
        let watcher = match after {
            Some((same, revision)) if same == born && revision == version => Some(watcher),
            None => Some(watcher),
            Some(_replaced) => None,
        };
        let identity = super::ProcessIdentity {
            pid: raw,
            born,
            boot: mac_boot()?,
        };
        Ok(Some(Self {
            identity,
            watcher,
            version,
            exited: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    pub(super) fn wait(&self, bound: Option<std::time::Duration>) -> io::Result<bool> {
        let Some(watcher) = &self.watcher else {
            return Ok(true);
        };
        if self.exited.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(true);
        }
        let deadline = bound
            .map(|bound| {
                Instant::now().checked_add(bound).ok_or_else(|| {
                    io::Error::other("the process observation deadline exceeds the clock")
                })
            })
            .transpose()?;
        loop {
            let left = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
            match watcher.poll_forever(left) {
                Some(kqueue::Event {
                    ident: kqueue::Ident::Pid(pid),
                    data: kqueue::EventData::Proc(kqueue::Proc::Exit(_status)),
                }) if u32::try_from(pid) == Ok(self.identity.pid) => {
                    self.exited
                        .store(true, std::sync::atomic::Ordering::Release);
                    return Ok(true);
                }
                Some(kqueue::Event {
                    data: kqueue::EventData::Error(source),
                    ..
                }) if source.kind() == io::ErrorKind::Interrupted => {}
                Some(kqueue::Event {
                    data: kqueue::EventData::Error(source),
                    ..
                }) => return Err(source),
                Some(event) => {
                    return Err(io::Error::other(format!(
                        "unexpected retained process event: {event:?}"
                    )));
                }
                None => return Ok(false),
            }
        }
    }

    fn same_generation(&self) -> io::Result<bool> {
        let pid = i32::try_from(self.identity.pid).map_err(io::Error::other)?;
        Ok(mac_generation(pid)?
            .is_some_and(|(born, version)| born == self.identity.born && version == self.version))
    }

    pub(super) fn stop(&self) -> io::Result<()> {
        self.signal(super::GroupStop::Kill)
    }

    pub(super) fn signal(&self, how: super::GroupStop) -> io::Result<()> {
        let signal = match how {
            super::GroupStop::Ask => Signal::TERM,
            super::GroupStop::Kill => Signal::KILL,
        };
        if self.wait(Some(std::time::Duration::ZERO))? {
            return Ok(());
        }
        let pid = i32::try_from(self.identity.pid).map_err(io::Error::other)?;
        let Some((born, version)) = mac_generation(pid)? else {
            return Ok(());
        };
        if born != self.identity.born {
            return Ok(());
        }
        let mut token = AuditToken {
            words: [0, 0, 0, 0, 0, self.identity.pid, 0, version],
        };
        #[expect(
            unsafe_code,
            reason = "the kernel validates the retained PID generation atomically before signaling, preventing numeric PID reuse"
        )]
        let stopped =
            unsafe { proc_signal_with_audittoken(std::ptr::addr_of_mut!(token), signal.as_raw()) };
        if stopped == 0 {
            return Ok(());
        }
        let source = io::Error::last_os_error();
        if source.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error())
            && !self.same_generation()?
        {
            return Ok(());
        }
        Err(source)
    }
}

#[cfg(target_os = "macos")]
fn mac_generation(pid: i32) -> io::Result<Option<(u64, u32)>> {
    let mut bytes = [0_u8; 56];
    let size = i32::try_from(bytes.len()).map_err(io::Error::other)?;
    #[expect(
        unsafe_code,
        reason = "proc_pidinfo fills the frozen 56-byte process-unique-identifier API structure in an initialized buffer"
    )]
    let returned = unsafe { proc_pidinfo(pid, 17, 1, bytes.as_mut_ptr().cast(), size) };
    if returned == 0 {
        let source = io::Error::last_os_error();
        if source.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()) {
            return Ok(None);
        }
        return Err(source);
    }
    if returned != size {
        return Err(io::Error::other(
            "the kernel returned an incomplete process generation record",
        ));
    }
    let born = u64::from_ne_bytes(bytes[16..24].try_into().map_err(io::Error::other)?);
    let version = u32::from_ne_bytes(bytes[32..36].try_into().map_err(io::Error::other)?);
    Ok(Some((born, version)))
}

#[cfg(target_os = "macos")]
fn mac_boot() -> io::Result<String> {
    let mut bytes = [0_u8; 64];
    let mut size = bytes.len();
    #[expect(
        unsafe_code,
        reason = "sysctlbyname copies the current kernel boot UUID into its bounded initialized buffer"
    )]
    let result = unsafe {
        sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            bytes.as_mut_ptr().cast(),
            std::ptr::addr_of_mut!(size),
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let returned = bytes
        .get(..size)
        .ok_or_else(|| io::Error::other("the kernel boot UUID exceeded its buffer"))?;
    let boot = std::ffi::CStr::from_bytes_with_nul(returned)
        .map_err(io::Error::other)?
        .to_str()
        .map_err(io::Error::other)?;
    if boot.is_empty() {
        return Err(io::Error::other("the kernel boot identity is empty"));
    }
    Ok(boot.to_owned())
}
