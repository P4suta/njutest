// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Native process ownership shared by the engine, test support and task runner.

#![expect(
    clippy::redundant_pub_crate,
    reason = "private modules share crate-only ownership transitions; public visibility would violate the workspace unreachable_pub gate"
)]

mod event;
mod group;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use windows as sys;

pub use event::{ChildEvent, ExitStop, ExitSubscription};
#[cfg(unix)]
pub use group::Leader;
pub use group::{GroupChild, GroupStart, PreparedGroup};
#[cfg(unix)]
pub use rust_mutants_decision::group::{Delivered, Others, StopDecision, Stopped, decide_stop};
pub use sys::{Membership, ReaderStop, ReaderWait, configure_reader, stream_ended};

/// What the leader's non-reaping completion observation established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderObservation {
    /// The leader may still execute.
    Running,
    /// The exited leader remains waitable and pins its group identity.
    ExitedWaitable,
}

/// The inherited forceful-completion backstop for an owned OS process member.
pub const REAPING_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Which cancellation signal is delivered to an owned group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupStop {
    /// Requests deferred cleanup before forceful cancellation.
    Ask,
    /// Ends execution immediately.
    Kill,
}

/// Signals the group while the unreaped leader keeps its identity owned.
///
/// # Errors
/// The kernel refused complete cancellation or observation of the owned group.
#[cfg(unix)]
pub fn stop_group(leader: Leader<'_>, how: GroupStop) -> std::io::Result<Stopped> {
    sys::stop_group(leader.pid(), how)
}

#[cfg(unix)]
fn checked_decide_stop(
    group: Delivered,
    leader: Delivered,
    others: Others,
    classify: impl FnOnce(Delivered, Delivered, Others) -> StopDecision,
) -> std::io::Result<StopDecision> {
    let decided = classify(group, leader, others);
    if rust_mutants_decision::group::agrees(group, leader, others, decided) {
        Ok(decided)
    } else {
        Err(std::io::Error::other(format!(
            "group-stop decision {decided:?} contradicts group {group:?}, leader {leader:?}, others {others:?}"
        )))
    }
}

/// The exact event that released a cancellable output reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderReady {
    /// The kernel observed readable bytes or writer EOF.
    Readable,
    /// The retained owner explicitly stopped the reader.
    Stopped,
    /// Windows anonymous pipes require a counted readiness backstop.
    #[cfg(windows)]
    AnonymousPipeBackstop,
}

/// The inherited backstop where Windows anonymous pipe handles expose no readiness event.
#[cfg(windows)]
pub const ANONYMOUS_PIPE_BACKSTOP: std::time::Duration = std::time::Duration::from_millis(5);

/// A process generation captured from the kernel rather than a bare numeric PID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pid: u32,
    born: u64,
    boot: String,
}

impl ProcessIdentity {
    /// The numeric identity whose generation this value also retains.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    /// The platform-bound generation token a producer may record beside its lease.
    #[must_use]
    pub fn token(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            std::env::consts::OS,
            self.boot,
            self.pid,
            self.born
        )
    }
}

/// A retained kernel completion subscription for one exact foreign process generation.
#[derive(Debug)]
pub struct ForeignProcess {
    handle: sys::ForeignHandle,
}

impl ForeignProcess {
    /// Captures and subscribes to a process generation before returning its identity.
    ///
    /// # Errors
    /// The kernel refused its identity or completion subscription.
    pub fn retain(pid: u32) -> std::io::Result<Option<Self>> {
        sys::ForeignHandle::retain(pid).map(|handle| handle.map(|handle| Self { handle }))
    }

    /// Subscribes only when the current kernel generation matches the recorded producer.
    ///
    /// # Errors
    /// The kernel refused identity or completion observation.
    pub fn subscribe(identity: &ProcessIdentity) -> std::io::Result<Option<Self>> {
        Ok(Self::retain(identity.pid)?.filter(|process| process.identity() == identity))
    }

    /// The exact kernel generation retained by this subscription.
    #[must_use]
    pub const fn identity(&self) -> &ProcessIdentity {
        &self.handle.identity
    }

    /// Whether this subscription agrees with a producer's exact recorded token.
    #[must_use]
    pub fn matches(&self, token: &str) -> bool {
        self.identity().token() == token
    }

    /// Waits for this process generation's actual exit or a semantic observation bound.
    ///
    /// # Errors
    /// The kernel refused the retained event or its deadline width.
    pub fn wait(&self, bound: Option<std::time::Duration>) -> std::io::Result<bool> {
        self.handle.wait(bound)
    }

    /// Cancels only the retained generation and confirms its actual completion.
    ///
    /// # Errors
    /// The exact generation refused cancellation or completion.
    pub fn stop(&self) -> std::io::Result<()> {
        self.handle.stop()?;
        if self.wait(Some(REAPING_GRACE))? {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the retained process has no confirmed exit event",
            ))
        }
    }
}

/// Cancels the kernel generation retained for `pid` and confirms that individual producer ended.
///
/// # Errors
/// The PID is invalid or its retained cancellation/completion capability was refused.
pub fn stop_process(pid: u32) -> std::io::Result<()> {
    if pid == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a process subscription needs a positive PID",
        ));
    }
    match ForeignProcess::retain(pid)? {
        Some(process) => process.stop(),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::{Delivered, Others, StopDecision, Stopped, checked_decide_stop, decide_stop};
    #[cfg(unix)]
    #[test]
    fn an_invalid_foreign_pid_is_refused_instead_of_reported_settled() {
        let error = super::stop_process(0).expect_err("zero names no retained process generation");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn group_stop_self_check_accepts_every_decision_the_classifier_makes() {
        for group in Delivered::ALL {
            for leader in Delivered::ALL {
                for others in Others::ALL {
                    let checked = checked_decide_stop(group, leader, others, decide_stop);
                    assert_eq!(
                        checked.expect("the classifier's decision matches the independent check"),
                        decide_stop(group, leader, others)
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_whole_group_claim_for_a_refused_group_is_rejected() {
        for others in [Others::Somebody, Others::Unseen] {
            let checked =
                checked_decide_stop(Delivered::Refused, Delivered::Sent, others, |_, _, _| {
                    StopDecision::Reached(Stopped::Group)
                });
            let error = checked.expect_err("the planted classifier must not pass its self-check");
            let said = error.to_string();
            assert!(
                said.contains("group-stop decision Reached(Group)")
                    && said.contains(&format!("others {others:?}")),
                "{said}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn every_planted_wrong_group_stop_decision_is_rejected() {
        for group in Delivered::ALL {
            for leader in Delivered::ALL {
                for others in Others::ALL {
                    for planted in Stopped::ALL
                        .map(StopDecision::Reached)
                        .into_iter()
                        .chain(std::iter::once(StopDecision::Failed))
                    {
                        if planted == decide_stop(group, leader, others) {
                            continue;
                        }
                        let checked = checked_decide_stop(group, leader, others, |_, _, _| planted);
                        assert!(
                            checked.is_err(),
                            "a planted {planted:?} passed for {group:?}, {leader:?}, {others:?}"
                        );
                    }
                }
            }
        }
    }

    use std::io::{BufRead as _, Read as _, Write as _};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::{ForeignProcess, GroupChild, ReaderReady, ReaderWait};

    #[test]
    fn held_producer_fixture() {
        if std::env::var_os("NJUTEST_PROCESS_OWNED_FIXTURE").is_none() {
            return;
        }
        let mut output = std::io::stderr().lock();
        writeln!(output, "owned-process-ready").expect("the real producer publishes readiness");
        output.flush().expect("readiness reaches the actual pipe");
        let mut release = [0_u8; 1];
        std::io::stdin()
            .read_exact(&mut release)
            .expect("the owner releases the real producer");
        assert_eq!(release, [b'r']);
    }

    fn held_child() -> GroupChild {
        let mut command = Command::new(std::env::current_exe().expect("the compiled fixture"));
        command
            .args(["--exact", "tests::held_producer_fixture", "--nocapture"])
            .env("NJUTEST_PROCESS_OWNED_FIXTURE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = GroupChild::start(&mut command).expect("the real owned fixture starts");
        let mut output = std::io::BufReader::new(child.stderr().expect("the producer pipe"));
        let mut line = String::new();
        output
            .read_line(&mut line)
            .expect("actual producer readiness");
        assert_eq!(
            line, "owned-process-ready\n",
            "the real process did not publish readiness"
        );
        child
    }

    #[test]
    fn a_bridge_stop_never_claims_process_completion() {
        let mut child = held_child();
        let completion = child.completion();
        let bridge = completion.subscribe().expect("an owned exit subscription");
        bridge.stopper().stop();
        assert!(!bridge.wait().expect("the bridge stop is observed"));
        assert!(
            !completion
                .wait(Some(Duration::ZERO))
                .expect("actual leader state")
        );
        child
            .stdin()
            .expect("the release pipe")
            .write_all(b"r")
            .expect("release");
        assert!(
            child
                .wait_status()
                .expect("all producers settled")
                .success()
        );
        assert!(
            completion
                .subscribe()
                .expect("a late subscription")
                .wait()
                .expect("sticky exit")
        );
    }

    #[test]
    fn dropping_the_group_settles_before_the_exit_observer_is_released() {
        let child = held_child();
        let completion = child.completion();
        assert!(
            !completion
                .wait(Some(Duration::ZERO))
                .expect("the producer is held")
        );
        drop(child);
        assert!(
            completion
                .wait(Some(Duration::ZERO))
                .expect("actual exit after mandatory cleanup")
        );
    }

    #[test]
    fn foreign_subscription_and_stop_retain_the_actual_kernel_generation() {
        let mut child = held_child();
        let pid = child.id().expect("the unreaped leader");
        let process = ForeignProcess::retain(pid)
            .expect("kernel generation query")
            .expect("a live process");
        let identity = process.identity().clone();
        let subscribed = ForeignProcess::subscribe(&identity)
            .expect("an identity subscription")
            .expect("the same live generation");
        assert!(subscribed.matches(&identity.token()));
        assert!(
            !subscribed
                .wait(Some(Duration::ZERO))
                .expect("the live generation is held")
        );
        process
            .stop()
            .expect("the exact retained kernel generation exits");
        assert!(
            subscribed
                .wait(Some(Duration::ZERO))
                .expect("the retained generation exited")
        );
        child
            .wait_status()
            .expect("the complete group settles before reap");
        assert!(
            ForeignProcess::subscribe(&identity)
                .expect("late generation query")
                .is_none()
        );
        let other = held_child();
        let replacement = ForeignProcess::retain(other.id().expect("another leader"))
            .expect("another generation")
            .expect("another live process");
        assert!(!replacement.matches(&identity.token()));
        drop(other);
    }

    #[test]
    fn pipe_readiness_eof_and_reader_stop_are_distinct_owned_events() {
        let (mut reader, mut writer) = std::io::pipe().expect("the actual pipe");
        super::configure_reader(&reader).expect("nonblocking reader");
        let (wait, stop) = ReaderWait::channel().expect("the reader stop event");
        writer.write_all(b"r").expect("actual readable data");
        assert_read_attempt(wait.wait(&reader).expect("actual pipe readiness"));
        let mut byte = [0_u8; 1];
        reader
            .read_exact(&mut byte)
            .expect("the actual readable byte");
        assert_eq!(byte, [b'r']);
        drop(writer);
        assert_read_attempt(wait.wait(&reader).expect("actual pipe EOF readiness"));
        assert_eq!(reader.read(&mut byte).expect("actual EOF"), 0);
        stop.stop().expect("owned reader stop publication");
        assert_eq!(
            wait.wait(&reader).expect("the owned stop event"),
            ReaderReady::Stopped
        );
    }
    #[cfg(unix)]
    fn assert_read_attempt(ready: ReaderReady) {
        assert_eq!(ready, ReaderReady::Readable);
    }

    #[cfg(windows)]
    fn assert_read_attempt(ready: ReaderReady) {
        match ready {
            ReaderReady::Readable | ReaderReady::AnonymousPipeBackstop => {}
            ReaderReady::Stopped => panic!("no reader stop was published before the read attempt"),
        }
    }
}
