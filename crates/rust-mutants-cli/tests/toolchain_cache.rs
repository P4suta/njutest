// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind, what a later command finds, and what puts one finding back to the tests.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, reads a document as a table, and recounts a fixture the way a reader does"
)]

use std::ffi::OsString;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

include!("support/metadata.rs");

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
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
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

fn said(output: &Output) -> String {
    njutest_devkit::process::strict_utf8(&output.stdout).into_owned()
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
        test_metadata(
            &rust_mutants_cli::app::stored::Store::read(fixture.root())
                .root()
                .join("monday/run-report-v2.json"),
        )
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
        njutest_devkit::process::strict_utf8(&refused.stderr).contains("--run-id"),
        "{refused:?}"
    );
}

#[test]
fn cache_says_what_the_store_holds_and_clear_outcomes_empties_it() {
    let fixture = Fixture::copy("fixture-simple");
    let measured = against(
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
    assert!(
        measured.status.code().is_some_and(|code| code < 2),
        "the arranging run reached a mutation verdict: {measured:?}"
    );
    let listed = said(&against(&fixture, &["cache"]));
    assert!(listed.contains("outcomes     "), "{listed}");
    assert!(listed.contains("kept         0"), "{listed}");
    let cleared = said(&against(&fixture, &["cache", "--clear-outcomes"]));
    assert!(
        cleared.contains("outcomes") && cleared.contains("removed"),
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
    let measured = against(
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
    assert!(
        measured.status.code().is_some_and(|code| code < 2),
        "the arranging run reached a mutation verdict: {measured:?}"
    );
    let output = against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", "f0d2"],
    );
    let text = said(&output);
    assert!(
        text.contains(
            "REPLAY    f0d20edfda2959667ff1 survived, which is the proof that \
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
    let measured = against(
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
    assert!(
        measured.status.code().is_some_and(|code| code < 2),
        "the arranging run reached a mutation verdict: {measured:?}"
    );
    let output = against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", "f0d2"],
    );
    let text = said(&output);
    assert!(
        text.contains("REPLAY    f0d20edfda2959667ff1 still survived"),
        "a replay says whether the answer is still the same: {text}"
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
}

/// A run of the fixture, and what it stored.
fn measured(fixture: &Fixture) {
    let output = against(fixture, &["run", "--offline", "--locked", "--ui", "quiet"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

/// How many of the run's rows an earlier run answered for.
fn reused(fixture: &Fixture) -> usize {
    let directory = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    );
    let text = std::fs::read_to_string(directory.join("run-report-v2.json")).expect("the report");
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("the report is JSON");
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
    measured(&fixture);
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

    measured(&fixture);
    let warm = reused(&fixture);
    assert!(
        warm > 0,
        "a file no unit compiled cannot change what a test says, so every answer still answers"
    );
}

#[test]
fn an_edit_to_a_file_a_target_compiled_is_an_answer_that_stops_answering() {
    let fixture = Fixture::copy("fixture-simple");
    measured(&fixture);

    let path = fixture.root().join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the library");
    std::fs::write(
        &path,
        format!("{source}\n/// One more thing the tests do not call.\npub const ADDED: u8 = 1;\n"),
    )
    .expect("the library changes");

    measured(&fixture);
    assert_eq!(
        reused(&fixture),
        0,
        "the file the tests run is the file that decides; nothing about it is remembered"
    );
}

/// The stored report of the run a test named, as a document a test can rewrite.
fn stored(fixture: &Fixture) -> (std::path::PathBuf, serde_json::Value) {
    let path = rust_mutants_cli::app::stored::Store::read(fixture.root())
        .root()
        .join("monday/run-report-v2.json");
    let text = std::fs::read_to_string(&path).expect("the run this test named");
    (
        path,
        njutest_devkit::strictjson::decode_str(&text).expect("a report is JSON"),
    )
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

/// The document with the accounting and score its rows imply, which is what a reader holds a stored answer to.
fn refolded(mut document: serde_json::Value) -> serde_json::Value {
    let rows = document["mutants"].as_array().expect("the rows").clone();
    let mut counted: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    let (mut unreached, mut discharged, mut expected) = (0u64, 0u64, 0u64);
    for row in &rows {
        *counted
            .entry(row["outcome"].as_str().expect("an outcome"))
            .or_insert(0) += 1;
        unreached += u64::from(row["not_run_reason"].as_str() == Some("unreached"));
        discharged += u64::from(row["not_run_reason"].as_str() == Some("discharged"));
        expected += u64::from(row["expected"].as_bool().unwrap_or_default());
    }
    let rejections = document["rejections"].as_array().map_or(0, Vec::len);
    {
        let accounting = document["accounting"]
            .as_object_mut()
            .expect("the accounting");
        for field in [
            "killed",
            "survived",
            "step_limit_reached",
            "waited",
            "inconclusive",
            "errored",
            "not_run",
        ] {
            accounting.insert(
                field.into(),
                serde_json::json!(counted.get(field).copied().unwrap_or_default()),
            );
        }
        accounting.insert("unreached".into(), serde_json::json!(unreached));
        accounting.insert("discharged".into(), serde_json::json!(discharged));
        accounting.insert("expected".into(), serde_json::json!(expected));
        let cataloged = u64::try_from(rows.len() + rejections).expect("a count");
        accounting.insert("cataloged".into(), serde_json::json!(cataloged));
        accounting.insert(
            "executed".into(),
            serde_json::json!(cataloged - counted.get("not_run").copied().unwrap_or_default()),
        );
    }
    let detected = counted.get("killed").copied().unwrap_or_default();
    let decided = detected + counted.get("survived").copied().unwrap_or_default();
    document["score"] = if decided > 0 {
        serde_json::json!({"detected": detected, "decided": decided, "value": detected as f64 / decided as f64})
    } else {
        serde_json::Value::Null
    };
    document
}

#[test]
fn a_replay_says_what_the_stored_answer_was_and_never_more_than_it_knows() {
    let fixture = Fixture::copy("fixture-simple");
    let measured = against(
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
    assert!(
        measured.status.code().is_some_and(|code| code < 2),
        "the named arranging run reached a mutation verdict: {measured:?}"
    );
    let (path, document) = stored(&fixture);
    let killed = one_with(&document, "killed");

    let mut without = document.clone();
    without["mutants"] = serde_json::json!([]);
    without["findings"] = serde_json::json!([]);
    without["run"]["exit_code"] = serde_json::json!(0);
    rewritten(&path, &refolded(without));
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
    disagreeing["findings"]
        .as_array_mut()
        .expect("the findings")
        .push(serde_json::json!({
            "kind": "surviving-mutant",
            "mutant": killed,
            "detail": "no test noticed it"
        }));
    disagreeing["run"]["exit_code"] = serde_json::json!(1);
    rewritten(&path, &refolded(disagreeing.clone()));
    let text = said(&against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", &killed],
    ));
    assert!(
        text.contains("was survived, now killed"),
        "an answer that moved is reported as having moved, both halves named: {text}"
    );

    let mut proven = disagreeing;
    proven["mutants"][at]["outcome"] = serde_json::json!("not_run");
    proven["mutants"][at]["not_run_reason"] = serde_json::json!("discharged");
    proven["mutants"][at]["unreached"] = serde_json::json!(false);
    let findings = proven["findings"].as_array_mut().expect("the findings");
    findings.retain(|finding| finding["mutant"].as_str() != Some(killed.as_str()));
    findings.push(serde_json::json!({
        "kind": "discharged-mutant",
        "mutant": killed,
        "detail": "a proof discharged it"
    }));
    proven["run"]["exit_code"] = serde_json::json!(1);
    rewritten(&path, &refolded(proven));
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
    let measured = against(
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
    assert!(
        measured.status.code().is_some_and(|code| code < 2),
        "the named arranging run reached a mutation verdict: {measured:?}"
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
    let message = njutest_devkit::process::strict_utf8(&output.stderr).into_owned();
    assert_eq!(
        output.status.code(),
        Some(2),
        "a run nobody stored is not a run that said nothing about this mutation: {}",
        said(&output)
    );
    assert!(
        message.contains("tuesday")
            && message.contains(&format!(
                "{}",
                rust_mutants_cli::config::Config::default()
                    .reports
                    .directory
                    .display()
            )),
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
