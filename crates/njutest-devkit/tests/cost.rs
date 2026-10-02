// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The cost publisher's records, read back as the bytes a reader sees.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and reads as a table"
)]

use std::path::PathBuf;

fn one_record(directory: &std::path::Path) -> serde_json::Value {
    let mut records: Vec<PathBuf> = std::fs::read_dir(directory)
        .expect("the cost directory")
        .map(|entry| entry.expect("one record").path())
        .collect();
    records.sort();
    assert_eq!(records.len(), 1, "exactly one record: {records:?}");
    let first = records.first().expect("the record");
    njutest_devkit::strictjson::decode_slice(&std::fs::read(first).expect("the record"))
        .expect("a complete record")
}

fn run_child(test: &str, directory: &std::path::Path) {
    let status = std::process::Command::new(std::env::current_exe().expect("this binary"))
        .args(["--exact", test, "--nocapture"])
        .env("NJUTEST_TEST_COST_DIR", directory)
        .env("NEXTEST_BINARY_ID", "njutest-devkit::suite")
        .env("NEXTEST_TEST_NAME", "cost_records_test")
        .status()
        .expect("the child test runs");
    assert!(status.success(), "the child test {test} passed: {status}");
}

#[test]
fn a_direct_build_owns_its_actual_start() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_direct_start", guard.path());
    let record = one_record(guard.path());
    assert_eq!(
        record.get("schema").and_then(|value| value.as_str()),
        Some("njutest-test-cost-v3"),
        "a v3-labelled payload carries v3 semantics"
    );
    let work = record.get("work").expect("the work block");
    assert_eq!(
        work.get("observed_cargo_starts")
            .and_then(serde_json::Value::as_u64),
        Some(1),
        "a helper that launches the process itself observed its start"
    );
    assert_eq!(
        work.get("builds").and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        work.get("launch_failures")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
}

#[test]
fn a_direct_build_that_cannot_start_keeps_its_attempted_work() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_direct_failed", guard.path());
    let record = one_record(guard.path());
    let work = record.get("work").expect("the work block");
    let identity = work
        .get("unbound")
        .and_then(|unbound| unbound.as_object())
        .and_then(|unbound| unbound.keys().next())
        .cloned()
        .expect("the attempt's identity");
    assert_eq!(
        identity, "direct: a direct build that cannot start",
        "the failed launch keeps exactly its request's identity"
    );
    let held = work
        .get("unbound")
        .and_then(|unbound| unbound.get(&identity))
        .expect("the attempt's counts");
    let said = |name: &str| held.get(name).and_then(serde_json::Value::as_u64);
    assert_eq!(said("requests"), Some(1), "the attempted request is kept");
    assert_eq!(said("failed_launches"), Some(1));
    assert_eq!(said("processes"), Some(0));
    assert_eq!(
        work.get("builds").and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert_eq!(
        work.get("launch_failures")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        work.get("observed_cargo_starts")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
}

#[test]
fn cost_child_direct_start() {
    if std::env::var_os("NJUTEST_TEST_COST_DIR").is_none() {
        return;
    }
    let mut command = std::process::Command::new(njutest_devkit::paths::cargo_binary());
    command.arg("--version").current_dir(std::env::temp_dir());
    njutest_devkit::cost::cargo(command, "the version probe of a direct build test")
        .expect("cargo runs");
}

#[test]
fn cost_child_direct_failed() {
    if std::env::var_os("NJUTEST_TEST_COST_DIR").is_none() {
        return;
    }
    let directory = std::env::var_os("NJUTEST_TEST_COST_DIR").expect("a cost directory");
    let mut command = std::process::Command::new(std::path::Path::new(&directory).join("no-such"));
    command
        .arg("--version")
        .current_dir(std::path::Path::new(&directory));
    let refused = njutest_devkit::cost::cargo(command, "a direct build that cannot start");
    assert!(refused.is_err(), "the launch itself fails");
}
