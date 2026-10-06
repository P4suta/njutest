// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ended children the kernel hands a process that adopts what it starts, reaped where no child it started could be among them.

use std::io;
use std::os::fd::AsFd as _;

use rust_mutants_decision::descent::{Scope, could_have_started};
use rustix::process::{Pid, PidfdFlags, WaitId, WaitIdOptions};

use crate::{Asked, procfs};

/// Makes this process adopt every process it starts, so one whose parent ends is handed to it rather than to anybody above.
///
/// # Errors
/// The kernel refused to make this process a reaper.
pub fn adopt() -> io::Result<()> {
    rustix::process::set_child_subreaper(Some(rustix::process::getpid()))?;
    Ok(())
}

/// Reaps every ended child this process adopted that no child it started could be, and names each.
///
/// # Errors
/// The process table or an adopted child's end could not be read, for a reason other than another thread having reaped it first.
pub fn reap_adopted() -> io::Result<Vec<u32>> {
    if rustix::process::child_subreaper()?.is_none() {
        return Ok(Vec::new());
    }
    let me = raw(rustix::process::getpid())?;
    let own = Scope {
        group: raw(rustix::process::getpgrp())?,
        session: raw(rustix::process::getsid(None)?)?,
    };
    let mut reaped = Vec::new();
    for pid in procfs::processes()? {
        let Asked::Answered(seen) = procfs::parsed(pid)? else {
            continue;
        };
        if seen.ended
            && seen.parent == me
            && !could_have_started(pid, seen.scope, own)
            && reap(pid, &seen)?
        {
            reaped.push(pid);
        }
    }
    Ok(reaped)
}

/// A process id as `/proc` spells it.
fn raw(pid: Pid) -> io::Result<u32> {
    u32::try_from(pid.as_raw_nonzero().get()).map_err(io::Error::other)
}

/// Reaps the ended child `pid` where the process a pidfd now holds is still the one `seen` describes: true once reaped, and false where another thread reaped it first.
fn reap(pid: u32, seen: &procfs::Stat) -> io::Result<bool> {
    let child = Pid::from_raw(i32::try_from(pid).map_err(io::Error::other)?)
        .ok_or_else(|| io::Error::other("an adopted child needs a positive id"))?;
    let handle = match crate::unix::asked(rustix::process::pidfd_open(child, PidfdFlags::empty()))?
    {
        Asked::Answered(handle) => handle,
        Asked::Gone => return Ok(false),
    };
    match procfs::parsed(pid)? {
        Asked::Answered(held) if held == *seen => {}
        Asked::Answered(_) | Asked::Gone => return Ok(false),
    }
    loop {
        match rustix::process::waitid(
            WaitId::PidFd(handle.as_fd()),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
        ) {
            Ok(Some(_status)) => return Ok(true),
            Ok(None) => {
                return Err(io::Error::other(format!(
                    "adopted child {pid} ({}) ended and had no end to reap",
                    seen.name
                )));
            }
            Err(rustix::io::Errno::CHILD) => return Ok(false),
            Err(rustix::io::Errno::INTR) => {}
            Err(source) => return Err(source.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufRead as _;
    use std::process::{Command, Stdio};

    use crate::{Asked, GroupChild, procfs};

    const ADOPTER: &str = "NJUTEST_PROCESS_ADOPTER";

    fn alone(test: &str) {
        let ran = Command::new(std::env::current_exe().expect("this test binary"))
            .args(["--exact", test, "--nocapture"])
            .env(ADOPTER, "1")
            .output()
            .expect("the adopting process runs");
        assert!(
            ran.status.success(),
            "{test}, run in a process of its own so the reaper it becomes reaches no other test: \
             {:?}\n{:?}",
            std::str::from_utf8(&ran.stdout),
            std::str::from_utf8(&ran.stderr)
        );
    }

    struct Direct {
        child: std::process::Child,
    }

    impl Direct {
        fn start(command: &mut Command) -> Self {
            Self {
                child: command.spawn().expect("a child started directly"),
            }
        }

        fn reaped(&mut self) -> std::process::ExitStatus {
            self.child.wait().expect("its starter reaps it")
        }
    }

    impl Drop for Direct {
        fn drop(&mut self) {
            if let Err(refused) = self.child.kill() {
                eprintln!("the direct child refused its end: {refused}");
            }
            if let Err(refused) = self.child.wait() {
                eprintln!("the direct child refused its reaping: {refused}");
            }
        }
    }

    fn ended_children() -> Vec<(u32, procfs::Stat)> {
        let me = std::process::id();
        procfs::processes()
            .expect("the process table")
            .into_iter()
            .filter_map(|pid| match procfs::parsed(pid).expect("a process's stat") {
                Asked::Answered(stat) if stat.ended && stat.parent == me => Some((pid, stat)),
                Asked::Answered(_) | Asked::Gone => None,
            })
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn the_members_a_group_leaves_to_this_process_are_reaped_when_it_settles() {
        if std::env::var_os(ADOPTER).is_none() {
            alone(
                "adopted::tests::the_members_a_group_leaves_to_this_process_are_reaped_when_it_settles",
            );
            return;
        }
        super::adopt().expect("this process adopts");
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & echo $!"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        let mut child = GroupChild::start(&mut command).expect("the shell starts");
        let mut said = String::new();
        std::io::BufReader::new(child.stdout().expect("the shell's output"))
            .read_line(&mut said)
            .expect("the member the shell started");
        let member = said.trim().parse::<u32>().expect("a process id");
        child.wait().expect("the group settles");
        assert_eq!(
            ended_children(),
            Vec::new(),
            "the shell ended and left its member {member} to this process, which ends the member \
             when the group settles and is then the only one that can reap it"
        );
        assert_eq!(
            procfs::parsed(member).expect("the member's stat"),
            Asked::Gone,
            "the member is reaped, not merely ended"
        );
    }

    #[test]
    fn a_child_this_process_started_is_left_to_whoever_waits_for_it() {
        if std::env::var_os(ADOPTER).is_none() {
            alone("adopted::tests::a_child_this_process_started_is_left_to_whoever_waits_for_it");
            return;
        }
        super::adopt().expect("this process adopts");
        let mut grouped = GroupChild::start(&mut Command::new("true")).expect("a group leader");
        let mut session =
            GroupChild::start_session(&mut Command::new("true")).expect("a session leader");
        let mut direct = Direct::start(&mut Command::new("true"));
        let direct_pid = rustix::process::Pid::from_raw(
            i32::try_from(direct.child.id()).expect("a process id that fits"),
        )
        .expect("a positive process id");
        rustix::process::waitid(
            rustix::process::WaitId::Pid(direct_pid),
            rustix::process::WaitIdOptions::EXITED | rustix::process::WaitIdOptions::NOWAIT,
        )
        .expect("the direct child's end, left to reap");
        for child in [&grouped, &session] {
            assert!(
                child.completion().wait(None).expect("the leader's end"),
                "the leader has ended and waits to be reaped"
            );
        }
        let ended = ended_children();
        assert_eq!(
            ended.len(),
            3,
            "the three children this process started have ended unreaped: {ended:?}"
        );
        assert_eq!(
            super::reap_adopted().expect("the adopted children are reaped"),
            Vec::<u32>::new(),
            "a child this process started leads its own group or session or stays in this \
             process's, so the reaper leaves it to the code that waits for it"
        );
        assert!(grouped.wait_status().expect("its owner reaps it").success());
        assert!(session.wait_status().expect("its owner reaps it").success());
        assert!(direct.reaped().success());
    }
}
