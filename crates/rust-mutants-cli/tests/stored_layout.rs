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
use rust_mutants::id::RunId;
use rust_mutants_cli::app::stored;
use rust_mutants_cli::cli;
use rust_mutants_cli::report::run as run_report;

include!("support/metadata.rs");

/// A directory a test arranges runs under, removed with the value.
fn scratch(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("rust-mutants-stored-{name}-"))
        .tempdir()
        .expect("a directory to arrange runs under")
}

/// A stored run of that name, as far as anything reading the directory can tell.
fn run_at(directory: &Path, id: &str) -> PathBuf {
    let path = directory.join(id);
    std::fs::create_dir_all(&path).expect("a run directory");
    std::fs::write(path.join(run_report::FILE_NAME), b"{}").expect("a run report");
    path
}

fn document() -> run_report::RunDocument {
    njutest_devkit::strictjson::decode_str(include_str!(
        "../../../fuzz/seeds/run_report/one-run.json"
    ))
    .expect("the committed current report seed")
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
    )
    .expect("the generated name is canonical");
    let later = stored::run_id(
        "2026-09-05T12:00:01Z"
            .parse::<Timestamp>()
            .expect("an instant a second after it"),
    )
    .expect("the generated name is canonical");
    assert!(
        earlier < later,
        "a directory listing is how the runs are read back, so the name has to sort the \
         way they ran: {earlier} then {later}"
    );
    assert!(
        earlier
            .as_str()
            .chars()
            .all(|one| one.is_ascii_alphanumeric()),
        "and it is a name every filesystem takes: {earlier}"
    );
    assert!(
        earlier.as_str().starts_with("20260905t120000"),
        "and a person reading it knows when it ran without opening it: {earlier}"
    );
    assert_ne!(
        stored::run_id(
            "2026-09-05T12:00:00.001Z"
                .parse::<Timestamp>()
                .expect("an instant a millisecond after it")
        )
        .expect("the generated name is canonical"),
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
        "a.b",
        "a.",
        "CON",
        "lpt9",
        "20260905T120000Z-ABCDEF",
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
        "v0-1-0",
        "a_b-c",
        "20260905t120000z-abcdef",
        "1",
        &"x".repeat(64),
    ] {
        let named = stored::named(&run_named(Some(right)), Timestamp::UNIX_EPOCH)
            .expect("the explicit run name is valid");
        assert_eq!(
            named.as_str(),
            right,
            "{right:?} is a name a directory can be, and a run asked for it: comparing two \
             runs is done by the names somebody gave them"
        );
    }
    let now = "2026-09-05T12:00:00Z"
        .parse::<Timestamp>()
        .expect("an instant");
    assert_eq!(
        stored::named(&run_named(None), now).expect("the generated name is valid"),
        stored::run_id(now).expect("the generated run id is valid"),
        "and a run nobody named goes by the instant it started"
    );
}

#[test]
fn a_new_run_cannot_alias_a_historical_spelling_by_case() {
    let owned_directory = scratch("write-case-alias");
    let directory = owned_directory.path();
    let historical = run_at(directory, "RUN-A");
    let id = RunId::try_from("run-a").expect("a canonical writable id");

    let refused = stored::store(directory, &id, &document())
        .expect_err("a case-fold alias is not a writable run");
    assert!(
        refused.to_string().contains("ASCII case"),
        "the complete inventory, not a later write failure, explains the refusal: {refused}"
    );
    assert_eq!(
        std::fs::read(historical.join(run_report::FILE_NAME)).expect("the historical report"),
        b"{}",
        "the historical run was not overwritten through a case-insensitive alias"
    );
}

#[test]
fn an_exact_canonical_run_may_replace_its_own_document() {
    let owned_directory = scratch("exact-replace");
    let directory = owned_directory.path();
    let id = RunId::try_from("same-run").expect("a canonical writable id");
    let first = stored::store(directory, &id, &document()).expect("the first write");
    let second = stored::store(directory, &id, &document()).expect("an exact replacement");
    assert_eq!(first, second);
}

#[test]
fn a_latest_pointer_cannot_bypass_a_case_collision_inventory() {
    let owned_directory = scratch("pointer-case-alias");
    let directory = owned_directory.path();
    let upper = run_at(directory, "RUN-A");
    let lower = run_at(directory, "run-a");
    if upper != lower
        && stored::subdirectories(directory)
            .expect("the store is readable")
            .len()
            == 2
    {
        std::fs::write(
            directory.join(run_report::LATEST_FILE_NAME),
            r#"{"run":"run-a"}"#,
        )
        .expect("a latest pointer");
        assert!(
            stored::newest(directory).is_err(),
            "the pointer fast path must first establish that stored spellings are unique"
        );
    }
}

#[test]
fn the_newest_run_is_the_one_the_last_run_pointed_at() {
    let owned_directory = scratch("pointer");
    let directory = owned_directory.path();
    let older = run_at(directory, "20260101T000000000Z");
    assert!(test_metadata(&older).is_dir(), "the older run was arranged");
    let newer = run_at(directory, "20260102T000000000Z");
    let by_name = stored::newest(directory).expect("a run is stored");
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
            "document": "../../outside.json",
        }))
        .expect("a pointer"),
    )
    .expect("a pointer the last run wrote");

    assert_eq!(
        stored::newest(directory).expect("a run is stored"),
        directory
            .join("20260101T000000000Z")
            .join(run_report::FILE_NAME),
        "and only the typed run component is used. The legacy document field is data, \
         never a path the reader follows"
    );
}

#[test]
fn a_pointer_at_a_run_nobody_kept_falls_back_to_the_names_rather_than_refusing() {
    let owned_directory = scratch("dangling");
    let directory = owned_directory.path();
    let newer = run_at(directory, "20260102T000000000Z");
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
        stored::newest(directory).expect("a run is still stored"),
        newer.join(run_report::FILE_NAME),
        "a pointer at a run that is gone is a pointer, not an answer: `--gc --all` \
         removes runs and leaves the pointer, and refusing there would mean a person \
         cannot read the reports they still have"
    );
}

#[test]
fn a_pointer_nobody_can_parse_is_refused_instead_of_becoming_another_run() {
    let owned_directory = scratch("unreadable-pointer");
    let directory = owned_directory.path();
    let only = run_at(directory, "20260102T000000000Z");
    for text in ["", "not json at all", "{}", r#"{"document": 7}"#] {
        std::fs::write(
            directory.join(run_report::LATEST_FILE_NAME),
            text.as_bytes(),
        )
        .expect("a pointer nobody can read");
        let error = stored::newest(directory).expect_err("the corrupt pointer is observable");
        assert!(
            error.to_string().contains("stored") || error.to_string().contains("pointer"),
            "a malformed pointer must not silently select {only:?}: {error}"
        );
    }
}

#[test]
fn a_pointer_run_is_a_typed_component_not_a_path() {
    let owned_directory = scratch("pointer-traversal");
    let directory = owned_directory.path();
    let only = run_at(directory, "20260102T000000000Z");
    assert!(test_metadata(&only).is_dir(), "the run was arranged");
    std::fs::write(
        directory.join(run_report::LATEST_FILE_NAME),
        r#"{"document_type":"rust-mutants/latest-run","schema_version":1,"run":"../../outside","document":"../../outside"}"#,
    )
    .expect("an adversarial pointer");
    assert!(
        stored::newest(directory).is_err(),
        "a pointer cannot escape the store or fall back to an unrelated valid run"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_cannot_stand_in_for_a_run_or_its_report() {
    use std::os::unix::fs::symlink;

    let owned_directory = scratch("symlink-run");
    let directory = owned_directory.path();
    let owned_outside = scratch("symlink-outside");
    let outside = owned_outside.path();
    let outside_run = run_at(outside, "elsewhere");
    assert!(
        test_metadata(&outside_run).is_dir(),
        "the outside run was arranged"
    );
    symlink(
        outside.join("elsewhere"),
        directory.join("20260101T000000000Z"),
    )
    .expect("an adversarial run symlink");
    assert!(
        stored::newest(directory).is_err()
            && stored::report_of(directory, Some("20260101T000000000Z")).is_err(),
        "a canonical name does not make a symlink beneath it part of the store"
    );

    let real = directory.join("20260102T000000000Z");
    std::fs::create_dir_all(&real).expect("a real run directory");
    symlink(
        outside.join("elsewhere").join(run_report::FILE_NAME),
        real.join(run_report::FILE_NAME),
    )
    .expect("an adversarial report symlink");
    assert!(
        stored::report_of(directory, Some("20260102T000000000Z")).is_err(),
        "the report itself must be a regular file rather than a link"
    );
}

#[test]
fn a_directory_with_no_run_in_it_is_named_rather_than_answered_about() {
    let owned_directory = scratch("empty");
    let directory = owned_directory.path();
    let refused = stored::newest(directory).expect_err("nothing is stored here");
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
    let owned_directory = scratch("prune");
    let directory = owned_directory.path();
    for day in 1..=5u32 {
        run_at(directory, &format!("2026010{day}T000000000Z"));
    }
    let recording = directory.join("20260106T000000000Z");
    std::fs::create_dir_all(recording.join("run")).expect("a run that recorded and did not report");
    let traces = directory.join("traces");
    std::fs::create_dir_all(traces.join("20260101T000000000Z")).expect("a recording of a command");

    stored::prune(directory, 2).expect("stored directories are readable");

    let left: Vec<String> = stored::subdirectories(directory)
        .expect("stored directories are readable")
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| njutest_devkit::paths::utf8(Path::new(name)).to_owned())
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
    let owned_directory = scratch("keep-zero");
    let directory = owned_directory.path();
    for day in 1..=3u32 {
        run_at(directory, &format!("2026010{day}T000000000Z"));
    }
    stored::prune(directory, 0).expect("keeping everything does not read the store");
    assert_eq!(
        stored::subdirectories(directory)
            .expect("stored directories are readable")
            .len(),
        3,
        "`keep = 0` is how a person says to keep every run, and reading it as a count \
         would delete the run that had just finished"
    );
}

#[test]
fn what_a_run_left_behind_is_a_directory_and_never_a_file_beside_them() {
    let owned_directory = scratch("files");
    let directory = owned_directory.path();
    run_at(directory, "20260101T000000000Z");
    std::fs::write(directory.join("latest.json"), b"{}").expect("the pointer");
    std::fs::write(directory.join("notes.md"), b"mine").expect("a file somebody put here");

    let found = stored::subdirectories(directory).expect("stored directories are readable");
    assert_eq!(
        found.len(),
        1,
        "a file beside the runs is not a run, and removing one to keep a count would \
         remove somebody's notes: {found:?}"
    );

    let (runs, recordings) = stored::kept(directory).expect("stored directories are readable");
    assert_eq!(runs.len(), 1, "and a run is one with a report in it");
    assert!(
        recordings.is_empty(),
        "and a recording is one without: {recordings:?}"
    );

    stored::prune(directory, 1).expect("stored directories are readable");
    assert!(
        test_metadata(&directory.join("notes.md")).is_file()
            && test_metadata(&directory.join("latest.json")).is_file(),
        "which is what keeps them there"
    );
}

#[test]
fn a_store_that_cannot_be_enumerated_is_not_reported_as_empty() {
    let owned_directory = scratch("not-a-directory");
    let directory = owned_directory.path();
    let path = directory.join("store");
    std::fs::write(&path, b"not a directory").expect("a file in the directory's place");

    let failure = stored::subdirectories(&path).expect_err("a partial store is never accepted");
    assert_eq!(
        failure.code(),
        rust_mutants::error::REPORT_MISSING,
        "the stable stored-report error class owns traversal failures: {failure}"
    );
    assert!(
        failure.to_string().contains(&path.display().to_string()),
        "the error names the directory that could not be read: {failure}"
    );
}

#[test]
fn removing_all_but_the_newest_removes_the_oldest_and_keeps_the_order_they_are_in() {
    let owned_directory = scratch("oldest");
    let directory = owned_directory.path();
    let all: Vec<PathBuf> = (1..=4u32)
        .map(|day| run_at(directory, &format!("2026010{day}T000000000Z")))
        .collect();

    stored::oldest(&all, 4).expect("keeping every directory is a complete cleanup");
    assert_eq!(
        stored::subdirectories(directory)
            .expect("stored directories are readable")
            .len(),
        4,
        "asking to keep as many as there are removes none"
    );

    stored::oldest(&all, 1).expect("the three oldest directories are reclaimable");
    let left = stored::subdirectories(directory).expect("stored directories are readable");
    assert_eq!(left.len(), 1, "and keeping one leaves one: {left:?}");
    assert_eq!(
        left.first(),
        all.last(),
        "the last of the list, because the list is in name order and a name is when it \
         ran"
    );
}
