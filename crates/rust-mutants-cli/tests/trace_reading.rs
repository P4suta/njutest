// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a recording back: which one is read, and what is said about one that is not whole.
//!
//! A recording is what a person goes to when a run did something they did not
//! expect, so the two things that must never happen are reading a different
//! run's and being told a broken one is fine. Every recording here is written
//! by hand, because what is being put to the test is the reading.

#![expect(
    clippy::expect_used,
    reason = "the helpers that lay out a directory of recordings are not themselves tests: one \
              that could not be written leaves nothing to read"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

/// What one command said, driven in this process.
struct Said {
    code: u8,
    out: String,
    err: String,
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: mjutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

fn asked(fixture: &Fixture, args: &[&str]) -> Said {
    let root = fixture.root().to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", root.as_str()])
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    Said {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// One event of a recording, as it goes on the wire.
fn event(seq: u64, elapsed: u64, rest: &serde_json::Value) -> String {
    let mut one = serde_json::json!({
        "seq": seq,
        "timestamp": "2026-01-01T00:00:00Z",
        "elapsed_ms": elapsed,
    });
    if let (Some(into), Some(from)) = (one.as_object_mut(), rest.as_object()) {
        for (name, value) in from {
            let _replaced = into.insert(name.clone(), value.clone());
        }
    }
    serde_json::to_string(&one).expect("an event is a document")
}

/// A recording that begins, ends, and loses nothing, with one phase that took `phase_ms`.
fn whole(phase_ms: u64, dropped: u64) -> Vec<String> {
    vec![
        event(
            1,
            0,
            &serde_json::json!({"type": "run-start", "schema": "rust-mutants-trace-v1", "engine": "0.1.0"}),
        ),
        event(
            2,
            0,
            &serde_json::json!({"type": "phase-start", "phase": {"name": "discover"}}),
        ),
        event(
            3,
            phase_ms,
            &serde_json::json!({
                "type": "phase-end",
                "phase": {"name": "discover", "duration_ms": phase_ms},
            }),
        ),
        event(
            4,
            phase_ms,
            &serde_json::json!({
                "type": "run-end",
                "run": {
                    "outcome": "completed",
                    "events_emitted": 3,
                    "events_dropped": dropped,
                },
            }),
        ),
    ]
}

/// Writes `lines` as the recording of `name` beside its run, and answers where it went.
fn recorded(fixture: &Fixture, name: &str, lines: &[String]) -> PathBuf {
    let directory = fixture
        .root()
        .join(rust_mutants_cli::config::DEFAULT_REPORTS_DIRECTORY)
        .join(name)
        .join("trace");
    write_at(&directory, lines);
    directory
}

/// The same, under `traces/`, which is where a command that wrote no report keeps one.
fn beside(fixture: &Fixture, name: &str, lines: &[String]) -> PathBuf {
    let directory = fixture
        .root()
        .join(rust_mutants_cli::config::DEFAULT_REPORTS_DIRECTORY)
        .join("traces")
        .join(name);
    write_at(&directory, lines);
    directory
}

fn write_at(directory: &Path, lines: &[String]) {
    std::fs::create_dir_all(directory).expect("a directory to record in");
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(directory.join("trace.jsonl"), text).expect("the recording");
}

#[test]
fn a_recording_that_begins_ends_and_loses_nothing_is_said_to_be_whole() {
    let fixture = Fixture::copy("fixture-simple");
    let _at = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));

    let checked = asked(&fixture, &["trace", "check"]);
    assert_eq!(checked.code, 0, "{}{}", checked.out, checked.err);
    assert!(
        checked.out.contains("COMPLETE") && checked.out.contains("lost nothing"),
        "a reader going to a recording after a surprise needs to know first whether it \
         is all there: {}",
        checked.out
    );
    assert!(
        checked.out.contains("20260101T000000000Z"),
        "and which run it is, because reading another run's is the one mistake that \
         costs the whole investigation: {}",
        checked.out
    );
}

#[test]
fn each_way_a_recording_can_be_broken_is_said_in_its_own_words() {
    let cases: [(&str, Vec<String>, &str); 4] = [
        (
            "a beginning that was lost",
            whole(5, 0).split_off(1),
            "does not begin with run-start",
        ),
        (
            "an end that was lost",
            {
                let mut events = whole(5, 0);
                let _end = events.pop();
                events
            },
            "does not end with run-end",
        ),
        (
            "events the sink lost between two it kept",
            {
                let mut events = whole(5, 0);
                let _second = events.remove(1);
                events
            },
            "is missing",
        ),
        (
            "events the run admits it dropped",
            whole(5, 7),
            "dropped 7 events",
        ),
    ];

    for (what, events, said) in cases {
        let fixture = Fixture::copy("fixture-simple");
        let _at = recorded(&fixture, "20260101T000000000Z", &events);
        let checked = asked(&fixture, &["trace", "check"]);
        assert_eq!(
            checked.code, 1,
            "{what} is a recording a reader must not act on as if it were whole: {}{}",
            checked.out, checked.err
        );
        assert!(
            checked.out.contains("PROBLEM") && checked.out.contains(said),
            "and it is said in the words that name what went wrong rather than as one \
             word for every kind: {what} answered {}",
            checked.out
        );
    }
}

#[test]
fn a_phase_that_began_and_did_not_end_is_named() {
    let fixture = Fixture::copy("fixture-simple");
    let mut events = whole(5, 0);
    let _ended = events.remove(2);
    let _at = recorded(&fixture, "20260101T000000000Z", &events);

    let checked = asked(&fixture, &["trace", "check"]);
    assert_eq!(checked.code, 1, "{}{}", checked.out, checked.err);
    assert!(
        checked.out.contains("discover") && checked.out.contains("different number"),
        "a phase left open is where the run was killed, and naming it is the answer to \
         the question the reader came with: {}",
        checked.out
    );
}

#[test]
fn the_recording_read_with_nothing_named_is_the_newest_one() {
    let fixture = Fixture::copy("fixture-simple");
    let _older = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));
    let _newer = recorded(&fixture, "20260102T000000000Z", &whole(9, 0));

    let summarised = asked(&fixture, &["trace", "summary"]);
    assert_eq!(summarised.code, 0, "{}{}", summarised.out, summarised.err);
    assert!(
        summarised.out.contains("20260102T000000000Z"),
        "the names of runs sort the way they ran, and a reader who named none meant the \
         one that just happened: {}",
        summarised.out
    );
}

#[test]
fn a_run_named_is_the_one_read_and_a_prefix_names_it_too() {
    let fixture = Fixture::copy("fixture-simple");
    let _older = recorded(&fixture, "20260101T000000000Z-aaaaaa", &whole(5, 0));
    let _newer = recorded(&fixture, "20260102T000000000Z-bbbbbb", &whole(9, 0));

    let exact = asked(
        &fixture,
        &["trace", "summary", "--run", "20260101T000000000Z-aaaaaa"],
    );
    assert!(
        exact.code == 0 && exact.out.contains("aaaaaa"),
        "a run named is the run read: {}{}",
        exact.out,
        exact.err
    );

    let by_prefix = asked(
        &fixture,
        &["trace", "summary", "--run", "20260101T000000000Z"],
    );
    assert!(
        by_prefix.code == 0 && by_prefix.out.contains("aaaaaa"),
        "and the instant names it without the digits nobody types, because that is how \
         a person reads it off a report: {}{}",
        by_prefix.out,
        by_prefix.err
    );
}

#[test]
fn a_recording_in_a_directory_given_outright_is_read_without_a_report_directory_at_all() {
    let fixture = Fixture::copy("fixture-simple");
    let elsewhere = fixture.temp().join("somebody-sent-me-this");
    write_at(&elsewhere, &whole(5, 0));

    let checked = asked(
        &fixture,
        &["trace", "check", "--dir", &elsewhere.to_string_lossy()],
    );
    assert_eq!(checked.code, 0, "{}{}", checked.out, checked.err);
    assert!(
        checked.out.contains(&elsewhere.display().to_string()),
        "a recording out of a bug report is named by where it is, since it has no run \
         under this tree to be named after: {}",
        checked.out
    );
}

#[test]
fn a_recording_that_is_not_there_is_named_rather_than_read_as_an_empty_one() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = asked(&fixture, &["trace", "summary"]);
    assert_ne!(
        empty.code, 0,
        "a table of zeroes reads as a run that did nothing, which is a different thing \
         to go and look for: {}{}",
        empty.out, empty.err
    );
    assert!(
        empty.err.contains("no recording is stored"),
        "and the answer says there is none rather than which one: {}",
        empty.err
    );

    let _at = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));
    let named = asked(&fixture, &["trace", "summary", "--run", "20260305"]);
    assert_ne!(named.code, 0, "{}{}", named.out, named.err);
    assert!(
        named.err.contains("20260305") && named.err.contains("no recording named"),
        "and a run that was named says which name found nothing, because the usual \
         cause is a name from another tree: {}",
        named.err
    );
}

#[test]
fn a_recording_nobody_can_read_is_refused_by_naming_the_file() {
    let fixture = Fixture::copy("fixture-simple");
    let at = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));
    std::fs::write(at.join("trace.jsonl"), b"{not a recording\n").expect("a broken recording");

    let checked = asked(&fixture, &["trace", "check"]);
    assert_ne!(
        checked.code, 0,
        "a recording that will not parse is not a recording with no problems in it: {}{}",
        checked.out, checked.err
    );
    assert!(
        checked.err.contains("trace.jsonl"),
        "and the file is named, because a reader has to know which one to go and look \
         at: {}",
        checked.err
    );
}

#[test]
fn a_directory_beside_the_runs_with_no_recording_in_it_is_not_one() {
    let fixture = Fixture::copy("fixture-simple");
    let reports = fixture
        .root()
        .join(rust_mutants_cli::config::DEFAULT_REPORTS_DIRECTORY);
    std::fs::create_dir_all(reports.join("20260109T000000000Z").join("trace"))
        .expect("a run whose recording was removed");
    let _at = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));

    let summarised = asked(&fixture, &["trace", "summary"]);
    assert!(
        summarised.code == 0 && summarised.out.contains("20260101T000000000Z"),
        "a directory with no recording in it is not the newest recording, and reading it \
         as one would refuse where a recording is there to read: {}{}",
        summarised.out,
        summarised.err
    );
}

#[test]
fn a_command_that_wrote_no_report_keeps_its_recording_where_a_reader_finds_it() {
    let fixture = Fixture::copy("fixture-simple");
    let _run = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));
    let _other = beside(&fixture, "20260103T000000000Z", &whole(7, 0));

    let summarised = asked(&fixture, &["trace", "summary"]);
    assert!(
        summarised.code == 0 && summarised.out.contains("20260103T000000000Z"),
        "a `list` or a `catalog` asked to record leaves no report to keep its recording \
         beside, and a reader looking for what it did finds it among the rest: {}{}",
        summarised.out,
        summarised.err
    );
}

#[test]
fn what_moved_between_two_recordings_is_said_column_by_column() {
    let fixture = Fixture::copy("fixture-simple");
    let _before = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));
    let mut busier = whole(5, 0);
    let last = busier.pop().expect("the run ends");
    busier.push(event(
        4,
        6,
        &serde_json::json!({"type": "phase-start", "phase": {"name": "validate"}}),
    ));
    busier.push(event(
        5,
        9,
        &serde_json::json!({
            "type": "phase-end",
            "phase": {"name": "validate", "duration_ms": 3},
        }),
    ));
    busier.push(last.replace("\"seq\":4", "\"seq\":6"));
    let _after = recorded(&fixture, "20260102T000000000Z", &busier);

    let moved = asked(
        &fixture,
        &[
            "trace",
            "diff",
            "20260101T000000000Z",
            "20260102T000000000Z",
        ],
    );
    assert_eq!(moved.code, 0, "{}{}", moved.out, moved.err);
    assert!(
        moved.out.contains("A\t20260101T000000000Z")
            && moved.out.contains("B\t20260102T000000000Z"),
        "which two recordings are being compared is the first thing said, or a reader \
         cannot tell which way the numbers moved: {}",
        moved.out
    );
    assert!(
        moved.out.contains("CHANGED\tevents\t4\t6"),
        "and what moved is said with both numbers, because a column that only says it \
         changed leaves the reader to go and read both recordings anyway: {}",
        moved.out
    );
    assert!(
        moved.out.contains("phase-start\t1\t2"),
        "column by column, so a run that did one more of something says which: {}",
        moved.out
    );

    let unchanged = asked(
        &fixture,
        &[
            "trace",
            "diff",
            "20260101T000000000Z",
            "20260101T000000000Z",
        ],
    );
    assert!(
        unchanged.code == 0 && !unchanged.out.contains("CHANGED"),
        "and a recording against itself moved nothing: {}{}",
        unchanged.out,
        unchanged.err
    );
}

#[test]
fn a_diff_against_a_recording_that_is_not_there_names_it_rather_than_showing_nothing() {
    let fixture = Fixture::copy("fixture-simple");
    let _before = recorded(&fixture, "20260101T000000000Z", &whole(5, 0));

    let moved = asked(
        &fixture,
        &["trace", "diff", "20260101T000000000Z", "20261231"],
    );
    assert_ne!(
        moved.code, 0,
        "nothing changed and one of them is missing are the same output and different \
         facts: {}{}",
        moved.out, moved.err
    );
    assert!(
        moved.err.contains("20261231"),
        "so the one that is not there is named: {}",
        moved.err
    );
}
