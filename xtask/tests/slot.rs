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
    observed: xtask::observation::Observation,
    slots: std::path::PathBuf,
    turns: std::path::PathBuf,
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("the complete owned lane observation scope");
        let slots = root.path().join("slots");
        let turns = root.path().join("turns");
        std::fs::create_dir(&slots).expect("the lane directory");
        std::fs::create_dir(&turns).expect("the turn directory");
        let observed = xtask::observation::Observation::filesystem(root.path(), true)
            .expect("subscribe before starting any lane producer");
        Self {
            observed,
            slots,
            turns,
            root,
        }
    }

    fn command(&self, script: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .args(["slot", "heavy", "--", "sh", "-c", script])
            .env("NJUTEST_SLOT_DIR", self.slots.as_path())
            .env_remove("NJUTEST_SLOT_HELD")
            .env("TURNS", self.turns.as_path())
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
        self.root
            .path()
            .join("turns")
            .join(name)
            .try_exists()
            .expect("a marker can be looked for")
    }

    fn release(&self) {
        release_turns(self.turns.as_path()).expect("the holder's release observation");
    }

    fn waiters(&self) -> usize {
        std::fs::read_dir(self.slots.as_path())
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
fn until(machine: &Machine, limit: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now()
        .checked_add(limit)
        .expect("the existing semantic limit");
    loop {
        if ready() {
            return true;
        }
        let waited = machine
            .observed
            .wait(
                "lane-test-producers",
                "marker, queue or terminal publication",
                Some(deadline),
            )
            .expect("the actual subscribed host wait");
        eprintln!(
            "{}",
            serde_json::to_string(&waited.note).expect("the actual host measurement")
        );
        match waited.event.expect("the actual producer observation") {
            xtask::observation::Event::Changed => {}
            xtask::observation::Event::Deadline => return ready(),
            xtask::observation::Event::Completed | xtask::observation::Event::Cancelled => {
                return false;
            }
        }
    }
}

fn finished_within(limit: Duration, child: &mut SupervisedChild) -> Option<ExitStatus> {
    let owner = format!("lane-test-child:{:?}", child.id());
    let started = Instant::now();
    let arrived = child
        .completion()
        .expect("the owned producer completion")
        .wait(Some(limit))
        .expect("the retained actual completion result");
    let result = arrived.then(|| {
        child
            .wait()
            .expect("the whole owned group and its inherited descriptors settled")
    });
    let note = xtask::observation::WaitNote {
        owner,
        cause: "owned group completion or the existing semantic deadline".to_owned(),
        elapsed_ns: u64::try_from(started.elapsed().as_nanos()).expect("the actual duration fits"),
        machine: xtask::observation::Machine {
            os: std::env::consts::OS,
            cpus: std::thread::available_parallelism()
                .expect("the executing host processors")
                .get(),
        },
    };
    eprintln!(
        "{}",
        serde_json::to_string(&note).expect("the actual host measurement")
    );
    result
}

#[test]
fn a_turn_wait_registers_its_pipe_before_the_release_is_observed() {
    let turns = tempfile::tempdir().expect("the owned turn rendezvous");
    let observed = xtask::observation::Observation::filesystem(turns.path(), false)
        .expect("subscribe before starting the shell producer");
    let mut command = Command::new("sh");
    command
        .args(["-c", UNTIL_GO])
        .env("TURNS", turns.path())
        .stdout(Stdio::piped());
    let mut child = SupervisedChild::launch(&mut command).expect("the real shell waiter");
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(2))
        .expect("the bounded subscription control");
    let registered = loop {
        let endpoints = std::fs::read_dir(turns.path())
            .expect("the complete turn endpoint inventory")
            .map(|entry| entry.expect("a readable endpoint").file_name())
            .filter(|name| name.to_str().expect("a UTF-8 endpoint").starts_with("go."))
            .count();
        if endpoints == 1 {
            break true;
        }
        let waited = observed
            .wait("shell-turn", "release-subscription", Some(deadline))
            .expect("the actual native observation");
        eprintln!(
            "{}",
            serde_json::to_string(&waited.note).expect("the actual host wait")
        );
        match waited.event.expect("a readable producer event") {
            xtask::observation::Event::Changed => {}
            xtask::observation::Event::Deadline => break false,
            xtask::observation::Event::Completed | xtask::observation::Event::Cancelled => {
                break false;
            }
        }
    };
    release_turns(turns.path()).expect("publish the actual release to the registered producer");
    assert!(
        child
            .wait()
            .expect("the released shell completes")
            .success()
    );
    assert!(
        registered,
        "a shell turn must register its owned release endpoint before checking the sticky marker"
    );
}

#[test]
fn a_turn_release_is_retained_before_the_shell_subscribes() {
    let turns = tempfile::tempdir().expect("the owned turn rendezvous");
    release_turns(turns.path()).expect("a release before registration");
    let mut command = Command::new("sh");
    command.args(["-c", UNTIL_GO]).env("TURNS", turns.path());
    let mut child = SupervisedChild::launch(&mut command).expect("the late subscriber");
    assert!(
        child
            .wait()
            .expect("the retained release is read")
            .success()
    );
    let names: Vec<_> = std::fs::read_dir(turns.path())
        .expect("the complete endpoint inventory")
        .map(|entry| entry.expect("a readable endpoint").file_name())
        .collect();
    assert_eq!(names, [std::ffi::OsString::from("go")]);
}

#[test]
fn a_turn_release_refuses_an_unowned_endpoint_shape() {
    let turns = tempfile::tempdir().expect("the owned turn rendezvous");
    std::fs::write(turns.path().join("go.foreign"), "unowned")
        .expect("a planted regular-file endpoint");
    let error = release_turns(turns.path()).expect_err("a regular file is not a release FIFO");
    assert!(
        error.to_string().contains("not a producer-owned FIFO"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(turns.path().join("go.foreign")).expect("unchanged"),
        b"unowned"
    );
}

struct BlockedWriter(Option<rustix::process::Pid>);

impl Drop for BlockedWriter {
    fn drop(&mut self) {
        let Some(writer) = self.0 else {
            return;
        };
        match rustix::process::kill_process(writer, rustix::process::Signal::KILL) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => {}
            Err(_unstopped) => std::process::abort(),
        }
    }
}

#[test]
fn completed_work_ends_its_blocked_writer_before_the_parent_disposes_its_cache() {
    let turns = tempfile::tempdir().expect("the producer's cache and rendezvous");
    let gate = turns.path().join("go");
    assert!(
        Command::new("mkfifo")
            .arg(&gate)
            .status()
            .expect("the explicit producer gate")
            .success()
    );
    let mut command = Command::new("sh");
    command.args(["-c", "sh -c 'read released < \"$TURNS/go\"; printf late > \"$TURNS/cache\"' & printf '%s\\n' $! > \"$TURNS/writer\"; exit 0"])
        .env("TURNS", turns.path());
    let stops = xtask::work::Stops::arm().expect("owned stop observations");
    let leader = std::cell::Cell::new(None);
    let ended = xtask::work::run(&mut command, None, &stops, |pid| {
        leader.set(Some(pid));
        Ok(())
    })
    .expect("the work's complete result");
    assert!(matches!(ended, xtask::work::Ended::Exited(status) if status.success()));
    let writer: i32 = std::fs::read_to_string(turns.path().join("writer"))
        .expect("the exact writer identity")
        .trim()
        .parse::<i32>()
        .expect("the writer pid");
    let writer = rustix::process::Pid::from_raw(writer).expect("a positive writer pid");
    let alive = xtask::work::listed()
        .expect("the executing host's actual processes")
        .iter()
        .any(|one| {
            one.pid == u32::try_from(writer.as_raw_nonzero().get()).expect("the writer pid fits")
                && Some(one.group) == leader.get()
                && !one.ended
        });
    let cleanup = BlockedWriter(alive.then_some(writer));
    assert!(
        !alive,
        "a successful leader exit must settle its producer group before cache disposal; the gated late writer is still alive"
    );
    drop(cleanup);
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("a run's output is UTF-8")
}

fn callback_refusal(from_started: bool) {
    let mut command = Command::new("sh");
    command.args(["-c", "read gate"]).stdin(Stdio::piped());
    let leader = std::cell::Cell::new(None);
    let stops = xtask::work::Stops::arm().expect("the signal producer");
    let mut heard = || {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "the actual output consumer refused",
        ))
    };
    let mut bound = xtask::work::Bound {
        ceiling: Duration::from_secs(60),
        quiet: Duration::from_secs(60),
        heard: &mut heard,
    };
    let refused = xtask::work::run(
        &mut command,
        (!from_started).then_some(&mut bound),
        &stops,
        |pid| {
            leader.set(Some(pid));
            if from_started {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "the actual start observer refused",
                ))
            } else {
                Ok(())
            }
        },
    )
    .expect_err("the observer failure remains a failure");
    assert!(
        matches!(refused, xtask::work::WorkError::Watch { source } if source.kind() == if from_started { std::io::ErrorKind::PermissionDenied } else { std::io::ErrorKind::BrokenPipe })
    );
    assert!(
        !xtask::work::listed()
            .expect("the actual process inventory")
            .iter()
            .any(|one| Some(one.pid) == leader.get() && !one.ended),
        "the callback failure returned only after the producer ended"
    );
    let waits = stops.take_waits();
    assert!(
        waits.iter().any(|wait| wait.owner
            == format!(
                "process-group:{}",
                leader.get().expect("the actual launched leader")
            )
            && wait.machine.cpus > 0),
        "the cleanup retains its measured executing-host wait"
    );
}

#[test]
fn a_start_callback_failure_returns_after_its_producer_is_reaped() {
    callback_refusal(true);
}

#[test]
fn an_output_callback_failure_returns_after_its_producer_is_reaped() {
    callback_refusal(false);
}

#[test]
fn a_second_run_waits_until_the_first_has_ended() {
    let machine = Machine::new();
    let first = machine.run(&holds_until_go());
    assert!(
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
        "the first run never started"
    );
    let progress = machine.turns.as_path().join("waiting.log");
    let told = std::fs::File::create(&progress).expect("the waiting run's progress");
    let mut second = machine.command("test ! -e \"$TURNS/inside\"");
    second.stderr(Stdio::from(told));
    let second = SupervisedChild::launch(&mut second).expect("the second run in the lane");
    assert!(
        until(&machine, Duration::from_secs(60), || {
            std::fs::read_to_string(&progress)
                .is_ok_and(|said| said.contains("waiting for the heavy lane"))
        }),
        "the second run did not say it was waiting behind the first"
    );
    machine.release();
    let first = first.wait_with_output().expect("the first run's answer");
    let second = second.wait_with_output().expect("the second run's answer");
    let waited = std::fs::read_to_string(&progress).expect("the waiting run's progress");
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert!(
        second.status.success(),
        "the second run started while the first was still inside the lane: {waited}"
    );
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
        "the holder never started"
    );
    let record = machine.slots.as_path().join("heavy.holder");
    assert!(
        until(&machine, Duration::from_secs(60), || {
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
    std::fs::read_to_string(machine.turns.as_path().join("work"))
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
            until(&machine, Duration::from_secs(60), || machine
                .marker("inside")),
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
        let work = std::fs::read_to_string(machine.turns.as_path().join("work"))
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
        "the holder never started"
    );
    let work = std::fs::read_to_string(machine.turns.as_path().join("work"))
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
    let gone = until(&machine, Duration::from_secs(10), || {
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
        "the holder never started"
    );
    let names = ["first", "second", "third", "fourth"];
    let mut waiting = Vec::new();
    for (ahead, name) in names.iter().enumerate() {
        waiting.push(machine.run(&format!("echo {name} >> \"$TURNS/order\"")));
        assert!(
            until(&machine, Duration::from_secs(60), || machine.waiters()
                == ahead + 1),
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
    let order = std::fs::read_to_string(machine.turns.as_path().join("order"))
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
        "the holder never started"
    );
    let mut dead = machine.run("echo dead >> \"$TURNS/order\"");
    assert!(
        until(&machine, Duration::from_secs(60), || machine.waiters() == 1),
        "the first waiter never began to wait"
    );
    let mut next = machine.run("echo next >> \"$TURNS/order\"");
    assert!(
        until(&machine, Duration::from_secs(60), || machine.waiters() == 2),
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
    let order = std::fs::read_to_string(machine.turns.as_path().join("order"))
        .expect("the run behind wrote its name");
    assert_eq!(order.lines().collect::<Vec<&str>>(), ["next"]);
}

/// The id `name` wrote into the turns directory.
fn written(machine: &Machine, name: &str) -> String {
    std::fs::read_to_string(machine.turns.as_path().join(name))
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")
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

/// The groups `record` names under the holder that wrote it, as the next run reads them in `this_boot`.
fn groups_of(record: &str, this_boot: Option<&str>) -> Vec<xtask::lanes::Recorded> {
    xtask::lanes::Record::read(record.as_bytes()).unreleased(this_boot)
}

#[test]
fn a_record_from_another_boot_names_no_group_and_one_without_a_boot_still_does() {
    use xtask::lanes::Recorded;

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
fn a_line_a_writer_was_killed_in_does_not_swallow_the_next_one() {
    use std::ffi::OsString;

    let machine = Machine::new();
    let environment = xtask::environment::Environment::of([
        (
            OsString::from("NJUTEST_SLOT_DIR"),
            machine.slots.as_path().as_os_str().to_owned(),
        ),
        (OsString::from("NJUTEST_SLOT_HELD"), OsString::from("heavy")),
    ]);
    let lanes = xtask::lanes::Lanes::from_environment(&environment).expect("the lanes");
    let held = lanes
        .inside(xtask::lanes::Lane::Heavy)
        .expect("the lane the run is inside");
    let boot = xtask::lanes::boot();
    let record = machine.slots.as_path().join("heavy.holder");
    std::fs::write(
        &record,
        format!(
            "pid=1\nboot={}\ngroup=40 holder=1 sess",
            boot.clone().unwrap_or_default()
        ),
    )
    .expect("a record a writer was killed in");
    held.working_on(std::process::id())
        .expect("the next group is recorded");
    let text = std::fs::read_to_string(&record).expect("the record");
    let groups = groups_of(&text, boot.as_deref());
    assert!(
        groups.iter().any(|group| group.pid == std::process::id()),
        "a line a writer was killed in the middle of took the next writer's line with it, and \
         the group that line named would be left running by the next run: {text:?}"
    );
    assert_eq!(
        xtask::lanes::Record::read(text.as_bytes()).unread(),
        ["group=40 holder=1 sess"],
        "the line nobody finished is named as one, rather than read or lost: {text:?}"
    );
}

#[test]
fn a_record_is_read_only_as_far_as_its_writers_finished_it() {
    use xtask::lanes::Record;

    let written = Record::read(
        b"pid=1\ngroup=40 holder=1 session=7 born=th\0\ngroup=41 holder=1 session=7 born=then\n\
          \xff\xfe\ngroup=42 hol",
    );
    assert_eq!(
        written
            .unreleased(None)
            .iter()
            .map(|group| group.pid)
            .collect::<Vec<u32>>(),
        [41],
        "a line the next writer ended with the torn mark, one that is not text, and the one \
         still being written are no lines, so only the group a finished line names is read"
    );
    assert_eq!(
        written.unread(),
        [
            "group=40 holder=1 session=7 born=th",
            "\\xff\\xfe",
            "group=42 hol"
        ],
        "each is named as it stands"
    );
    assert_eq!(written.field("pid"), Some("1"));
    assert!(
        Record::read(b"pid=1\n\0\n").unread().is_empty(),
        "two writers that each found the same unfinished line end it twice, which leaves an \
         empty torn line and nothing to name"
    );
}

#[test]
fn a_reaped_leader_record_does_not_prove_group_and_descriptor_settlement() {
    let machine = Machine::new();
    let holder = reaped();
    let leader = reaped();
    let record = machine.slots.as_path().join("heavy.holder");
    let original = format!(
        "pid={holder}\nholder_born=gone\nboot={}\ngroup={leader} holder={holder} session=0 born=gone\n",
        xtask::lanes::boot().expect("the actual boot identity")
    );
    std::fs::write(&record, &original).expect("the unprovable original receipt");
    let output = machine
        .run("true")
        .wait_with_output()
        .expect("the actual lane consumer");
    assert!(
        !output.status.success(),
        "a reaped leader was accepted as complete group and descriptor settlement: {output:?}"
    );
    assert_eq!(
        std::fs::read_to_string(record).expect("the retained refused receipt"),
        original
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
    let record = machine.slots.as_path().join("heavy.holder");
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
            "pid={dead}\nboot={}\ngroup={pid} holder={dead} session={session} born={born}\n",
            xtask::lanes::boot().unwrap_or_default()
        ),
    )
    .expect("an old record without the holder's start");
    let mut unknown = machine.run("true");
    let went_past = finished_within(Duration::from_secs(60), &mut unknown);
    let spared = stranger
        .try_wait()
        .expect("the stranger can still be looked at")
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
        went_past.is_some_and(|status| status.success()) && spared,
        "a record from before holders recorded their start proves neither that the holder died \
         nor that its groups are orphaned, so the next run left the group alone: {went_past:?}"
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
        until(&machine, Duration::from_secs(60), || machine
            .marker("daemon")),
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
    let turns = machine.turns.as_path().to_owned();
    let mut orphaning = Command::new("sh");
    orphaning
        .args(["-c", "sleep 300 & echo $! > \"$TURNS/member\""])
        .env("TURNS", &turns)
        .process_group(0);
    let mut leader = SupervisedChild::launch(&mut orphaning).expect("a leader that leaves");
    let group = leader.id().expect("the leader's id");
    leader.wait().expect("the leader ends at once");
    assert!(
        until(&machine, Duration::from_secs(60), || machine
            .marker("member")),
        "the member never started"
    );
    let member = written(&machine, "member");
    let member_pid = member.parse::<u32>().expect("the member's id");
    let session = xtask::lanes::session(member_pid).expect("the member's session");
    let record = machine.slots.as_path().join("heavy.holder");
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
        machine.slots.as_path().join("heavy.holder"),
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

#[test]
fn a_run_behind_a_holder_whose_work_shows_nothing_stops_waiting() {
    let machine = Machine::new();
    let mut holder = machine.run("mkdir \"$TURNS/inside\"; exec sleep 60");
    assert!(
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
        "the holder never started"
    );
    let told = machine.turns.as_path().join("stalled.log");
    let mut behind = machine.command("true");
    behind
        .env("NJUTEST_SLOT_QUIET_SECONDS", "3")
        .stderr(Stdio::from(
            std::fs::File::create(&told).expect("the waiting run's progress"),
        ));
    let mut behind = SupervisedChild::launch(&mut behind).expect("a run behind the holder");
    let ended = finished_within(Duration::from_secs(20), &mut behind);
    let asked = Command::new("kill")
        .args(["-TERM", &holder.id().expect("a live holder").to_string()])
        .status()
        .expect("kill");
    assert!(asked.success(), "the holder could not be asked to stop");
    holder.wait().expect("the holder is reaped");
    let said = std::fs::read_to_string(&told).expect("the waiting run's progress");
    assert!(
        ended.is_some_and(|status| !status.success()),
        "a holder whose work neither used the processor nor started a process held every run \
         behind it for as long as it liked: {ended:?}: {said}"
    );
    assert!(
        said.contains("XT0203") && said.contains("NJUTEST_SLOT_QUIET_SECONDS"),
        "the run that stopped waiting says why, and what bounds it: {said}"
    );
}

#[test]
fn a_run_behind_a_holder_whose_work_keeps_moving_waits_for_it() {
    let machine = Machine::new();
    let holder = machine.run(&holds_until_go());
    assert!(
        until(&machine, Duration::from_secs(60), || machine
            .marker("inside")),
        "the holder never started"
    );
    let mut behind = machine.command("true");
    behind.env("NJUTEST_SLOT_QUIET_SECONDS", "3");
    let mut behind = SupervisedChild::launch(&mut behind).expect("a run behind the holder");
    let early = finished_within(Duration::from_secs(9), &mut behind);
    machine.release();
    let late = finished_within(Duration::from_secs(60), &mut behind);
    let holder = holder.wait_with_output().expect("the holder's answer");
    assert!(holder.status.success(), "{}", text(&holder.stderr));
    assert!(
        early.is_none(),
        "work that keeps starting processes is moving however long it takes, and the run behind \
         it waited rather than give up by the clock: {early:?}"
    );
    assert!(
        late.is_some_and(|status| status.success()),
        "once the holder let go, the run behind it went in: {late:?}"
    );
}
