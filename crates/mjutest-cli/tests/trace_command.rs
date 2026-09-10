// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `mjutest trace` reads out of a recording: which proof made a run shorter, which command made it long, and what moved between two runs.
//!
//! Every one of these is a pure function over events, and each is reached
//! here directly. Driving the command instead means starting a process, and a
//! measurement of what a run's own tests reach does not follow a guard across
//! that boundary: the rules below were 98 mutations nothing was ever routed to.

use std::collections::BTreeMap;

use mjutest_cli::app::trace::{
    COMMAND_WIDTH, SLOWEST_KEPT, commands, counts, delta, describe, keys, phases, proofs, said,
    slowest,
};
use mjutest_cli::trace::{
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
                granularity: "block".to_owned(),
                discharged: proofs
                    .iter()
                    .map(|proof| DischargeRecord {
                        target: format!("t{proof}"),
                        proof: (*proof).to_owned(),
                    })
                    .collect(),
                ..RouteRecord::default()
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
                duration_ms,
                ..ExecRecord::default()
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
    assert!(
        !exact.ends_with('…'),
        "while one that fits is not marked as cut, or every line carries a mark and none \
         of them means anything: {exact:?}"
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
        (delta(1, 4), delta(3, 0), delta(2, 2)),
        (3, -3, 0),
        "and the number a reader is looking at is signed, because slower and faster are \
         not the same news"
    );
    assert_eq!(
        delta(u64::MAX, 0),
        i64::MIN.saturating_add(1),
        "a difference too large to be one is the largest there is rather than a number \
         that wrapped round to the wrong sign"
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
