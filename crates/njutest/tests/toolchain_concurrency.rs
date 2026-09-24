// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run establishes about each test binary's threads, on a fixture that spawns one and a fixture that spawns none.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document as a table"
)]

use njutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest::cli::Environment;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-concurrency-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

fn environment(root: &Path) -> Environment {
    let cache = njutest_devkit::paths::cache_beside(root).expect("a cache directory");
    Environment {
        cache_directory: cache,
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[]),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

/// The one part of the report `verify` leaves in `fixture`.
fn verified(fixture: &Fixture) -> serde_json::Value {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked"]
            .into_iter()
            .map(OsString::from),
        &environment(&fixture.root),
        &mut out,
        &mut err,
    );
    let answered = njutest_devkit::process::answered(code, out, err);
    assert!(
        matches!(answered.status.code(), Some(0..=2)),
        "the run finishes with a verdict: {answered:?}"
    );
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let path = fixture
        .root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(run.as_str())
        .join(njutest::app::reports::DOCUMENT_NAME);
    let whole: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&path).expect("the report"),
    )
    .expect("JSON");
    whole["report"]["builds"][0]["parts"][0].clone()
}

fn record_of<'a>(part: &'a serde_json::Value, target: &str) -> &'a serde_json::Value {
    let records = part["concurrency"].as_array().expect("concurrency records");
    let named: Vec<&serde_json::Value> = records
        .iter()
        .filter(|one| one["target"] == target)
        .collect();
    let [one] = named.as_slice() else {
        panic!("{target} is one record: {records:?}");
    };
    one
}

fn limitation<'a>(part: &'a serde_json::Value, name: &str) -> Vec<&'a str> {
    part["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter(|one| one["name"] == name)
        .filter_map(|one| one["detail"].as_str())
        .collect()
}

#[test]
fn a_binary_whose_test_spawns_a_thread_is_concurrent_and_says_where() {
    let fixture = fixture("fixture-threaded");
    let part = verified(&fixture);
    let record = record_of(&part, "fixture-threaded/lib/fixture_threaded");
    assert_eq!(record["standing"]["state"], "concurrent", "{record}");
    let because = record["standing"]["because"].as_array().expect("reasons");
    assert!(
        because.iter().any(|one| one["kind"] == "loose-reach"),
        "the baseline reached the code on a thread no test answers for: {record}"
    );
    assert!(
        because.iter().any(|one| one["kind"] == "starts"
            && one["path"] == "src/lib.rs"
            && one["what"] == "spawn"),
        "and the source that starts it is named: {record}"
    );
    let stated = limitation(&part, njutest::limitation::SCHEDULE_NOT_EXPLORED);
    let [detail] = stated.as_slice() else {
        panic!("one limitation names every binary nothing explored: {stated:?}");
    };
    assert!(
        detail.contains("fixture-threaded/lib/fixture_threaded"),
        "the binary is named in the closing list: {detail}"
    );
}

#[test]
fn a_binary_that_starts_nothing_and_reached_nothing_off_its_tests_is_proven_single_threaded() {
    let fixture = fixture("fixture-baseline");
    let part = verified(&fixture);
    let record = record_of(&part, "fixture-baseline/lib/fixture_baseline");
    assert_eq!(
        record["standing"]["state"], "single-threaded",
        "a binary whose tests stayed on their own threads and whose closure starts nothing \
         needs no schedule explored: {record}"
    );
    let doc = record_of(&part, "fixture-baseline/doc/fixture_baseline");
    assert_eq!(
        doc["standing"],
        serde_json::json!({ "state": "not-proven", "why": [{ "kind": "no-touch" }] }),
        "a doctest records no reach and its code sits in a doc string, so nothing is proven: {doc}"
    );
    let stated = limitation(&part, njutest::limitation::SCHEDULE_NOT_EXPLORED);
    assert!(
        stated
            .iter()
            .all(|detail| !detail.contains("fixture-baseline/lib/fixture_baseline")),
        "a proven binary is not a hole: {stated:?}"
    );
}

#[test]
fn a_delayed_guard_that_makes_a_test_late_is_a_schedule_dependence_the_run_names() {
    let fixture = fixture("fixture-scheduled");
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[schedules]\nexplore = 4\n",
    )
    .expect("the configuration");
    let part = verified(&fixture);
    let record = record_of(&part, "fixture-scheduled/lib/fixture_scheduled");
    assert_eq!(record["standing"]["state"], "concurrent", "{record}");
    let explored = &record["explored"];
    assert_eq!(explored["state"], "broke", "{record}");
    assert_eq!(explored["path"], "src/lib.rs", "{record}");
    assert_eq!(
        explored["line"], 9,
        "the guard in `work`, which the spawned thread reaches: {record}"
    );
    assert_eq!(
        explored["failed"],
        serde_json::json!(["tests::a_message_arrives_in_time"]),
        "{record}"
    );
    let findings: Vec<&serde_json::Value> = part["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|one| one["kind"] == "schedule-dependent")
        .collect();
    let [finding] = findings.as_slice() else {
        panic!("one binary a schedule broke is one finding: {findings:?}");
    };
    assert_eq!(
        finding["subject"],
        "fixture-scheduled/lib/fixture_scheduled"
    );
    assert!(
        finding["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("src/lib.rs:9")),
        "the finding names the guard a reader goes to: {finding}"
    );
}

#[test]
fn a_binary_not_proven_single_threaded_is_unexplored_until_a_run_asks() {
    let fixture = fixture("fixture-scheduled");
    let part = verified(&fixture);
    let record = record_of(&part, "fixture-scheduled/lib/fixture_scheduled");
    assert_eq!(
        record["explored"],
        serde_json::json!({ "state": "unexplored", "why": "not-asked" }),
        "{record}"
    );
    let stated = limitation(&part, njutest::limitation::SCHEDULE_NOT_EXPLORED);
    assert!(
        stated
            .iter()
            .any(|detail| detail.contains("fixture-scheduled/lib/fixture_scheduled")),
        "{stated:?}"
    );
}
