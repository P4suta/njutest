// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a run's report is stored, which of them are kept, and which one is the newest.

#![expect(
    clippy::expect_used,
    reason = "the helpers that arrange a directory of runs are not themselves tests: a \
              directory that could not be made leaves nothing to prune"
)]

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants_cli::app::stored;
use rust_mutants_cli::cli;
use rust_mutants_cli::report::run as run_report;

/// A directory a test arranges runs under.
fn scratch(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("rust-mutants-stored-{}-{name}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).expect("a directory to arrange runs under");
    root
}

/// A stored run of that name, as far as anything reading the directory can tell.
fn run_at(directory: &Path, id: &str) -> PathBuf {
    let path = directory.join(id);
    std::fs::create_dir_all(&path).expect("a run directory");
    std::fs::write(path.join(run_report::FILE_NAME), b"{}").expect("a run report");
    path
}

/// What `--run-id` was given, as the command line hands it over.
fn run_named(id: Option<&str>) -> cli::Command {
    let mut parsed = cli::parse(
        ["rust-mutants", "run"]
            .into_iter()
            .map(std::ffi::OsString::from),
    )
    .expect("`run` with nothing else on it parses");
    if let cli::Command::Run { run_id, .. } = &mut parsed.command {
        *run_id = id.map(ToOwned::to_owned);
    }
    parsed.command
}

#[test]
fn the_name_a_run_goes_by_when_nobody_named_it_sorts_the_runs_in_the_order_they_ran() {
    let earlier = stored::run_id(
        "2026-09-05T12:00:00Z"
            .parse::<Timestamp>()
            .expect("an instant"),
    );
    let later = stored::run_id(
        "2026-09-05T12:00:01Z"
            .parse::<Timestamp>()
            .expect("an instant a second after it"),
    );
    assert!(
        earlier < later,
        "a directory listing is how the runs are read back, so the name has to sort the \
         way they ran: {earlier} then {later}"
    );
    assert!(
        earlier.chars().all(|one| one.is_ascii_alphanumeric()),
        "and it is a name every filesystem takes: {earlier}"
    );
    assert!(
        earlier.starts_with("20260905T120000"),
        "and a person reading it knows when it ran without opening it: {earlier}"
    );
    assert_ne!(
        stored::run_id(
            "2026-09-05T12:00:00.001Z"
                .parse::<Timestamp>()
                .expect("an instant a millisecond after it")
        ),
        earlier,
        "two runs a millisecond apart are two runs, and one name would have the second \
         write over the first"
    );
}

#[test]
fn a_name_a_directory_cannot_be_is_refused_rather_than_written_somewhere_else() {
    for wrong in [
        "",
        ".",
        "..",
        "a/b",
        "a b",
        "a\\b",
        "träge",
        "a:b",
        &"x".repeat(65),
    ] {
        let refused = stored::named(&run_named(Some(wrong)), Timestamp::UNIX_EPOCH);
        assert!(
            refused.is_err(),
            "{wrong:?} is not a name a run's directory can be, and taking it would write \
             the report somewhere the report directory does not hold: {refused:?}"
        );
    }
    assert!(
        stored::named(&run_named(Some("..")), Timestamp::UNIX_EPOCH).is_err(),
        "the one that matters most: a run named `..` stores its report over the \
         directory that holds every other run"
    );
}

#[test]
fn a_name_a_person_chose_is_the_one_the_run_goes_by() {
    for right in [
        "nightly",
        "pr-1234",
        "v0.1.0",
        "a_b-c.d",
        "1",
        &"x".repeat(64),
    ] {
        assert_eq!(
            stored::named(&run_named(Some(right)), Timestamp::UNIX_EPOCH)
                .as_deref()
                .ok(),
            Some(right),
            "{right:?} is a name a directory can be, and a run asked for it: comparing two \
             runs is done by the names somebody gave them"
        );
    }
    let now = "2026-09-05T12:00:00Z".parse().expect("an instant");
    assert_eq!(
        stored::named(&run_named(None), now).ok(),
        Some(stored::run_id(now)),
        "and a run nobody named goes by the instant it started"
    );
}

#[test]
fn the_newest_run_is_the_one_the_last_run_pointed_at() {
    let directory = scratch("pointer");
    let _older = run_at(&directory, "20260101T000000000Z");
    let newer = run_at(&directory, "20260102T000000000Z");
    let by_name = stored::newest(&directory).expect("a run is stored");
    assert_eq!(
        by_name,
        newer.join(run_report::FILE_NAME),
        "with no pointer to read, the names are the order they ran in"
    );

    std::fs::write(
        directory.join(run_report::LATEST_FILE_NAME),
        serde_json::to_string(&serde_json::json!({
            "document_type": "rust-mutants/latest-run",
            "schema_version": 1,
            "run": "20260101T000000000Z",
            "document": format!("20260101T000000000Z/{}", run_report::FILE_NAME),
        }))
        .expect("a pointer"),
    )
    .expect("a pointer the last run wrote");

    assert_eq!(
        stored::newest(&directory).expect("a run is stored"),
        directory
            .join("20260101T000000000Z")
            .join(run_report::FILE_NAME),
        "and the pointer wins over the names, because a run given a name of its own \
         does not sort where it ran"
    );
}

#[test]
fn a_pointer_at_a_run_nobody_kept_falls_back_to_the_names_rather_than_refusing() {
    let directory = scratch("dangling");
    let newer = run_at(&directory, "20260102T000000000Z");
    std::fs::write(
        directory.join(run_report::LATEST_FILE_NAME),
        serde_json::to_string(&serde_json::json!({
            "document_type": "rust-mutants/latest-run",
            "schema_version": 1,
            "run": "removed",
            "document": format!("removed/{}", run_report::FILE_NAME),
        }))
        .expect("a pointer"),
    )
    .expect("a pointer at a run somebody removed");

    assert_eq!(
        stored::newest(&directory).expect("a run is still stored"),
        newer.join(run_report::FILE_NAME),
        "a pointer at a run that is gone is a pointer, not an answer: `--gc --all` \
         removes runs and leaves the pointer, and refusing there would mean a person \
         cannot read the reports they still have"
    );
}

#[test]
fn a_pointer_nobody_can_parse_is_the_same_as_no_pointer() {
    let directory = scratch("unreadable-pointer");
    let only = run_at(&directory, "20260102T000000000Z");
    for text in ["", "not json at all", "{}", r#"{"document": 7}"#] {
        std::fs::write(
            directory.join(run_report::LATEST_FILE_NAME),
            text.as_bytes(),
        )
        .expect("a pointer nobody can read");
        assert_eq!(
            stored::newest(&directory).expect("a run is stored"),
            only.join(run_report::FILE_NAME),
            "a pointer from another release, or one an interrupted run left half \
             written, does not cost a person the reports they have"
        );
    }
}

#[test]
fn a_directory_with_no_run_in_it_is_named_rather_than_answered_about() {
    let directory = scratch("empty");
    let refused = stored::newest(&directory).expect_err("nothing is stored here");
    assert!(
        refused
            .to_string()
            .contains(&directory.display().to_string()),
        "the refusal names the directory it looked in, because the usual cause is that \
         the run wrote its reports somewhere else: {refused}"
    );
    assert!(
        stored::newest(&directory.join("was-never-made")).is_err(),
        "and a directory that is not there at all is the same answer"
    );
}

#[test]
fn only_the_newest_runs_are_kept_and_a_recording_never_costs_a_run_its_place() {
    let directory = scratch("prune");
    for day in 1..=5u32 {
        run_at(&directory, &format!("2026010{day}T000000000Z"));
    }
    let recording = directory.join("20260106T000000000Z");
    std::fs::create_dir_all(recording.join("run")).expect("a run that recorded and did not report");
    let traces = directory.join("traces");
    std::fs::create_dir_all(traces.join("20260101T000000000Z")).expect("a recording of a command");

    stored::prune(&directory, 2);

    let left: Vec<String> = stored::subdirectories(&directory)
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    assert!(
        left.contains(&"20260104T000000000Z".to_owned())
            && left.contains(&"20260105T000000000Z".to_owned()),
        "the newest two runs are the ones kept: {left:?}"
    );
    assert!(
        !left.contains(&"20260101T000000000Z".to_owned())
            && !left.contains(&"20260103T000000000Z".to_owned()),
        "and the older ones are gone, because the disk is the reason there is a limit \
         at all: {left:?}"
    );
    assert!(
        left.contains(&"20260106T000000000Z".to_owned()),
        "a run that recorded and wrote no report is bounded by a count of its own \
         rather than against the stored runs: {left:?}"
    );
    assert!(
        left.contains(&"traces".to_owned()),
        "and `traces` sorts after every run name, so counting it as one would leave \
         `keep - 1` runs stored: {left:?}"
    );
}

#[test]
fn keeping_none_keeps_everything_rather_than_removing_everything() {
    let directory = scratch("keep-zero");
    for day in 1..=3u32 {
        run_at(&directory, &format!("2026010{day}T000000000Z"));
    }
    stored::prune(&directory, 0);
    assert_eq!(
        stored::subdirectories(&directory).len(),
        3,
        "`keep = 0` is how a person says to keep every run, and reading it as a count \
         would delete the run that had just finished"
    );
}

#[test]
fn what_a_run_left_behind_is_a_directory_and_never_a_file_beside_them() {
    let directory = scratch("files");
    run_at(&directory, "20260101T000000000Z");
    std::fs::write(directory.join("latest.json"), b"{}").expect("the pointer");
    std::fs::write(directory.join("notes.md"), b"mine").expect("a file somebody put here");

    let found = stored::subdirectories(&directory);
    assert_eq!(
        found.len(),
        1,
        "a file beside the runs is not a run, and removing one to keep a count would \
         remove somebody's notes: {found:?}"
    );

    let (runs, recordings) = stored::kept(&directory);
    assert_eq!(runs.len(), 1, "and a run is one with a report in it");
    assert!(
        recordings.is_empty(),
        "and a recording is one without: {recordings:?}"
    );

    stored::prune(&directory, 1);
    assert!(
        directory.join("notes.md").is_file() && directory.join("latest.json").is_file(),
        "which is what keeps them there"
    );
}

#[test]
fn removing_all_but_the_newest_removes_the_oldest_and_keeps_the_order_they_are_in() {
    let directory = scratch("oldest");
    let all: Vec<PathBuf> = (1..=4u32)
        .map(|day| run_at(&directory, &format!("2026010{day}T000000000Z")))
        .collect();

    stored::oldest(&all, 4);
    assert_eq!(
        stored::subdirectories(&directory).len(),
        4,
        "asking to keep as many as there are removes none"
    );

    stored::oldest(&all, 1);
    let left = stored::subdirectories(&directory);
    assert_eq!(left.len(), 1, "and keeping one leaves one: {left:?}");
    assert_eq!(
        left.first(),
        all.last(),
        "the last of the list, because the list is in name order and a name is when it \
         ran"
    );
}
