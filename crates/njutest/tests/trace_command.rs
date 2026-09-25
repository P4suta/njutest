// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `njutest trace` reads out of a recording: which proof made a run shorter, which command made it long, and what moved between two runs.
//!
//! Every test here needs a published report, and `Store::keep` answers `NJ6004` on Windows because publication is rooted at a POSIX directory capability. `docs/limitations.md` says so; these say it by not existing there.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helpers that build a recording are not themselves tests, and one that \
              cannot be written is a setup failure to report by panicking"
)]

use std::collections::BTreeMap;

use njutest::app::trace::{
    COMMAND_WIDTH, SLOWEST_KEPT, commands, counts, delta, describe, keys, phases, proofs, said,
    slowest,
};
use njutest::trace::{
    DischargeRecord, Event, ExecRecord, Payload, PhaseRecord, Problem, RouteRecord,
};

fn at(seq: u64, payload: Payload) -> Event {
    Event {
        seq,
        timestamp: "2027-01-15T08:00:00Z".to_owned(),
        elapsed_ms: 0,
        payload,
    }
}

fn discharging(seq: u64, proofs: &[&str]) -> Event {
    at(
        seq,
        Payload::Route {
            route: RouteRecord {
                mutant: format!("m{seq}"),
                granularity: rust_mutants::session::Granularity::Block,
                discharged: proofs
                    .iter()
                    .map(|proof| DischargeRecord {
                        target: format!("t{proof}"),
                        proof: (*proof).to_owned(),
                    })
                    .collect(),
                fallback: None,
                reaching: Vec::new(),
                tests: Vec::new(),
                considered: Vec::new(),
                reused: None,
                refused: None,
            },
        },
    )
}

fn ran(seq: u64, argv: &[&str], duration_ms: u64) -> Event {
    at(
        seq,
        Payload::Exec {
            exec: ExecRecord {
                argv: argv.iter().map(|one| (*one).to_owned()).collect(),
                dir: None,
                env_names: Vec::new(),
                timeout_ms: None,
                stopped: rust_mutants::execute::Stopped::Exited {
                    exit: rust_mutants::runner::ProcessExit::Code(0),
                },
                duration_ms,
                output_bytes: 0,
                output_sha256: None,
                output_truncated: false,
                output_path: None,
                error: None,
                output: Vec::new(),
            },
        },
    )
}

fn ended(seq: u64, name: &str, duration_ms: u64) -> Event {
    at(
        seq,
        Payload::PhaseEnd {
            phase: PhaseRecord {
                name: name.to_owned(),
                duration_ms: Some(duration_ms),
            },
        },
    )
}

#[test]
fn the_proofs_are_counted_by_how_many_executions_each_removed() {
    let events = [
        discharging(1, &["never-infected", "branch-never-taken"]),
        discharging(2, &["never-infected"]),
        discharging(3, &["never-infected", "branch-never-taken", "aardvark"]),
    ];
    assert_eq!(
        proofs(&events),
        vec![
            ("never-infected".to_owned(), 3),
            ("branch-never-taken".to_owned(), 2),
            ("aardvark".to_owned(), 1),
        ],
        "a reader who sees a run go faster asks which proof did it, so the one that did \
         the most is first; two that did the same are in the order a person can find \
         them in, which is alphabetical and not whichever the map happened to hold"
    );
    assert!(
        proofs(&[ran(1, &["cargo"], 1)]).is_empty(),
        "and a recording with no route in it discharged nothing, rather than answering \
         with a proof nobody named"
    );
}

#[test]
fn the_slowest_commands_are_the_ones_a_person_asks_about() {
    let mut events: Vec<Event> = (1..=8)
        .map(|one| ran(one, &["/usr/bin/cargo", &format!("job{one}")], one * 10))
        .collect();
    events.push(ran(9, &["/usr/bin/cargo", "tied"], 80));
    let slow = slowest(&events);
    assert_eq!(
        slow.len(),
        SLOWEST_KEPT,
        "a summary names a few, because a list of every command a run started is the \
         recording again and not an answer"
    );
    assert_eq!(
        slow.first().map(|(ms, _line)| *ms),
        Some(80),
        "the longest first: a run is mostly the time its subprocesses take, and the \
         question is which of them"
    );
    assert_eq!(
        slow.first().map(|(_ms, line)| line.as_str()),
        Some("cargo job8"),
        "and two that took the same are in the order a person can find them in: {slow:?}"
    );
    assert!(
        slow.iter().all(|(ms, _line)| *ms >= 40),
        "while the quick ones are the ones nobody is asking about: {slow:?}"
    );
}

#[test]
fn a_command_is_shown_by_its_own_name_and_cut_where_it_stops_being_readable() {
    assert_eq!(
        said(&["/usr/local/bin/cargo".to_owned(), "test".to_owned()]),
        "cargo test",
        "a program is named the way a person says it, because the directory it was \
         found in is the same for every line and tells a reader nothing"
    );
    assert_eq!(said(&[]), "", "a command with no program is no line at all");

    let long: Vec<String> = std::iter::once("cargo".to_owned())
        .chain((0..40).map(|one| format!("--flag-{one}")))
        .collect();
    let line = said(&long);
    assert!(
        line.chars().count() <= COMMAND_WIDTH.saturating_add(2) && line.ends_with('…'),
        "and a command longer than a line is cut where it stops being readable, with \
         something to say it was cut: a line that runs off the terminal takes the ones \
         above it with it, and one cut without a mark is a command a reader can neither \
         run nor recognise while the ones they could run look the same. It said \
         {line:?}, {} characters",
        line.chars().count()
    );

    let wide = said(&["x".repeat(COMMAND_WIDTH.saturating_add(20))]);
    assert!(
        wide.ends_with('…') && wide.chars().count() <= COMMAND_WIDTH.saturating_add(2),
        "and so is a program whose own name is longer than the line: {wide:?}"
    );
    let exact = said(&["cargo".to_owned(), "x".repeat(COMMAND_WIDTH - 6)]);
    assert_eq!(
        exact.chars().count(),
        COMMAND_WIDTH,
        "a command that fills the line exactly is not one that ran over it: {exact:?}"
    );
    assert!(
        !exact.ends_with('…'),
        "while one that fits is not marked as cut, or every line carries a mark and none \
         of them means anything: {exact:?}"
    );

    let filled = said(&[
        "cargo".to_owned(),
        "x".repeat(COMMAND_WIDTH - 6),
        "--and-one-more".to_owned(),
    ]);
    assert!(
        filled.ends_with('…') && filled.starts_with("cargo x"),
        "and one that fills the line exactly and has an argument left over was cut, \
         whether or not the characters ran over: what a reader is not shown is what the \
         mark is about. It said {filled:?}"
    );

    let long = said(&["cargo".to_owned(), "y".repeat(COMMAND_WIDTH * 2)]);
    assert!(
        long.starts_with("cargo y"),
        "a line that was cut kept its beginning, because the program and its first \
         arguments are what a reader is looking for: {long:?}"
    );
}

#[test]
fn what_a_recording_holds_is_counted_by_type_by_phase_and_by_program() {
    let events = [
        ran(1, &["/bin/cargo", "test"], 5),
        ran(2, &["cargo", "build"], 7),
        ran(3, &["/opt/rustc", "--version"], 1),
        ended(4, "baseline", 100),
        ended(5, "baseline", 40),
        ended(6, "mutation", 9),
    ];
    assert_eq!(
        counts(&events),
        BTreeMap::from([("exec".to_owned(), 3), ("phase-end".to_owned(), 3)]),
        "what a recording holds is counted by the name a reader greps for"
    );
    assert_eq!(
        commands(&events),
        BTreeMap::from([("cargo".to_owned(), 2), ("rustc".to_owned(), 1)]),
        "and a program is one program however it was reached: counting the path would \
         make one cargo two"
    );
    assert_eq!(
        commands(&[ran(1, &[], 1)]),
        BTreeMap::from([("?".to_owned(), 1)]),
        "while a command with no program at all is counted under a name that says so: an \
         empty one puts a blank row in the table, which reads as a program whose name the \
         run lost rather than an execution nobody gave one"
    );
    assert_eq!(
        phases(&events),
        BTreeMap::from([("baseline".to_owned(), 140), ("mutation".to_owned(), 9)]),
        "a phase that ran twice is that phase for as long as both took, which is why a \
         stage and the work inside it may not share a name"
    );

    let none = ended(7, "silent", 0);
    assert_eq!(
        phases(&[Event {
            payload: Payload::PhaseEnd {
                phase: PhaseRecord {
                    name: "silent".to_owned(),
                    duration_ms: None,
                },
            },
            ..none
        }]),
        BTreeMap::from([("silent".to_owned(), 0)]),
        "and a phase whose end carried no duration is named with nothing rather than \
         left out: a phase missing from the table reads as a phase that never ran"
    );
}

#[test]
fn a_difference_names_every_side_and_says_which_way_it_went() {
    let before = BTreeMap::from([("a".to_owned(), 1), ("gone".to_owned(), 3)]);
    let after = BTreeMap::from([("a".to_owned(), 4), ("new".to_owned(), 2)]);
    assert_eq!(
        keys(&before, &after),
        vec!["a".to_owned(), "gone".to_owned(), "new".to_owned()],
        "a diff is about both recordings, so what only one of them holds is still a row: \
         a phase that stopped happening is the answer as often as one that got slower"
    );
    assert_eq!(
        (
            delta(1, 4).to_string(),
            delta(3, 0).to_string(),
            delta(2, 2).to_string(),
        ),
        ("+3".to_owned(), "-3".to_owned(), "+0".to_owned()),
        "and the number a reader is looking at is signed, because slower and faster are \
         not the same news"
    );
    assert_eq!(
        delta(u64::MAX, 0).to_string(),
        format!("-{}", u64::MAX),
        "the full wire range remains exact rather than wrapping or clamping"
    );
}

#[test]
fn every_problem_a_recording_can_have_is_said_in_a_line_that_names_it() {
    let problems = [
        Problem::MissingRunStart,
        Problem::MissingRunEnd,
        Problem::SequenceGap {
            expected: 7,
            found: 9,
        },
        Problem::Dropped(3),
        Problem::PhaseRepeated {
            name: "equivalence".to_owned(),
            times: 2,
        },
    ];
    for problem in &problems {
        match problem {
            Problem::MissingRunStart
            | Problem::MissingRunEnd
            | Problem::SequenceGap { .. }
            | Problem::Dropped(..)
            | Problem::PhaseRepeated { .. } => {}
        }
        let line = describe(problem);
        assert!(
            !line.is_empty() && !line.contains('{') && line.len() > 20,
            "a problem a reader cannot read is one they cannot act on: {problem:?} said \
             {line:?}"
        );
    }
    assert!(
        describe(&problems[2]).contains('7') && describe(&problems[2]).contains('9'),
        "a gap says which number was expected and which arrived: {}",
        describe(&problems[2])
    );
    assert!(
        describe(&problems[4]).contains("equivalence"),
        "and a phase that opened twice is named, because a reader summing the table has \
         to know which row is the sum of two: {}",
        describe(&problems[4])
    );
}

/// A recording of `run` under `root`, written where the command looks for one.
fn recorded(root: &std::path::Path, run: &str, events: &[Event]) {
    let directory = root.join(".njutest/trace").join(run);
    std::fs::create_dir_all(&directory).expect("a directory for the recording");
    let mut stream = String::new();
    for event in events {
        stream.push_str(&serde_json::to_string(event).expect("an event is a document"));
        stream.push('\n');
    }
    std::fs::write(directory.join(njutest::trace::FILE_NAME), stream).expect("the recording");
}

/// A whole recording, from a run-start to a run-end, with something in between.
fn whole(run: &str) -> Vec<Event> {
    vec![
        at(
            1,
            Payload::RunStart {
                start: njutest::trace::StartRecord::of(
                    run,
                    njutest::report::RunKind::Full,
                    njutest::config::Contract::StandardV1,
                ),
            },
        ),
        ended(2, "baseline", 120),
        ran(3, &["/usr/bin/cargo", "test"], 90),
        discharging(4, &["never-infected"]),
        at(
            5,
            Payload::RunEnd {
                run: njutest::trace::RunRecord {
                    verdict: njutest::report::Verdict::Assured,
                    accounting: None,
                    error: None,
                    events_emitted: 4,
                    events_dropped: 0,
                },
            },
        ),
    ]
}

/// What the command says, driven in this process rather than in one of its own.
fn asked(root: &std::path::Path, args: &[&str]) -> (u8, String, String) {
    let scratch = root.join("scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    let environment = njutest::cli::Environment {
        cache_directory: root.join("cache"),
        working_directory: root.to_path_buf(),
        temp_directory: scratch,
        program: std::path::PathBuf::from("this test never runs it"),
        vars: Vec::new(),
        cancel: rust_mutants::runner::Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut said, mut complaints) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(std::ffi::OsString::from),
        &environment,
        &mut said,
        &mut complaints,
    );
    (
        code,
        njutest_devkit::process::strict_utf8(&said).into_owned(),
        njutest_devkit::process::strict_utf8(&complaints).into_owned(),
    )
}

#[test]
fn a_summary_says_what_the_recording_holds_and_whether_it_holds_together() {
    let dir = tempfile::tempdir().expect("tempdir");
    recorded(
        dir.path(),
        "20270115T080000Z-aaaaaa",
        &whole("20270115T080000Z-aaaaaa"),
    );
    let (code, said, complained) =
        asked(dir.path(), &["trace", "summary", "20270115T080000Z-aaaaaa"]);
    assert_eq!(
        code, 0,
        "a recording with nothing wrong with it: {complained}"
    );
    for line in [
        "RUN\t20270115T080000Z-aaaaaa",
        "EVENTS\t5",
        "TYPE\texec\t1",
        "PHASE\tbaseline\t120ms",
        "COMMAND\tcargo\t1",
        "PROOF\tnever-infected\t1",
        "SLOWEST\t90ms\tcargo test",
        "PROBLEMS\tno problems",
    ] {
        assert!(
            said.contains(line),
            "a summary is what somebody reads instead of the recording, so every count \
             it took is on it: {line:?} is not in\n{said}"
        );
    }

    let mut broken = whole("20270115T080000Z-bbbbbb");
    let lost = broken.remove(1);
    assert_eq!(
        lost.payload.type_name(),
        "phase-end",
        "the removed phase end is the deliberate hole in this recording"
    );
    recorded(dir.path(), "20270115T080000Z-bbbbbb", &broken);
    let (code, said, complained) =
        asked(dir.path(), &["trace", "summary", "20270115T080000Z-bbbbbb"]);
    assert_eq!(
        code, 2,
        "while a recording with a hole in it is one nothing was established from, and \
         the code says so rather than leaving it to whoever reads the lines: {said}"
    );
    assert!(said.contains("PROBLEM\t"), "{said}");
    assert!(
        complained.is_empty(),
        "a readable recording is not a refusal"
    );
}

#[test]
fn a_recording_that_is_not_there_is_named_rather_than_answered_about() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (code, said, complained) =
        asked(dir.path(), &["trace", "summary", "20200101T000000Z-000000"]);
    assert_eq!(code, 3, "{complained}");
    assert!(said.is_empty(), "a missing recording has no summary");
    assert!(
        complained.contains("20200101T000000Z-000000") && complained.contains(".njutest/trace"),
        "a run nobody recorded is named with the path that would have held it, because \
         the answer is usually that the run was somewhere else: {complained}"
    );

    let (code, said, complained) = asked(dir.path(), &["trace", "summary"]);
    assert_eq!(
        code, 3,
        "and a directory where no run has finished has no latest run to summarise \
         either: {complained}"
    );
    assert!(said.is_empty(), "an absent latest run has no summary");
    assert!(
        complained.contains("no run has completed here yet"),
        "which it says, rather than failing without a word: a command that exits 3 in \
         silence is one a person runs again to see what happened: {complained:?}"
    );
}

#[test]
fn a_difference_says_what_moved_and_leaves_out_what_did_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    recorded(
        dir.path(),
        "20270115T080000Z-aaaaaa",
        &whole("20270115T080000Z-aaaaaa"),
    );
    let mut slower = whole("20270115T080000Z-bbbbbb");
    slower[1] = ended(2, "baseline", 300);
    slower.push(ran(6, &["/usr/bin/rustc", "--version"], 4));
    recorded(dir.path(), "20270115T080000Z-bbbbbb", &slower);

    let (code, said, complained) = asked(
        dir.path(),
        &[
            "trace",
            "diff",
            "20270115T080000Z-aaaaaa",
            "20270115T080000Z-bbbbbb",
        ],
    );
    assert_eq!(code, 0, "{complained}");
    assert!(
        said.contains("A\t20270115T080000Z-aaaaaa\t5 events")
            && said.contains("B\t20270115T080000Z-bbbbbb\t6 events"),
        "a difference names both recordings and how much each holds, or a reader has two \
         columns of numbers and no way to tell which run is which: {said}"
    );
    assert!(
        said.contains("PHASE\tbaseline\t120ms\t300ms\t+180ms"),
        "a phase that got slower says by how much and which way, because that is the \
         whole question: {said}"
    );
    assert!(
        said.contains("TYPE\texec\t1\t2"),
        "and a type there is more of says both numbers: {said}"
    );
    assert!(
        !said.contains("TYPE\trun-start"),
        "while what did not move is left out: a diff that lists everything is the two \
         recordings again: {said}"
    );
}

#[test]
fn a_summary_carries_what_the_engine_recorded_beside_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let run = "20270115T080000Z-cccccc";
    recorded(dir.path(), run, &whole(run));

    let beside = dir
        .path()
        .join(".njutest/trace")
        .join(run)
        .join(njutest::app::verify::BUILDS_DIRECTORY)
        .join("0000000000")
        .join(njutest::app::trace::ENGINE_DIRECTORY);
    std::fs::create_dir_all(&beside).expect("a directory for the engine's own");
    let engine = rust_mutants::testkit::trace::memory_recorder();
    engine.note("snapshot", "one");
    engine.note("snapshot", "two");
    let mut stream = String::new();
    for event in engine.events() {
        stream.push_str(&serde_json::to_string(&event).expect("an event is a document"));
        stream.push('\n');
    }
    std::fs::write(beside.join(rust_mutants::trace::FILE_NAME), &stream)
        .expect("the engine's recording");

    let (code, said, complained) = asked(dir.path(), &["trace", "summary", run]);
    assert_eq!(code, 0, "the combined recording is complete");
    assert!(
        said.lines().any(|line| line.starts_with("ENGINE\t")),
        "the engine does most of a run — the snapshot, the instrumentation, the \
         validation rounds, the builds — and keeps its own recording beside this one. A \
         summary that read only the runner's leaves the larger part of every run \
         unaccounted for, and a person looking for the minutes finds them nowhere: \
         {said}{complained}"
    );

    std::fs::write(
        beside.join(rust_mutants::trace::FILE_NAME),
        "not a recording\n",
    )
    .expect("a recording that is not one");
    let (code, said, complained) = asked(dir.path(), &["trace", "summary", run]);
    assert_eq!(
        code, 0,
        "an unreadable optional engine trace is reported in the summary"
    );
    assert!(
        complained.is_empty(),
        "the runner recording itself remains readable"
    );
    assert!(
        said.lines()
            .any(|line| { line.starts_with("ENGINE\t") && line.contains("\tunreadable\t") }),
        "and one it cannot read is said to be unreadable rather than passed over: a \
         summary with no ENGINE lines reads as a run the engine did not record, which \
         is a different thing to go and look for: {said}"
    );
}

#[test]
fn a_recording_that_cannot_be_read_is_a_refusal_and_never_an_empty_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let run = "20270115T080000Z-dddddd";
    let directory = dir.path().join(".njutest/trace").join(run);
    std::fs::create_dir_all(&directory).expect("a directory for the recording");
    std::fs::write(
        directory.join(njutest::trace::FILE_NAME),
        "{\"seq\":1,\"type\":\"nothing-of-the-sort\"}\n",
    )
    .expect("a recording that is not one");

    let (code, said, complained) = asked(dir.path(), &["trace", "summary", run]);
    assert_eq!(
        code, 3,
        "a recording this release cannot read is not a run that recorded nothing: \
         summarising it as an empty one hands a person a table of zeroes and no reason \
         to doubt it: {said}"
    );
    assert!(
        complained.contains("NJ6005"),
        "and says so with the code a person greps for: {complained:?}"
    );
}

#[test]
fn a_difference_against_a_recording_that_is_not_there_establishes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let run = "20270115T080000Z-eeeeee";
    recorded(dir.path(), run, &whole(run));

    let (code, said, complained) = asked(
        dir.path(),
        &["trace", "diff", run, "20200101T000000Z-000000"],
    );
    assert_eq!(
        code, 3,
        "a difference is about two recordings, so one of them missing is nothing to \
         answer with: reporting the half that is there as the difference would say every \
         phase of it appeared out of nowhere: {said}{complained}"
    );
    assert!(
        said.is_empty(),
        "and it says nothing at all rather than the beginning of a table: {said:?}"
    );
}

#[test]
fn the_slowest_names_what_a_run_actually_spends_its_time_on() {
    let events = vec![
        ran(1, &["cargo", "build"], 900),
        at(
            2,
            Payload::MutantExec {
                mutant: njutest::trace::MutantExecRecord {
                    mutant: "src/lib.rs:sign:gt-to-ge@8".to_owned(),
                    target: "fixture/test/smoke".to_owned(),
                    args: vec!["--exact".to_owned()],
                    outcome: "survived".to_owned(),
                    duration_ms: 12_000,
                    alone: false,
                    step_boundary: None,
                },
            },
        ),
    ];
    let named = slowest(&events);
    assert_eq!(
        named.first().map(|(duration, _)| *duration),
        Some(12_000),
        "a run spends most of itself putting mutations to targets, and a summary that \
         names `cargo build` as its slowest command while the twelve-second measurement \
         beside it is not in the list is a summary that reports the wrong thing to \
         somebody trying to make their run shorter: {named:?}"
    );
    assert!(
        named
            .iter()
            .any(|(_, command)| command.contains("src/lib.rs:sign:gt-to-ge@8")),
        "and it names the measurement by the mutation and target it was, because that is \
         what a reader would go and narrow: {named:?}"
    );
}
