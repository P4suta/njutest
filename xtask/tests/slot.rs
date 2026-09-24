// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A lane admits one whole-workspace run at a time, says whom a waiting run waits for, and lets go when its holder ends however it ends.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a lane directory or a child that cannot be made leaves no lane to test"
)]

use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use njutest_devkit::process::SupervisedChild;

/// The lanes and the marker files the scripted runs of one test share.
struct Machine {
    slots: tempfile::TempDir,
    turns: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        Self {
            slots: tempfile::tempdir().expect("a lane directory"),
            turns: tempfile::tempdir().expect("a directory the runs take turns in"),
        }
    }

    fn command(&self, script: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .args(["slot", "heavy", "--", "sh", "-c", script])
            .env("NJUTEST_SLOT_DIR", self.slots.path())
            .env_remove("NJUTEST_SLOT_HELD")
            .env("TURNS", self.turns.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, script: &str) -> SupervisedChild {
        SupervisedChild::launch(&mut self.command(script)).expect("a run in the lane")
    }

    fn marker(&self, name: &str) -> bool {
        self.turns
            .path()
            .join(name)
            .try_exists()
            .expect("a marker can be looked for")
    }

    fn release(&self) {
        std::fs::write(self.turns.path().join("go"), "").expect("the holder's release");
    }

    fn waiting(&self) -> bool {
        std::fs::read_dir(self.slots.path())
            .expect("a readable lane directory")
            .map(|entry| entry.expect("a readable lane entry").file_name())
            .any(|name| {
                name.to_str()
                    .expect("a UTF-8 lane entry")
                    .starts_with("heavy.waiting.")
            })
    }
}

const HOLDS_UNTIL_GO: &str = "mkdir \"$TURNS/inside\"; \
     while [ ! -e \"$TURNS/go\" ]; do sleep 0.05; done; \
     rmdir \"$TURNS/inside\"";

/// Waits until `ready` says so, or `limit` passes, and says which.
fn until(limit: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    ready()
}

fn finished_within(limit: Duration, child: &mut SupervisedChild) -> Option<ExitStatus> {
    let started = Instant::now();
    while started.elapsed() < limit {
        if let Some(status) = child.try_wait().expect("the run can be looked at") {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("a run's output is UTF-8")
}

#[test]
fn a_second_run_waits_until_the_first_has_ended() {
    let machine = Machine::new();
    let first = machine.run(HOLDS_UNTIL_GO);
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")),
        "the first run never started"
    );
    let second = machine.run("test ! -e \"$TURNS/inside\"");
    assert!(
        until(Duration::from_secs(60), || machine.waiting()),
        "the second run did not queue behind the first"
    );
    machine.release();
    let first = first.wait_with_output().expect("the first run's answer");
    let second = second.wait_with_output().expect("the second run's answer");
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert!(
        second.status.success(),
        "the second run started while the first was still inside the lane: {}",
        text(&second.stderr)
    );
    let waited = text(&second.stderr);
    assert!(
        waited.contains("waiting for the heavy lane") && waited.contains("$TURNS/go"),
        "a run that waits says what it waits for: {waited}"
    );
}

#[test]
fn a_run_inside_a_held_lane_does_not_wait_for_itself() {
    let machine = Machine::new();
    let holder = machine.run(HOLDS_UNTIL_GO);
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")),
        "the holder never started"
    );
    let mut nested = machine.command("true");
    nested.env("NJUTEST_SLOT_HELD", "heavy");
    let mut nested = SupervisedChild::launch(&mut nested).expect("a nested run");
    let ended = finished_within(Duration::from_secs(60), &mut nested);
    machine.release();
    let holder = holder.wait_with_output().expect("the holder's answer");
    assert!(holder.status.success(), "{}", text(&holder.stderr));
    assert!(
        ended.is_some_and(|status| status.success()),
        "a run already inside the lane waited for the lane it is inside: the gate's own \
         `mise run check` would wait for the gate forever"
    );
}

#[test]
fn a_killed_holder_lets_the_next_run_in() {
    let machine = Machine::new();
    let mut holder =
        machine.run("mkdir \"$TURNS/inside\"; while [ ! -e \"$TURNS/go\" ]; do sleep 0.05; done");
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")),
        "the holder never started"
    );
    let pid = holder.id().expect("a live holder").to_string();
    let killed = Command::new("kill")
        .args(["-KILL", &pid])
        .status()
        .expect("kill");
    assert!(killed.success(), "the holder could not be killed");
    let mut next = machine.run("true");
    let ended = finished_within(Duration::from_secs(60), &mut next);
    machine.release();
    holder.wait().expect("the killed holder is reaped");
    assert!(
        ended.is_some_and(|status| status.success()),
        "a holder that died without letting go kept the lane: every later run on the machine \
         would wait for a process that no longer exists"
    );
}

#[test]
fn a_run_answers_with_its_commands_exit_status() {
    let machine = Machine::new();
    for code in [3, 75] {
        let answer = machine
            .run(&format!("exit {code}"))
            .wait_with_output()
            .expect("the run's answer");
        assert_eq!(
            answer.status.code(),
            Some(code),
            "the lane answered for the command rather than passing its status on: {}",
            text(&answer.stderr)
        );
    }
}

#[test]
fn a_command_is_told_which_lane_it_is_inside() {
    let machine = Machine::new();
    let answer = machine
        .run("test \"$NJUTEST_SLOT_HELD\" = heavy")
        .wait_with_output()
        .expect("the run's answer");
    assert!(
        answer.status.success(),
        "a command inside the lane that asks for it again would wait for itself: {}",
        text(&answer.stderr)
    );
}
