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

include!("support/turns.rs");

/// A run that says which process its work is, marks itself inside the lane, and waits there until it is let go.
fn working_until_go() -> String {
    format!("echo $$ > \"$TURNS/work\"; mkdir \"$TURNS/inside\"; {UNTIL_GO}")
}

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
            .env("XTASK", env!("CARGO_BIN_EXE_xtask"))
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
        self.waiters() > 0
    }

    fn waiters(&self) -> usize {
        std::fs::read_dir(self.slots.path())
            .expect("a readable lane directory")
            .map(|entry| entry.expect("a readable lane entry").file_name())
            .filter(|name| {
                name.to_str()
                    .expect("a UTF-8 lane entry")
                    .starts_with("heavy.waiting.")
            })
            .count()
    }
}

/// A run that marks itself inside the lane, waits there until it is let go, and marks itself out.
fn holds_until_go() -> String {
    format!("mkdir \"$TURNS/inside\"; {UNTIL_GO}; rmdir \"$TURNS/inside\"")
}

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
    let first = machine.run(&holds_until_go());
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
    let holder = machine.run(&holds_until_go());
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

/// Kills the run `holder` outright once the work it started is recorded, and says which process that work is.
fn orphan_the_work(machine: &Machine, holder: &mut SupervisedChild) -> String {
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")),
        "the holder never started"
    );
    let record = machine.slots.path().join("heavy.holder");
    assert!(
        until(Duration::from_secs(60), || {
            std::fs::read_to_string(&record).is_ok_and(|text| text.contains("group="))
        }),
        "the holder never recorded the work it started"
    );
    let pid = holder.id().expect("a live holder").to_string();
    let killed = Command::new("kill")
        .args(["-KILL", &pid])
        .status()
        .expect("kill");
    assert!(killed.success(), "the holder could not be killed");
    holder.wait().expect("the killed holder is reaped");
    std::fs::read_to_string(machine.turns.path().join("work"))
        .expect("the work said who it is")
        .trim()
        .to_owned()
}

/// A run whose command succeeds only if the process `work` is gone by the time it runs.
fn after_it(machine: &Machine, work: &str) -> SupervisedChild {
    machine.run(&format!("if kill -0 {work} 2>/dev/null; then exit 7; fi"))
}

#[test]
fn a_killed_holder_s_work_is_ended_before_the_next_run_goes_in() {
    let machine = Machine::new();
    let mut holder = machine.run(&working_until_go());
    let work = orphan_the_work(&machine, &mut holder);
    let mut next = after_it(&machine, &work);
    let ended = finished_within(Duration::from_secs(60), &mut next);
    machine.release();
    assert!(
        ended.is_some_and(|status| status.success()),
        "the work a killed holder left running answers to nobody, so the next run ends it and goes \
         in once it has ended, rather than sharing the lane with it or waiting for as long as it \
         chooses to run: {ended:?}"
    );
}

#[test]
fn work_that_will_not_stop_when_asked_is_killed_before_the_next_run_goes_in() {
    let machine = Machine::new();
    let mut holder = machine.run(&format!("trap '' TERM; {}", working_until_go()));
    let work = orphan_the_work(&machine, &mut holder);
    let mut next = after_it(&machine, &work);
    let ended = finished_within(Duration::from_secs(60), &mut next);
    machine.release();
    assert!(
        ended.is_some_and(|status| status.success()),
        "a loop that ignores the request to stop is how a lane stayed held by an orphan for every \
         session on the machine; after the grace it is killed, and the next run goes in: {ended:?}"
    );
}

#[test]
fn a_run_asked_to_stop_stops_its_work_first() {
    for signal in ["-TERM", "-INT"] {
        let machine = Machine::new();
        let mut command = machine.command(&working_until_go());
        let mut run = SupervisedChild::launch(&mut command).expect("a run in the lane");
        assert!(
            until(Duration::from_secs(60), || machine.marker("inside")),
            "the run never started"
        );
        let unread = run.take_stderr();
        drop(unread);
        let pid = run.id().expect("a live run").to_string();
        let sent = Command::new("kill")
            .args([signal, &pid])
            .status()
            .expect("kill");
        assert!(sent.success(), "{signal} could not be sent");
        let ended = finished_within(Duration::from_secs(60), &mut run);
        let work = std::fs::read_to_string(machine.turns.path().join("work"))
            .expect("the work said who it is");
        let alive = Command::new("kill")
            .args(["-0", work.trim()])
            .status()
            .expect("kill -0");
        machine.release();
        assert!(ended.is_some(), "{signal}: the run did not end");
        assert!(
            !alive.success(),
            "{signal}: the run ended and left its work running without the lane, even with \
             nobody reading what it would have said"
        );
    }
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

#[test]
fn a_test_that_ends_while_its_run_waits_leaves_no_worker_behind() {
    let machine = Machine::new();
    let mut holder = machine.run(&working_until_go());
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")),
        "the holder never started"
    );
    let work = std::fs::read_to_string(machine.turns.path().join("work"))
        .expect("the work said who it is")
        .trim()
        .to_owned();
    let pid = holder.id().expect("a live holder").to_string();
    let killed = Command::new("kill")
        .args(["-KILL", &pid])
        .status()
        .expect("kill");
    assert!(killed.success(), "the holder could not be killed");
    holder.wait().expect("the killed holder is reaped");
    drop(machine);
    let gone = until(Duration::from_secs(10), || {
        !Command::new("kill")
            .args(["-0", &work])
            .status()
            .expect("kill -0")
            .success()
    });
    assert!(
        gone,
        "a test that ended as a panicking one does, its run killed and its directories removed, \
         left the work its run started waiting for a release nobody will write; under measurement \
         that worker kept the machine's lane for every session"
    );
}

#[test]
fn runs_that_wait_go_in_in_the_order_they_began_to_wait() {
    let machine = Machine::new();
    let mut holder = machine.run(&holds_until_go());
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")),
        "the holder never started"
    );
    let names = ["first", "second", "third", "fourth"];
    let mut waiting = Vec::new();
    for (ahead, name) in names.iter().enumerate() {
        waiting.push(machine.run(&format!("echo {name} >> \"$TURNS/order\"")));
        assert!(
            until(Duration::from_secs(60), || machine.waiters() == ahead + 1),
            "{name} never began to wait"
        );
    }
    machine.release();
    for run in waiting.iter_mut().chain(std::iter::once(&mut holder)) {
        assert!(
            finished_within(Duration::from_secs(60), run).is_some_and(|status| status.success()),
            "every run went in and finished"
        );
    }
    let order = std::fs::read_to_string(machine.turns.path().join("order"))
        .expect("every waiting run wrote its name");
    assert_eq!(
        order.lines().collect::<Vec<&str>>(),
        names,
        "the lane admits runs in the order they began to wait: a run that polls at the right \
         moment going in ahead of one that has waited half an hour is how a push starved behind \
         every push that came after it"
    );
}

#[test]
fn a_run_that_died_while_it_waited_holds_no_place_in_the_line() {
    let machine = Machine::new();
    let mut holder = machine.run(&holds_until_go());
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")),
        "the holder never started"
    );
    let mut dead = machine.run("echo dead >> \"$TURNS/order\"");
    assert!(
        until(Duration::from_secs(60), || machine.waiters() == 1),
        "the first waiter never began to wait"
    );
    let mut next = machine.run("echo next >> \"$TURNS/order\"");
    assert!(
        until(Duration::from_secs(60), || machine.waiters() == 2),
        "the second waiter never began to wait"
    );
    let pid = dead.id().expect("a live waiter").to_string();
    let killed = Command::new("kill")
        .args(["-KILL", &pid])
        .status()
        .expect("kill");
    assert!(killed.success(), "the first waiter could not be killed");
    dead.wait().expect("the killed waiter is reaped");
    machine.release();
    let went_in = finished_within(Duration::from_secs(20), &mut next);
    assert!(
        went_in.is_some_and(|status| status.success()),
        "a waiter killed in the line left a ticket nobody will use, and the run behind it never \
         went in: {went_in:?}"
    );
    assert!(
        finished_within(Duration::from_secs(60), &mut holder).is_some(),
        "the holder finished"
    );
    let order = std::fs::read_to_string(machine.turns.path().join("order"))
        .expect("the run behind wrote its name");
    assert_eq!(order.lines().collect::<Vec<&str>>(), ["next"]);
}

/// The id `name` wrote into the turns directory.
fn written(machine: &Machine, name: &str) -> String {
    std::fs::read_to_string(machine.turns.path().join(name))
        .expect("the run wrote its id")
        .trim()
        .to_owned()
}

/// Kills `pid`, or the group it leads when given as `-pid`, outright.
fn kill_outright(target: &str) {
    let killed = Command::new("kill")
        .args(["-KILL", "--", target])
        .status()
        .expect("kill");
    assert!(killed.success(), "{target} could not be killed");
}

#[test]
fn a_member_that_outlives_its_leader_is_ended_before_the_next_run_goes_in() {
    let machine = Machine::new();
    let mut holder = machine.run(&format!(
        "sh -c 'trap \"\" TERM; echo $$ > \"$TURNS/member\"; {}' & \
         echo $$ > \"$TURNS/work\"; mkdir \"$TURNS/inside\"; wait",
        UNTIL_GO.replace('\'', "'\\''")
    ));
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")
            && machine.marker("member")),
        "the holder and its member never started"
    );
    let member = written(&machine, "member");
    kill_outright(&holder.id().expect("a live holder").to_string());
    holder.wait().expect("the killed holder is reaped");
    let mut next = after_it(&machine, &member);
    let ended = finished_within(Duration::from_secs(60), &mut next);
    machine.release();
    assert!(
        ended.is_some_and(|status| status.success()),
        "the group, not its leader, is what has to be gone: a member that ignores the request to \
         stop outlives a leader that obeys it, and the next run went in beside it: {ended:?}"
    );
}

#[test]
fn a_group_started_inside_the_held_lane_is_ended_with_the_holder_s_own() {
    let machine = Machine::new();
    let mut holder = machine.run(&format!(
        "echo $$ > \"$TURNS/outer\"; \"$XTASK\" slot heavy -- sh -c 'trap \"\" TERM; \
         echo $$ > \"$TURNS/nested\"; mkdir \"$TURNS/inside\"; {}'",
        UNTIL_GO.replace('\'', "'\\''")
    ));
    assert!(
        until(Duration::from_secs(60), || machine.marker("inside")
            && machine.marker("nested")),
        "the nested work never started"
    );
    let nested = written(&machine, "nested");
    let outer = written(&machine, "outer");
    kill_outright(&holder.id().expect("a live holder").to_string());
    holder.wait().expect("the killed holder is reaped");
    kill_outright(&format!("-{outer}"));
    let mut next = after_it(&machine, &nested);
    let ended = finished_within(Duration::from_secs(60), &mut next);
    machine.release();
    assert!(
        ended.is_some_and(|status| status.success()),
        "work that started a group of its own inside the held lane, as the gate's check does, \
         registered it, so the next run ends it too rather than going in beside it: {ended:?}"
    );
}

#[test]
fn a_group_is_alive_while_it_holds_a_process_that_has_not_ended_and_unseen_when_none_could_be_listed()
 {
    use xtask::lanes::{Liveness, group_liveness};
    use xtask::work::Listed;

    let listed = |processes: &[(u32, u32, bool)]| -> Vec<Listed> {
        processes
            .iter()
            .map(|&(pid, group, ended)| Listed { pid, group, ended })
            .collect()
    };
    let born = "Sat Sep 26 12:00:00 2026";
    for (case, started_now, processes, expected) in [
        (
            "the leader runs",
            Some(born),
            Some(listed(&[(40, 40, false)])),
            Liveness::Alive,
        ),
        (
            "only a member runs",
            None,
            Some(listed(&[(41, 40, false)])),
            Liveness::Alive,
        ),
        (
            "the leader waits to be reaped",
            Some(born),
            Some(listed(&[(40, 40, true)])),
            Liveness::Gone,
        ),
        (
            "nobody is left",
            None,
            Some(listed(&[(7, 7, false)])),
            Liveness::Gone,
        ),
        (
            "the id names somebody else now",
            Some("Sat Sep 26 13:00:00 2026"),
            Some(listed(&[(40, 40, false)])),
            Liveness::Gone,
        ),
        ("nothing could be listed", None, None, Liveness::Unseen),
        (
            "the leader runs but nothing could be listed",
            Some(born),
            None,
            Liveness::Unseen,
        ),
    ] {
        assert_eq!(
            group_liveness(40, born, started_now, processes.as_deref()),
            expected,
            "{case}: a group is gone only when a look at it finds nobody that has not ended, and a \
             look that could not be taken answers neither way"
        );
    }
}
