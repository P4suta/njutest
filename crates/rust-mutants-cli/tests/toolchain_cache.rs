// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind, what a later command finds, and what puts one finding back to the tests.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::ffi::OsString;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn against(fixture: &Fixture, args: &[&str]) -> Output {
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
    njutest_devkit::process::answered(code, out, err)
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
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
    assert!(listed.contains("outcomes     "), "{listed}");
    assert!(listed.contains("kept         0"), "{listed}");
    let cleared = said(&against(&fixture, &["cache", "--clear-outcomes"]));
    assert!(
        cleared.contains("outcomes     ") && cleared.contains("removed"),
        "a store emptied says how much was in it: {cleared}"
    );
    let after = said(&against(&fixture, &["cache"]));
    assert!(after.contains("outcomes     0 records"), "{after}");
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
        removed.contains("kept         3 removed"),
        "the snapshot, the build cache and the scratch are the three directories one \
         run kept: {removed}"
    );
    let after = said(&against(&fixture, &["cache"]));
    assert!(after.contains("kept         0"), "{after}");
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
        &["replay", "--offline", "--locked", "--tier", "all", "16b0"],
    );
    let text = said(&output);
    assert!(
        text.contains(
            "REPLAY    16b0cd40508fc0785477 survived, which is the proof that \
                       discharged it holding"
        ),
        "the run proved this mutation cannot be noticed rather than running it, and the \
         replay is what puts that proof to the tests: {text}"
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
}

#[test]
fn replaying_a_mutant_a_run_measured_says_whether_the_answer_is_still_the_same() {
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
            "--no-touch",
            "--ui",
            "quiet",
        ],
    );
    let output = against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", "16b0"],
    );
    let text = said(&output);
    assert!(
        text.contains("REPLAY    16b0cd40508fc0785477 still survived"),
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
    let directory = njutest_devkit::fixture::newest_run(fixture.root());
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

/// The stored report of the run a test named, as a document a test can rewrite.
fn stored(fixture: &Fixture) -> (std::path::PathBuf, serde_json::Value) {
    let path = fixture
        .root()
        .join("reports/mutation/monday/run-report-v1.json");
    let text = std::fs::read_to_string(&path).expect("the run this test named");
    (path, serde_json::from_str(&text).expect("a report is JSON"))
}

fn rewritten(path: &std::path::Path, document: &serde_json::Value) {
    std::fs::write(path, serde_json::to_string(document).expect("JSON")).expect("the report");
}

fn row_of(document: &serde_json::Value, id: &str) -> usize {
    document["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .position(|mutant| mutant["id"] == id)
        .expect("the row this test just read an id from")
}

fn one_with(document: &serde_json::Value, outcome: &str) -> String {
    document["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["outcome"] == outcome)
        .and_then(|mutant| mutant["id"].as_str())
        .unwrap_or_else(|| panic!("a mutant the run reported as {outcome}"))
        .to_owned()
}

#[test]
fn a_replay_says_what_the_stored_answer_was_and_never_more_than_it_knows() {
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
            "--run-id",
            "monday",
        ],
    );
    let (path, document) = stored(&fixture);
    let killed = one_with(&document, "killed");

    let mut without = document.clone();
    without["mutants"] = serde_json::json!([]);
    rewritten(&path, &without);
    let text = said(&against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", &killed],
    ));
    assert!(
        text.contains("was nothing, now killed"),
        "a replay of a mutation no stored run answered for says the run said nothing, \
         because \"still killed\" would put a claim in a run's mouth: {text}"
    );

    let mut disagreeing = document;
    let at = row_of(&disagreeing, &killed);
    disagreeing["mutants"][at]["outcome"] = serde_json::json!("survived");
    rewritten(&path, &disagreeing);
    let text = said(&against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", &killed],
    ));
    assert!(
        text.contains("was survived, now killed"),
        "an answer that moved is reported as having moved, both halves named: {text}"
    );

    let mut proven = disagreeing;
    proven["mutants"][at]["not_run_reason"] = serde_json::json!("discharged");
    rewritten(&path, &proven);
    let output = against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", &killed],
    );
    let text = said(&output);
    assert!(
        text.contains("killed, and a proof discharged it: the proof is wrong"),
        "a proof said no test could notice this mutation and a test noticed it. That is \
         a fact about this engine and not about the project's tests, and the replay is \
         the one place that says so: {text}"
    );
}

#[test]
fn a_replay_told_to_read_a_run_that_is_not_there_says_so_rather_than_reading_nothing() {
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
            "--run-id",
            "monday",
        ],
    );
    let (path, document) = stored(&fixture);
    let killed = one_with(&document, "killed");

    let output = against(
        &fixture,
        &[
            "replay",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--run",
            "tuesday",
            &killed,
        ],
    );
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(
        output.status.code(),
        Some(2),
        "a run nobody stored is not a run that said nothing about this mutation: {}",
        said(&output)
    );
    assert!(
        message.contains("tuesday") && message.contains("reports/mutation"),
        "and the refusal names what was asked for and where runs are kept: {message}"
    );

    std::fs::write(&path, "{ not a report ").expect("the report");
    let unreadable = against(
        &fixture,
        &[
            "replay",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--run",
            "monday",
            &killed,
        ],
    );
    assert_eq!(
        unreadable.status.code(),
        Some(2),
        "a stored run that cannot be read is not a stored run that answered nothing: {}",
        said(&unreadable)
    );
}
