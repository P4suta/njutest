// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind, what a later command finds, and what puts one finding back to the tests.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::process::{Command, Output};

use mjutest_devkit::fixture::Fixture;

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.args(args);
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.output().expect("rust-mutants runs")
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_run_can_be_named_and_its_report_is_called_that() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
            "--run-id",
            "monday",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        fixture
            .root()
            .join("reports/mutation/monday/run-report-v1.json")
            .is_file(),
        "a run a person named is a run they can find again: {}",
        said(&output)
    );
    let refused = against(
        &fixture,
        &["run", "--offline", "--locked", "--run-id", "../escape"],
    );
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--run-id"),
        "{refused:?}"
    );
}

#[test]
fn cache_says_what_the_store_holds_and_clear_outcomes_empties_it() {
    let fixture = Fixture::copy("fixture-simple");
    let _measured = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
        ],
    );
    let listed = said(&against(&fixture, &["cache"]));
    assert!(listed.contains("outcomes    "), "{listed}");
    assert!(listed.contains("kept        0"), "{listed}");
    let cleared = said(&against(&fixture, &["cache", "--clear-outcomes"]));
    assert!(
        cleared.contains("outcomes    ") && cleared.contains("removed"),
        "a store emptied says how much was in it: {cleared}"
    );
    let after = said(&against(&fixture, &["cache"]));
    assert!(after.contains("outcomes    0 records"), "{after}");
}

#[test]
fn a_kept_snapshot_outlives_the_run_and_cache_names_the_run_that_kept_it() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
            "--keep-temp",
            "--run-id",
            "kept-one",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let listed = said(&against(&fixture, &["cache"]));
    assert!(
        listed.contains("(kept-one)"),
        "a directory a run was told to keep is one a later command can find: {listed}"
    );
    let swept = said(&against(&fixture, &["cache", "--gc"]));
    assert!(
        swept.contains("(kept-one)"),
        "and a sweep leaves it alone unless asked: {swept}"
    );
    let removed = said(&against(&fixture, &["cache", "--gc", "--kept"]));
    assert!(
        removed.contains("kept        2 removed"),
        "the snapshot and the build cache are two directories one run kept: {removed}"
    );
    let after = said(&against(&fixture, &["cache"]));
    assert!(after.contains("kept        0"), "{after}");
}

#[test]
fn replaying_a_recorded_outcome_asks_the_question_the_run_asked() {
    let fixture = Fixture::copy("fixture-simple");
    let _measured = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
        ],
    );
    let output = against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", "e5e8"],
    );
    let text = said(&output);
    assert!(
        text.contains("REPLAY    e5e872bfbcb2afbbf7a1 still survived"),
        "a replay says whether the answer is still the same: {text}"
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
}

/// A run of the fixture, and what it stored.
fn measured(fixture: &Fixture) -> Output {
    let output = against(fixture, &["run", "--offline", "--locked", "--ui", "quiet"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    output
}

/// How many of the run's rows an earlier run answered for.
fn reused(fixture: &Fixture) -> usize {
    let directory = std::fs::read_dir(fixture.root().join("reports/mutation"))
        .expect("a stored run")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("run-report-v1.json").is_file())
        .max()
        .expect("the newest run");
    let text = std::fs::read_to_string(directory.join("run-report-v1.json")).expect("the report");
    let document: serde_json::Value = serde_json::from_str(&text).expect("the report is JSON");
    document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .filter(|row| !row["source_run_id"].is_null())
        .count()
}

#[test]
fn an_edit_to_a_file_no_unit_compiled_leaves_every_outcome_reusable() {
    let fixture = Fixture::copy("fixture-simple");
    let _first = measured(&fixture);
    let rows = reused(&fixture);
    assert_eq!(rows, 0, "the first run had nothing to reuse");

    std::fs::write(
        fixture.root().join("NOTES.md"),
        "A file the compiler never reads.\n",
    )
    .expect("a file beside the code");
    std::fs::create_dir_all(fixture.root().join("docs")).expect("a directory");
    std::fs::write(
        fixture.root().join("docs/design.md"),
        "Nothing to compile.\n",
    )
    .expect("another one");

    let _again = measured(&fixture);
    let warm = reused(&fixture);
    assert!(
        warm > 0,
        "a file no unit compiled cannot change what a test says, so every answer still answers"
    );
}

#[test]
fn an_edit_to_a_file_a_target_compiled_is_an_answer_that_stops_answering() {
    let fixture = Fixture::copy("fixture-simple");
    let _first = measured(&fixture);

    let path = fixture.root().join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the library");
    std::fs::write(
        &path,
        format!("{source}\n/// One more thing the tests do not call.\npub const ADDED: u8 = 1;\n"),
    )
    .expect("the library changes");

    let _again = measured(&fixture);
    assert_eq!(
        reused(&fixture),
        0,
        "the file the tests run is the file that decides; nothing about it is remembered"
    );
}
