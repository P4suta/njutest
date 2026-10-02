// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The cost publisher's records, read back as the complete protocol a reader sees.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and reads as a table"
)]

use std::path::PathBuf;

/// The private marker a parent test sets to let a child-side helper case run.
const CHILD: &str = "NJUTEST_TEST_COST_CHILD";

/// Every work field the reader requires of a record, by name.
const REQUIRED: [&str; 19] = [
    "builds",
    "build_ms",
    "units",
    "build_requests",
    "build_hits",
    "build_misses",
    "build_keys",
    "unbound",
    "launch_failures",
    "observed_cargo_starts",
    "cargo_probes",
    "cargo_probe_ms",
    "cargo_metadata",
    "cargo_metadata_ms",
    "rustc_probes",
    "rustc_probe_ms",
    "unobserved_cargo",
    "platform",
    "platform_requests",
];

fn one_record(directory: &std::path::Path) -> serde_json::Value {
    let mut records: Vec<PathBuf> = std::fs::read_dir(directory)
        .expect("the cost directory")
        .map(|entry| entry.expect("one record").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    records.sort();
    assert_eq!(records.len(), 1, "exactly one record: {records:?}");
    njutest_devkit::strictjson::decode_slice(
        &std::fs::read(records.first().expect("the one")).expect("the record"),
    )
    .expect("a complete record")
}

fn the_work(record: &serde_json::Value) -> &serde_json::Value {
    record.get("work").expect("the work block")
}

fn count(work: &serde_json::Value, name: &str) -> u64 {
    work.get(name)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("the count {name} is present and measured"))
}

fn run_child(test: &str, directory: &std::path::Path) {
    let status = std::process::Command::new(std::env::current_exe().expect("this binary"))
        .args(["--exact", test, "--nocapture"])
        .env(CHILD, "1")
        .env("NJUTEST_TEST_COST_DIR", directory)
        .status()
        .expect("the child test runs");
    assert!(status.success(), "the child test {test} passed: {status}");
}

/// Gives the suite the child's measured work under this test's own nextest identity, exactly once.
fn owned_by_suite(record: &serde_json::Value) {
    if std::env::var_os("NJUTEST_TEST_COST_DIR").is_none() {
        return;
    }
    let root = record
        .get("root")
        .and_then(serde_json::Value::as_str)
        .expect("the child's measured root");
    njutest_devkit::cost::record(
        std::path::Path::new(root),
        record.get("work").expect("the child's measured work"),
        record.get("sealed").expect("the child's sealed work"),
    )
    .expect("the suite record of the child's work");
}

#[test]
fn a_real_direct_build_publishes_every_field_the_reader_requires() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_real_build", guard.path());
    let record = one_record(guard.path());
    owned_by_suite(&record);
    let work = the_work(&record);
    let absent: Vec<&str> = REQUIRED
        .iter()
        .filter(|name| work.get(**name).is_none_or(serde_json::Value::is_null))
        .copied()
        .collect();
    assert!(
        absent.is_empty(),
        "a published work payload carries every required field: absent {absent:?}: {work}"
    );
    assert_eq!(count(work, "observed_cargo_starts"), 1);
    assert_eq!(count(work, "builds"), 1);
    assert_eq!(count(work, "build_requests"), 1);
    assert_eq!(count(work, "launch_failures"), 0);
    assert!(
        count(work, "units") >= 1,
        "an actual build derives its artifacts from the Cargo JSON stream it observed: {work}"
    );
}

#[test]
fn a_failed_direct_attempt_closes_without_asking_the_cache() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_direct_failed", guard.path());
    let record = one_record(guard.path());
    owned_by_suite(&record);
    let work = the_work(&record);
    let held = work
        .get("unbound")
        .and_then(serde_json::Value::as_object)
        .and_then(|unbound| unbound.values().next())
        .expect("the attempt's counts");
    let said = |name: &str| held.get(name).and_then(serde_json::Value::as_u64);
    assert_eq!(
        said("misses"),
        Some(0),
        "direct work never asks the cache, so a failed start misses nothing: {work}"
    );
    assert_eq!(said("requests"), Some(1));
    assert_eq!(said("failed_launches"), Some(1));
    assert_eq!(said("processes"), Some(0));
    assert_eq!(
        count(work, "build_misses"),
        0,
        "the suite-level miss count closes too"
    );
}

#[test]
fn a_direct_build_that_cannot_start_keeps_its_attempted_work() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_direct_failed", guard.path());
    let record = one_record(guard.path());
    owned_by_suite(&record);
    let work = the_work(&record);
    let absent: Vec<&str> = REQUIRED
        .iter()
        .filter(|name| work.get(**name).is_none_or(serde_json::Value::is_null))
        .copied()
        .collect();
    assert!(
        absent.is_empty(),
        "a failed attempt publishes every required field too: absent {absent:?}: {work}"
    );
    let identity = work
        .get("unbound")
        .and_then(serde_json::Value::as_object)
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
    assert_eq!(
        said("misses"),
        Some(0),
        "direct work never asks the cache, so a failed start misses nothing"
    );
    assert_eq!(said("failed_launches"), Some(1));
    assert_eq!(said("processes"), Some(0));
    assert_eq!(count(work, "builds"), 0);
    assert_eq!(count(work, "build_misses"), 0);
    assert_eq!(count(work, "launch_failures"), 1);
    assert_eq!(count(work, "observed_cargo_starts"), 0);
    assert_eq!(
        count(work, "units"),
        0,
        "a never-started attempt proves zero artifacts"
    );
}

#[test]
fn cost_child_real_build() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let directory = std::env::var_os("NJUTEST_TEST_COST_DIR").expect("a cost directory");
    let root = std::path::Path::new(&directory).join("project");
    std::fs::create_dir_all(root.join("src")).expect("the project directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cost-direct-build\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [lib]\npath = \"src/lib.rs\"\n\n[workspace]\n",
    )
    .expect("the manifest");
    std::fs::write(root.join("src/lib.rs"), "pub fn one() -> u8 {\n    1\n}\n")
        .expect("the library");
    std::fs::write(
        root.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"cost-direct-build\"\nversion = \"0.0.0\"\n",
    )
    .expect("the lock file");
    let mut command = std::process::Command::new(njutest_devkit::paths::cargo_binary());
    command
        .args(["build", "--offline", "--locked"])
        .current_dir(&root);
    let built =
        njutest_devkit::cost::cargo(command, "a real direct build of a tiny offline project")
            .expect("the build runs");
    assert!(
        built.status.success(),
        "the real build succeeds:\n{:?}\n{:?}",
        built.stdout,
        built.stderr
    );
}

#[test]
fn cost_child_direct_failed() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let directory = std::env::var_os("NJUTEST_TEST_COST_DIR").expect("a cost directory");
    let mut command =
        std::process::Command::new(std::path::Path::new(&directory).join("no-such-cargo"));
    command
        .arg("--version")
        .current_dir(std::path::Path::new(&directory));
    let refused = njutest_devkit::cost::cargo(command, "a direct build that cannot start");
    assert!(refused.is_err(), "the launch itself fails");
}
