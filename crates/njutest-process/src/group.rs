// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mandatory owner for a leader, its inherited process set and completion observer.

use std::io;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};
use std::sync::Arc;
use std::time::Duration;

use super::event::ChildEvent;

/// A child whose native scope settles or moves to its acknowledged original owner before reap.
#[derive(Debug)]
pub struct GroupChild {
    owned: Owned,
}

/// A platform supervisor prepared before any child process exists.
#[derive(Debug)]
pub struct PreparedGroup {
    supervisor: super::sys::Supervisor,
}

/// The exact boundary that accepted a process or refused its creation or supervision.
#[derive(Debug)]
#[must_use]
pub enum GroupStart {
    /// One owner now holds the leader, members and completion observer.
    Started(GroupChild),
    /// No process was created.
    ProcessRefused {
        /// The operating system creation refusal.
        source: io::Error,
    },
    /// Every created process settled after its adoption or observation failed.
    SupervisionRefused {
        /// The adoption or exit-observation refusal retained after cleanup.
        source: io::Error,
    },
}

impl PreparedGroup {
    /// Prepares containment before a command or pipe producer starts.
    ///
    /// # Errors
    /// The operating system refused the platform container.
    pub fn new() -> io::Result<Self> {
        super::sys::Supervisor::new().map(|supervisor| Self { supervisor })
    }

    /// Starts a producer, returning a refusal only after mandatory cleanup.
    pub fn launch(self, command: &mut Command) -> GroupStart {
        match Owned::launch(self.supervisor, command) {
            Ok(owned) => GroupStart::Started(GroupChild { owned }),
            Err(StartRefusal::Process { source }) => GroupStart::ProcessRefused { source },
            Err(StartRefusal::Supervision { source }) => GroupStart::SupervisionRefused { source },
        }
    }
}

#[derive(Debug)]
enum StartRefusal {
    Process { source: io::Error },
    Supervision { source: io::Error },
}

#[derive(Debug)]
enum Settlement {
    Running,
    Settled {
        status: ExitStatus,
        refusal: Option<Arc<io::Error>>,
    },
    #[cfg(unix)]
    Transferred {
        status: ExitStatus,
    },
}

#[derive(Debug)]
struct Owned {
    child: Child,
    settlement: Settlement,
    supervisor: super::sys::Supervisor,
    exited: ExitRegistration,
    preparation_refusals: Vec<String>,
}

#[derive(Debug)]
enum ExitRegistration {
    Pending,
    Subscribed(Arc<ChildEvent>),
}

/// A group identity pinned by the owner's unreaped leader.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
pub struct Leader<'a> {
    pid: rustix::process::Pid,
    owner: &'a Owned,
}

#[cfg(unix)]
impl Leader<'_> {
    /// The owned group's numeric identity.
    #[must_use]
    pub const fn pid(self) -> rustix::process::Pid {
        self.pid
    }

    pub(super) fn stop(self, how: super::GroupStop) -> io::Result<super::Stopped> {
        self.owner.supervisor.stop(how)
    }
}

/// Only one explicitly transferred native member generation remains owned.
#[cfg(unix)]
#[derive(Debug)]
pub struct NamedMember {
    process: super::ForeignProcess,
}

#[cfg(unix)]
impl Drop for NamedMember {
    fn drop(&mut self) {
        if let Err(source) = self.process.stop() {
            terminal(&format!(
                "the transferred named member cleanup refused: {source}"
            ));
        }
    }
}

/// The genuinely reaped foreground status and its one transferred member.
#[cfg(unix)]
#[derive(Debug)]
#[must_use]
pub struct NamedMemberCompletion {
    /// The original foreground leader's actual reaped status.
    pub status: ExitStatus,
    /// Only this original native generation remains owned until disposal.
    pub member: NamedMember,
}

impl GroupChild {
    /// Starts a child with mandatory inherited-member cleanup.
    ///
    /// # Errors
    /// Creation or supervision failed after any created producer settled.
    pub fn start(command: &mut Command) -> io::Result<Self> {
        match PreparedGroup::new()?.launch(command) {
            GroupStart::Started(child) => Ok(child),
            GroupStart::ProcessRefused { source } | GroupStart::SupervisionRefused { source } => {
                Err(source)
            }
        }
    }

    /// Starts an original session that retains every nested process group until disposal.
    ///
    /// # Errors
    /// Creation or supervision failed after mandatory cleanup.
    #[cfg(unix)]
    pub(crate) fn start_session(command: &mut Command) -> io::Result<Self> {
        match Owned::launch(super::sys::Supervisor::session()?, command) {
            Ok(owned) => Ok(Self { owned }),
            Err(StartRefusal::Process { source } | StartRefusal::Supervision { source }) => {
                Err(source)
            }
        }
    }

    /// Takes the child's configured standard input once.
    pub const fn stdin(&mut self) -> Option<ChildStdin> {
        self.owned.child.stdin.take()
    }
    /// Takes the child's configured standard output once.
    pub const fn stdout(&mut self) -> Option<ChildStdout> {
        self.owned.child.stdout.take()
    }
    /// Takes the child's configured standard error once.
    pub const fn stderr(&mut self) -> Option<ChildStderr> {
        self.owned.child.stderr.take()
    }

    /// The leader identity while its process set remains owned and unreaped.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        match self.owned.settlement {
            Settlement::Running => Some(self.owned.child.id()),
            #[cfg(unix)]
            Settlement::Settled { .. } | Settlement::Transferred { .. } => None,
            #[cfg(not(unix))]
            Settlement::Settled { .. } => None,
        }
    }

    /// The retained non-reaping exit observer owned and joined by this group.
    #[must_use]
    pub fn completion(&self) -> Arc<ChildEvent> {
        Arc::clone(self.owned.event())
    }

    /// The platform's retained membership observations.
    #[must_use]
    #[cfg(unix)]
    pub const fn membership(&self) -> Option<Arc<super::Membership>> {
        self.owned.supervisor.membership()
    }

    /// The retained Windows job membership observations.
    #[must_use]
    #[cfg(windows)]
    pub fn membership(&self) -> Option<Arc<super::Membership>> {
        self.owned.supervisor.membership()
    }

    /// The retained process ownership for failure diagnostics.
    #[must_use]
    pub fn state(&self) -> String {
        self.owned.supervisor.state()
    }

    /// Requests cooperative cleanup, then settles every member and reaps the leader.
    ///
    /// # Errors
    /// Any control, observation, join or reap refused its transition.
    pub fn stop_with_grace(&mut self, grace: Duration) -> io::Result<()> {
        let mut failures = Vec::new();
        match self.owned.event().wait(Some(Duration::ZERO)) {
            Ok(true) => {}
            Ok(false) => {
                if let Err(source) = self.owned.supervisor.terminate_gently() {
                    failures.push(source.to_string());
                }
                if let Err(source) = self.owned.event().wait(Some(grace)) {
                    failures.push(source.to_string());
                }
            }
            Err(source) => failures.push(source.to_string()),
        }
        self.owned.settle(failures).map(|_status| ())
    }

    /// Checks natural leader status while the same native scope remains retained.
    ///
    /// # Errors
    /// Observation refusal triggers complete settlement and remains sticky.
    #[cfg(unix)]
    pub fn try_observe_status(&mut self) -> io::Result<Option<ExitStatus>> {
        self.owned.observe_status(Some(Duration::ZERO))
    }

    /// Waits for the natural leader status without ending its retained native scope.
    ///
    /// # Errors
    /// An observation refusal triggers complete settlement and remains sticky.
    #[cfg(unix)]
    pub fn observe_status(&mut self) -> io::Result<ExitStatus> {
        self.owned.observe_status(None)?.ok_or_else(|| {
            io::Error::other("a blocking natural leader observation returned no status")
        })
    }

    /// Returns a status only after an observed exit and complete group settlement.
    ///
    /// # Errors
    /// A failed observer still triggers mandatory cleanup before returning its refusal.
    pub fn try_wait_status(&mut self) -> io::Result<Option<ExitStatus>> {
        match &self.owned.settlement {
            #[cfg(unix)]
            Settlement::Settled { .. } | Settlement::Transferred { .. } => {
                return self.owned.result().map(Some);
            }
            #[cfg(not(unix))]
            Settlement::Settled { .. } => return self.owned.result().map(Some),
            Settlement::Running => {}
        }
        match self.owned.event().wait(Some(Duration::ZERO)) {
            Ok(false) => Ok(None),
            Ok(true) => self.owned.settle(Vec::new()).map(Some),
            Err(source) => self.owned.settle(vec![source.to_string()]).map(Some),
        }
    }

    /// Waits for the leader event, settles every member and joins before reaping.
    ///
    /// # Errors
    /// All ownership failures are retained after mandatory cleanup.
    pub fn wait_status(&mut self) -> io::Result<ExitStatus> {
        match &self.owned.settlement {
            #[cfg(unix)]
            Settlement::Settled { .. } | Settlement::Transferred { .. } => {
                return self.owned.result();
            }
            #[cfg(not(unix))]
            Settlement::Settled { .. } => return self.owned.result(),
            Settlement::Running => {}
        }
        let failures = match self.owned.event().wait(None) {
            Ok(true) => Vec::new(),
            Ok(false) => vec!["a blocking exit observation returned no event".to_owned()],
            Err(source) => vec![source.to_string()],
        };
        self.owned.settle(failures)
    }

    /// Reaps natural foreground exit only after its original native session recipient is retained.
    ///
    /// # Errors
    /// Recipient, observation or reap refusal triggers mandatory complete group settlement.
    #[cfg(unix)]
    pub fn release_to_parent(&mut self, parent: &super::ParentSession) -> io::Result<ExitStatus> {
        match self.owned.settlement {
            #[cfg(unix)]
            Settlement::Settled { .. } | Settlement::Transferred { .. } => {
                return self.owned.result();
            }
            #[cfg(not(unix))]
            Settlement::Settled { .. } => return self.owned.result(),
            Settlement::Running => {}
        }
        let status = match (|| {
            let status = self.observe_status()?;
            parent.confirm_child(
                &self.owned.child,
                self.owned.supervisor.original_session(),
                status,
            )?;
            self.owned.finish_event()?;
            Ok::<ExitStatus, io::Error>(status)
        })() {
            Ok(status) => status,
            Err(source) => return self.owned.settle(vec![source.to_string()]),
        };
        match self.owned.child.wait() {
            Ok(reaped) if reaped == status => {}
            Ok(reaped) => terminal(&format!(
                "the nested status changed across reaping: {status} to {reaped}"
            )),
            Err(source) => terminal(&format!(
                "the acknowledged nested leader reap refused: {source}"
            )),
        }
        if let Err(source) = self.owned.supervisor.release() {
            terminal(&format!(
                "the acknowledged nested supervisor release refused after reap: {source}"
            ));
        }
        self.owned.settlement = Settlement::Transferred { status };
        Ok(status)
    }

    /// Reaps the actual leader after all members except one named generation settle.
    ///
    /// # Errors
    /// Native membership or transfer refuses after mandatory complete original cleanup.
    #[cfg(unix)]
    pub fn reap_to_member(
        &mut self,
        process: super::ForeignProcess,
    ) -> io::Result<NamedMemberCompletion> {
        match self.owned.settlement {
            Settlement::Running => {}
            Settlement::Settled { .. } | Settlement::Transferred { .. } => {
                return Err(match self.owned.result() {
                    Err(source) => source,
                    Ok(_status) => io::Error::other("the original group was already consumed"),
                });
            }
        }
        let status = match (|| {
            let status = self.observe_status()?;
            self.owned.supervisor.settle_except_named(&process)?;
            self.owned.finish_event()?;
            Ok::<ExitStatus, io::Error>(status)
        })() {
            Ok(status) => status,
            Err(source) => {
                let status = self.owned.settle(vec![source.to_string()])?;
                terminal(&format!(
                    "the named member transfer refusal was lost during cleanup; unexpected status: {status}"
                ))
            }
        };
        let member = NamedMember { process };
        match self.owned.child.wait() {
            Ok(reaped) if reaped == status => {}
            Ok(reaped) => terminal(&format!(
                "the named member foreground status changed across reap: {status} to {reaped}"
            )),
            Err(source) => terminal(&format!(
                "the named member foreground reap refused: {source}"
            )),
        }
        if let Err(source) = self.owned.supervisor.release() {
            terminal(&format!(
                "the named member supervisor release refused after reap: {source}"
            ));
        }
        self.owned.settlement = Settlement::Transferred { status };
        Ok(NamedMemberCompletion { status, member })
    }

    /// Whether the complete process set settled and its leader was reaped.
    ///
    /// # Errors
    /// The retained ownership transition refused.
    pub fn try_wait(&mut self) -> io::Result<bool> {
        self.try_wait_status().map(|status| status.is_some())
    }
    /// Waits for complete process settlement and leader reaping.
    ///
    /// # Errors
    /// The retained ownership transition refused.
    pub fn wait(&mut self) -> io::Result<()> {
        self.wait_status().map(|_status| ())
    }

    /// The group capability while the leader remains unreaped.
    #[cfg(unix)]
    #[must_use]
    pub fn leader(&self) -> Option<Leader<'_>> {
        self.owned.leader()
    }

    /// Forcefully settles every member, joins the observer and reaps the leader.
    ///
    /// # Errors
    /// A failed transition is retained after mandatory settlement.
    pub fn stop(&mut self) -> io::Result<()> {
        self.owned.settle(Vec::new()).map(|_status| ())
    }

    /// Waits for natural completion until its semantic bound, then settles the set.
    ///
    /// # Errors
    /// A failed transition is retained after mandatory settlement.
    pub fn finish(&mut self, timeout: Duration) -> io::Result<()> {
        let failures = match self.owned.event().wait(Some(timeout)) {
            Ok(true | false) => Vec::new(),
            Err(source) => vec![source.to_string()],
        };
        self.owned.settle(failures).map(|_status| ())
    }
}

impl Owned {
    fn create(
        supervisor: super::sys::Supervisor,
        command: &mut Command,
    ) -> Result<Self, StartRefusal> {
        supervisor.configure(command);
        let child: Child = command
            .spawn()
            .map_err(|source| StartRefusal::Process { source })?;
        let mut owned = Self {
            child,
            settlement: Settlement::Running,
            supervisor,
            exited: ExitRegistration::Pending,
            preparation_refusals: Vec::new(),
        };
        if let Err(source) = owned.supervisor.adopt(&owned.child) {
            owned
                .preparation_refusals
                .push(format!("process adoption: {source}"));
            return Err(StartRefusal::Supervision { source });
        }
        Ok(owned)
    }

    fn launch(
        supervisor: super::sys::Supervisor,
        command: &mut Command,
    ) -> Result<Self, StartRefusal> {
        let mut owned = Self::create(supervisor, command)?;
        match ChildEvent::of(&owned.child) {
            Ok(exited) => owned.exited = ExitRegistration::Subscribed(Arc::new(exited)),
            Err(source) => {
                owned
                    .preparation_refusals
                    .push(format!("exit subscription: {source}"));
                return Err(StartRefusal::Supervision { source });
            }
        }
        Ok(owned)
    }

    fn event(&self) -> &Arc<ChildEvent> {
        match &self.exited {
            ExitRegistration::Subscribed(exited) => exited,
            ExitRegistration::Pending => {
                terminal("a process was exposed before its mandatory exit subscription")
            }
        }
    }

    fn exit_observed(&self) -> io::Result<bool> {
        match &self.exited {
            ExitRegistration::Subscribed(exited) => exited.wait(Some(Duration::ZERO)),
            ExitRegistration::Pending => Ok(false),
        }
    }

    fn finish_event(&self) -> io::Result<()> {
        match &self.exited {
            ExitRegistration::Subscribed(exited) => exited.finish(),
            ExitRegistration::Pending => Ok(()),
        }
    }

    fn result(&self) -> io::Result<ExitStatus> {
        match &self.settlement {
            Settlement::Running => Err(io::Error::other("the producer set has not settled")),
            #[cfg(unix)]
            Settlement::Transferred { status }
            | Settlement::Settled {
                status,
                refusal: None,
            } => Ok(*status),
            #[cfg(not(unix))]
            Settlement::Settled {
                status,
                refusal: None,
            } => Ok(*status),
            Settlement::Settled {
                refusal: Some(source),
                ..
            } => Err(io::Error::new(source.kind(), Arc::clone(source))),
        }
    }

    #[cfg(unix)]
    fn observe_status(&mut self, timeout: Option<Duration>) -> io::Result<Option<ExitStatus>> {
        match self.settlement {
            Settlement::Settled { .. } | Settlement::Transferred { .. } => {
                return self.result().map(Some);
            }
            Settlement::Running => {}
        }
        match self.event().wait(timeout) {
            Ok(false) => Ok(None),
            Ok(true) => match super::sys::ExitHandle::status(&self.child) {
                Ok(status) => Ok(Some(status)),
                Err(source) => self.settle(vec![source.to_string()]).map(Some),
            },
            Err(source) => self.settle(vec![source.to_string()]).map(Some),
        }
    }

    fn settle(&mut self, mut failures: Vec<String>) -> io::Result<ExitStatus> {
        match self.settlement {
            #[cfg(unix)]
            Settlement::Settled { .. } | Settlement::Transferred { .. } => return self.result(),
            #[cfg(not(unix))]
            Settlement::Settled { .. } => return self.result(),
            Settlement::Running => {}
        }
        failures.append(&mut self.preparation_refusals);
        if matches!(self.exited, ExitRegistration::Pending) {
            self.stop_prepared(&mut failures);
        }
        let leader = match self.exit_observed() {
            Ok(true) => super::LeaderObservation::ExitedWaitable,
            Ok(false) => super::LeaderObservation::Running,
            Err(source) => {
                failures.push(source.to_string());
                super::LeaderObservation::Running
            }
        };
        if let Err(source) = self.supervisor.settle(leader) {
            terminal(&format!(
                "{}; member settlement failed: {source}; prior failures: {failures:?}",
                self.supervisor.state()
            ));
        }
        self.confirm_leader(&mut failures);
        if let Err(source) = self
            .supervisor
            .settle(super::LeaderObservation::ExitedWaitable)
        {
            terminal(&format!(
                "{}; final member settlement failed: {source}; prior failures: {failures:?}",
                self.supervisor.state()
            ));
        }
        if let Err(source) = self.finish_event() {
            failures.push(source.to_string());
        }
        let status = match self.child.wait() {
            Ok(status) => status,
            Err(source) => terminal(&format!(
                "{}; owned leader reap failed: {source}; prior failures: {failures:?}",
                self.supervisor.state()
            )),
        };
        if let Err(source) = self.supervisor.release() {
            failures.push(source.to_string());
        }
        let refusal =
            (!failures.is_empty()).then(|| Arc::new(io::Error::other(failures.join("; "))));
        self.settlement = Settlement::Settled { status, refusal };
        self.result()
    }

    fn stop_prepared(&mut self, failures: &mut Vec<String>) {
        #[cfg(unix)]
        if let Err(source) = self.supervisor.adopt(&self.child) {
            failures.push(format!("retaining the created group: {source}"));
        }
        match ChildEvent::of(&self.child) {
            Ok(exited) => self.exited = ExitRegistration::Subscribed(Arc::new(exited)),
            Err(source) => failures.push(format!("created child exit subscription: {source}")),
        }
        #[cfg(windows)]
        if let Err(source) = super::sys::ExitHandle::stop_owned(&mut self.child) {
            failures.push(format!("created child cancellation: {source}"));
        }
    }

    fn kernel_confirmation(&self) -> io::Result<bool> {
        let exited = ChildEvent::of(&self.child)?;
        let observed = exited.wait(Some(super::REAPING_GRACE))?;
        if !observed {
            terminal("the retained child fallback observer has no confirmed exit event");
        }
        exited.finish()?;
        Ok(true)
    }

    fn confirm_leader(&self, failures: &mut Vec<String>) {
        let observed = match &self.exited {
            ExitRegistration::Subscribed(exited) => exited.wait(Some(super::REAPING_GRACE)),
            ExitRegistration::Pending => self.kernel_confirmation(),
        };
        let observed = match observed {
            Ok(observed) => Ok(observed),
            Err(source) => {
                failures.push(source.to_string());
                self.kernel_confirmation()
            }
        };
        match observed {
            Ok(true) => {}
            Ok(false) => terminal(&format!(
                "{}; the forcefully stopped leader has no exit event",
                self.supervisor.state()
            )),
            Err(source) => {
                failures.push(source.to_string());
                let members = self.supervisor.settle(super::LeaderObservation::Running);
                let joined = self.finish_event();
                terminal(&format!(
                    "{}; leader confirmation refused: {source}; members: {members:?}; observer join: {joined:?}; prior failures: {failures:?}",
                    self.supervisor.state()
                ));
            }
        }
    }

    #[cfg(unix)]
    fn leader(&self) -> Option<Leader<'_>> {
        match self.settlement {
            #[cfg(unix)]
            Settlement::Settled { .. } | Settlement::Transferred { .. } => return None,
            #[cfg(not(unix))]
            Settlement::Settled { .. } => return None,
            Settlement::Running => {}
        }
        let raw = match i32::try_from(self.child.id()) {
            Ok(raw) => raw,
            Err(source) => terminal(&format!(
                "the owned kernel child PID exceeds the platform width: {source}"
            )),
        };
        rustix::process::Pid::from_raw(raw).map(|pid| Leader { pid, owner: self })
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        match self.settlement {
            #[cfg(unix)]
            Settlement::Settled { .. } | Settlement::Transferred { .. } => {}
            #[cfg(not(unix))]
            Settlement::Settled { .. } => {}
            Settlement::Running => {
                if let Err(source) = self.settle(Vec::new()) {
                    eprintln!(
                        "the process set settled with a retained refusal during drop: {source}"
                    );
                }
            }
        }
    }
}

/// Reports an unproven ownership transition before refusing to detach it.
pub(crate) fn terminal(message: &str) -> ! {
    eprintln!("terminal native process ownership failure: {message}");
    std::process::abort();
}

#[cfg(test)]
mod tests {
    use super::GroupChild;

    #[cfg(unix)]
    #[test]
    fn a_child_that_has_been_reaped_leads_no_group() {
        let mut ended = GroupChild::start(&mut Command::new("true")).expect("true starts");
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

    #[cfg(unix)]
    #[test]
    fn stopping_a_group_whose_leader_has_already_exited_is_no_failure() {
        let mut ended = GroupChild::start(&mut Command::new("true")).expect("true starts");
        assert!(
            ended
                .owned
                .event()
                .wait(None)
                .expect("the leader exit event")
        );
        let stopped = ended.stop();
        assert!(
            stopped.is_ok(),
            "a child that exited before its stop arrived, and is not reaped yet, is stopped: on \
             macOS the group signal is refused with EPERM for such a group, and that is the \
             group being gone rather than a cleanup that failed: {stopped:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_left_running_is_stopped_and_reaped_when_it_is_dropped() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = std::time::Instant::now();
        let running = GroupChild::start(&mut command).expect("the child starts");
        drop(running);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "dropping a child that still runs ends it rather than waiting for it to end by itself"
        );
    }
    use std::io::{BufRead as _, Read as _};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn unwinding_before_exit_registration_still_settles_the_actual_producer() {
        let (reader, writer) = std::io::pipe().expect("the actual producer pipe");
        let mut command =
            Command::new(std::env::current_exe().expect("the real fixture executable"));
        command
            .args(["--exact", "tests::held_producer_fixture", "--nocapture"])
            .env("NJUTEST_PROCESS_OWNED_FIXTURE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(writer));
        let owned = super::Owned::create(
            super::super::sys::Supervisor::new().expect("a prepared supervisor"),
            &mut command,
        )
        .expect("the real process is owned before registering its exit");
        drop(command);
        let mut reader = std::io::BufReader::new(reader);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("actual producer readiness");
        assert_eq!(line, "owned-process-ready\n");
        let process = super::super::ForeignProcess::retain(owned.child.id())
            .expect("the exact generation")
            .expect("the live held producer");
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = owned;
            panic!("a real owner unwinds before its exit registration");
        }));
        assert!(
            unwound.is_err(),
            "the genuine panic was caught after mandatory cleanup"
        );
        assert!(
            process
                .wait(Some(Duration::ZERO))
                .expect("the actual kernel exit event")
        );
        let mut tail = Vec::new();
        assert_eq!(
            reader
                .read_to_end(&mut tail)
                .expect("actual inherited pipe EOF"),
            0
        );
    }
}
