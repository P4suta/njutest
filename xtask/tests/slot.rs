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
#[expect(
    clippy::too_many_lines,
    reason = "one row for each state the decision meets, which is the point of the table"
)]
fn a_group_is_the_work_s_while_its_leader_runs_or_a_member_shares_its_session() {
    use xtask::lanes::{Liveness, Recorded, Session, Start, group_liveness};
    use xtask::work::Listed;

    let born = "Sat Sep 26 12:00:00 2026";
    let recorded = |session: Option<u32>| Recorded {
        pid: 40,
        born: born.to_owned(),
        session,
    };
    let running = || Start::Running(born.to_owned());
    let member = |ended: bool| {
        vec![Listed {
            pid: 41,
            group: 40,
            ended,
        }]
    };
    let leader = |ended: bool| {
        vec![Listed {
            pid: 40,
            group: 40,
            ended,
        }]
    };
    let session = |of: u32| move |_member: u32| Session::Of(of);
    for (case, group, start, processes, sessions, expected) in [
        (
            "the leader runs",
            recorded(Some(7)),
            running(),
            Some(leader(false)),
            session(9),
            Liveness::Alive,
        ),
        (
            "a zombie leader and nobody else",
            recorded(Some(7)),
            running(),
            Some(leader(true)),
            session(7),
            Liveness::Gone,
        ),
        (
            "the id is somebody else's now",
            recorded(Some(7)),
            Start::Running("then".to_owned()),
            Some(leader(false)),
            session(7),
            Liveness::Gone,
        ),
        (
            "the leader's start could not be read",
            recorded(Some(7)),
            Start::Unread,
            Some(member(false)),
            session(7),
            Liveness::Unseen,
        ),
        (
            "gone leader, member in its session",
            recorded(Some(7)),
            Start::Absent,
            Some(member(false)),
            session(7),
            Liveness::Alive,
        ),
        (
            "gone leader, member in another session",
            recorded(Some(7)),
            Start::Absent,
            Some(member(false)),
            session(8),
            Liveness::Gone,
        ),
        (
            "gone leader, no session recorded",
            recorded(None),
            Start::Absent,
            Some(member(false)),
            session(7),
            Liveness::Gone,
        ),
        (
            "gone leader, nobody left",
            recorded(Some(7)),
            Start::Absent,
            Some(Vec::new()),
            session(7),
            Liveness::Gone,
        ),
        (
            "nothing could be listed",
            recorded(Some(7)),
            running(),
            None,
            session(7),
            Liveness::Unseen,
        ),
    ] {
        assert_eq!(
            group_liveness(&group, &start, processes.as_deref(), sessions),
            expected,
            "{case}: a group is the work's while its leader runs since the recorded time, or while \
             a member shares the session the gone leader had; a look that could not be taken \
             answers neither way"
        );
    }
    let unread = group_liveness(
        &recorded(Some(7)),
        &Start::Absent,
        Some(&member(false)),
        |_| Session::Unread,
    );
    assert_eq!(
        unread,
        Liveness::Unseen,
        "a member whose session could not be read ties nothing and frees nothing"
    );
}

#[test]
fn a_record_from_another_boot_names_no_group_and_one_without_a_boot_still_does() {
    use xtask::lanes::{Recorded, groups_of};

    let group = Recorded {
        pid: 40,
        born: "then".to_owned(),
        session: Some(7),
    };
    let written = "pid=1\nboot=A\ngroup=40 holder=1 session=7 born=then\n";
    assert_eq!(groups_of(written, Some("A")), std::slice::from_ref(&group));
    assert!(
        groups_of(written, Some("B")).is_empty(),
        "after a reboot every id in the record names something else, so nothing is ended for it"
    );
    assert_eq!(
        groups_of(written, None),
        std::slice::from_ref(&group),
        "a boot this run cannot read is no reason to leave the record's work running"
    );
    assert!(
        groups_of(
            "pid=1\nboot=A\ngroup=40 holder=2 session=7 born=then\n",
            Some("A")
        )
        .is_empty(),
        "a line another holder's run wrote, arriving after this holder took the record, is not this holder's"
    );
    assert!(
        groups_of(
            "pid=1\nboot=A\ngroup=40 holder=1 session=7 born=th",
            Some("A")
        )
        .is_empty(),
        "a line still being written is not read as a group started at some other time"
    );
    assert_eq!(
        groups_of("pid=1\ngroup=40 then\n", Some("A")),
        [Recorded {
            pid: 40,
            born: "then".to_owned(),
            session: None
        }],
        "a line from before sessions were recorded still names its group"
    );
}

#[test]
fn a_live_group_a_record_from_another_boot_names_is_left_alone() {
    use std::os::unix::process::CommandExt as _;

    let dead = reaped();

    let machine = Machine::new();
    let mut sleeping = Command::new("sleep");
    sleeping.arg("30").process_group(0);
    let mut stranger = SupervisedChild::launch(&mut sleeping).expect("a live group to name");
    let pid = stranger.id().expect("a live stranger");
    let born = xtask::lanes::started(pid).expect("when the stranger started");
    let session = xtask::lanes::session(pid).expect("the stranger's session");
    let record = machine.slots.path().join("heavy.holder");
    std::fs::write(
        &record,
        format!(
            "pid={dead}\nholder_born=gone\nboot=a-boot-long-gone\ngroup={pid} holder={dead} session={session} born={born}\n"
        ),
    )
    .expect("a record from another boot");
    let mut next = machine.run("true");
    let went_in = finished_within(Duration::from_secs(60), &mut next);
    let alive = stranger
        .try_wait()
        .expect("the stranger can be looked at")
        .is_none();
    std::fs::write(
        &record,
        format!(
            "pid={dead}\nholder_born=gone\nboot={}\ngroup={pid} holder={dead} session={session} born={born}\n",
            xtask::lanes::boot().unwrap_or_default()
        ),
    )
    .expect("a record from this boot");
    let mut control = machine.run("true");
    let control_in = finished_within(Duration::from_secs(60), &mut control);
    let ended = stranger
        .try_wait()
        .expect("the stranger can be looked at")
        .is_some();
    assert!(
        went_in.is_some_and(|status| status.success()),
        "{went_in:?}"
    );
    assert!(
        alive,
        "a group named by a record written in another boot is somebody else's now, and the next \
         run went in without touching it"
    );
    assert!(
        control_in.is_some_and(|status| status.success()),
        "{control_in:?}"
    );
    assert!(
        ended,
        "the same record from this boot is the dead holder's work, and it is ended"
    );
}

#[test]
fn what_a_holder_that_let_go_itself_left_in_its_group_is_left_alone() {
    let machine = Machine::new();
    let mut holder =
        machine.run("sh -c 'echo $$ > \"$TURNS/daemon\"; exec sleep 30' > /dev/null 2>&1 &");
    let finished = finished_within(Duration::from_secs(60), &mut holder);
    assert!(
        finished.is_some_and(|status| status.success()),
        "{finished:?}"
    );
    assert!(
        until(Duration::from_secs(60), || machine.marker("daemon")),
        "the daemon never started"
    );
    let daemon = written(&machine, "daemon");
    let mut next = machine.run(&format!("kill -0 {daemon}"));
    let went_in = finished_within(Duration::from_secs(60), &mut next);
    kill_outright(&daemon);
    assert!(
        went_in.is_some_and(|status| status.success()),
        "a holder that ended and let the lane go left a server in its group the way a build \
         leaves the compilation cache's, and the next run went in beside it without ending it: \
         {went_in:?}"
    );
}

#[test]
fn a_group_whose_leader_is_gone_is_ended_only_where_it_shares_the_recorded_session() {
    use std::os::unix::process::CommandExt as _;

    let dead = reaped();

    let machine = Machine::new();
    let turns = machine.turns.path().to_owned();
    let mut orphaning = Command::new("sh");
    orphaning
        .args(["-c", "sleep 300 & echo $! > \"$TURNS/member\""])
        .env("TURNS", &turns)
        .process_group(0);
    let mut leader = SupervisedChild::launch(&mut orphaning).expect("a leader that leaves");
    let group = leader.id().expect("the leader's id");
    leader.wait().expect("the leader ends at once");
    assert!(
        until(Duration::from_secs(60), || machine.marker("member")),
        "the member never started"
    );
    let member = written(&machine, "member");
    let member_pid = member.parse::<u32>().expect("the member's id");
    let session = xtask::lanes::session(member_pid).expect("the member's session");
    let record = machine.slots.path().join("heavy.holder");
    let boot = xtask::lanes::boot().unwrap_or_default();
    let line = |recorded: u32| {
        format!(
            "pid={dead}\nholder_born=gone\nboot={boot}\ngroup={group} holder={dead} session={recorded} born=a long time ago\n"
        )
    };
    std::fs::write(&record, line(session.saturating_add(1))).expect("a record in another session");
    let mut next = machine.run("true");
    let went_in = finished_within(Duration::from_secs(60), &mut next);
    let spared = Command::new("kill")
        .args(["-0", &member])
        .status()
        .expect("kill -0")
        .success();
    std::fs::write(&record, line(session)).expect("a record in the member's session");
    let mut control = machine.run(&format!("if kill -0 {member} 2>/dev/null; then exit 7; fi"));
    let control_in = finished_within(Duration::from_secs(60), &mut control);
    assert!(
        went_in.is_some_and(|status| status.success()),
        "{went_in:?}"
    );
    assert!(
        spared,
        "a group whose leader is gone and whose members are in another session is whoever reused \
         the id, and the next run went in without touching it"
    );
    assert!(
        control_in.is_some_and(|status| status.success()),
        "the same group in the recorded session is the work's, and it is ended: {control_in:?}"
    );
}

/// The id of a process that ran and was reaped, which is how a holder that died is named in a record.
fn reaped() -> u32 {
    let mut ending = Command::new("true");
    let mut ended = SupervisedChild::launch(&mut ending).expect("a process to reap");
    let pid = ended.id().expect("its id");
    ended.wait().expect("it is reaped");
    pid
}

#[test]
fn a_holder_is_alive_only_while_its_id_names_a_process_started_when_it_was() {
    use xtask::lanes::{HolderState, Start, holder_state};

    let born = "Sat Sep 26 12:00:00 2026";
    for (case, start, expected) in [
        (
            "it runs",
            Start::Running(born.to_owned()),
            HolderState::Alive,
        ),
        (
            "its id is somebody else's",
            Start::Running("then".to_owned()),
            HolderState::Dead,
        ),
        ("nothing has its id", Start::Absent, HolderState::Dead),
        (
            "its start could not be read",
            Start::Unread,
            HolderState::Unseen,
        ),
    ] {
        assert_eq!(holder_state(&start, born), expected, "{case}");
    }
}

#[test]
fn a_run_that_found_the_lock_free_waits_while_the_recorded_holder_still_runs() {
    use std::os::unix::process::CommandExt as _;

    let machine = Machine::new();
    let mut sleeping = Command::new("sleep");
    sleeping.arg("300").process_group(0);
    let mut holder =
        SupervisedChild::launch(&mut sleeping).expect("a holder whose lock was removed");
    let pid = holder.id().expect("the holder's id");
    let born = xtask::lanes::started(pid).expect("when the holder started");
    let session = xtask::lanes::session(pid).expect("the holder's session");
    std::fs::write(
        machine.slots.path().join("heavy.holder"),
        format!(
            "pid={pid}\nholder_born={born}\nboot={}\ngroup={pid} holder={pid} session={session} born={born}\n",
            xtask::lanes::boot().unwrap_or_default()
        ),
    )
    .expect("the live holder's record");
    let mut next = machine.run("true");
    let early = finished_within(Duration::from_secs(3), &mut next);
    let spared = holder
        .try_wait()
        .expect("the holder can be looked at")
        .is_none();
    kill_outright(&pid.to_string());
    holder.wait().expect("the holder is reaped");
    let late = finished_within(Duration::from_secs(60), &mut next);
    assert!(
        early.is_none() && spared,
        "a lock taken over a removed lock file is not the holder's death: the next run waited, \
         and the holder's work ran on: early {early:?}, spared {spared}"
    );
    assert!(
        late.is_some_and(|status| status.success()),
        "once the holder ended, the next run went in: {late:?}"
    );
}
