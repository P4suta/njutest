// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running the suite under a sanitizer: what it finds, and what it says when it cannot run.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use mjutest_cli::assure::sanitize::{Sanitizing, sanitize};
use mjutest_cli::report::FindingKind;
use mjutest_cli::trace::Recorder;
use mjutest_cli::watch::Watch;
use rust_mutants::runner::Cancel;

fn cargo(dir: &Path, name: &str, said: &str, code: i32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let path = dir.join(name);
    std::fs::write(
        &path,
        format!("#!/bin/sh\ncat <<'SAID'\n{said}\nSAID\nexit {code}\n"),
    )
    .expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn sanitized(
    cargo: &Path,
    dir: &Path,
    sanitizers: &[String],
) -> mjutest_cli::assure::sanitize::Sanitized {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    sanitize(
        &Sanitizing {
            root: dir,
            cargo,
            host: "x86_64-unknown-linux-gnu",
            env: std::env::vars_os()
                .filter(|(name, _)| name == "PATH")
                .collect(),
            packages: &[],
            sanitizers,
            timeout: Some(Duration::from_secs(30)),
            offline: true,
        },
        Watch::new(&cancel, &trace),
    )
}

#[test]
fn a_configuration_that_asks_for_no_sanitizer_runs_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let done = sanitized(&cargo(dir.path(), "cargo-ok", "", 0), dir.path(), &[]);
    assert!(done.ran.is_empty());
    assert!(done.findings.is_empty());
    assert!(done.limitations.is_empty());
}

#[test]
fn a_suite_a_sanitizer_passes_is_one_it_ran_under() {
    let dir = tempfile::tempdir().expect("tempdir");
    let done = sanitized(
        &cargo(dir.path(), "cargo-ok", "test result: ok. 3 passed", 0),
        dir.path(),
        &["address".to_owned()],
    );
    assert_eq!(done.ran, ["address"]);
    assert!(done.findings.is_empty(), "{done:?}");
    assert_eq!(
        done.limitations
            .iter()
            .map(|one| one.name.clone())
            .collect::<Vec<String>>(),
        ["sanitizer-standard-library-not-instrumented"],
        "every sanitizer run says what it did not instrument"
    );
}

#[test]
fn what_a_sanitizer_finds_is_a_defect() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "==1234==ERROR: AddressSanitizer: heap-use-after-free on address 0x602000000010";
    let done = sanitized(
        &cargo(dir.path(), "cargo-asan", said, 1),
        dir.path(),
        &["address".to_owned()],
    );
    let finding = done.findings.first().expect("a finding");
    assert_eq!(finding.kind, FindingKind::UndefinedBehaviour);
    assert!(finding.kind.is_defect());
    assert_eq!(finding.subject, "sanitizer:address");
    assert!(
        finding.detail.contains("heap-use-after-free"),
        "{finding:?}"
    );
}

#[test]
fn a_data_race_the_thread_sanitizer_sees_is_a_defect() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "WARNING: ThreadSanitizer: data race (pid=1234)";
    let done = sanitized(
        &cargo(dir.path(), "cargo-tsan", said, 66),
        dir.path(),
        &["thread".to_owned()],
    );
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::UndefinedBehaviour)
    );
}

#[test]
fn a_sanitizer_that_was_asked_for_and_could_not_run_is_a_gap_and_not_a_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: the option `Z` is only accepted on the nightly compiler";
    let done = sanitized(
        &cargo(dir.path(), "cargo-stable", said, 1),
        dir.path(),
        &["address".to_owned()],
    );
    assert!(done.ran.is_empty(), "{done:?}");
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::NotMeasured)
    );
    assert!(
        done.limitations
            .iter()
            .any(|one| one.name == "sanitizer-unavailable"),
        "{done:?}"
    );
}

#[test]
fn a_test_that_fails_under_a_sanitizer_is_a_failing_test() {
    let dir = tempfile::tempdir().expect("tempdir");
    let done = sanitized(
        &cargo(
            dir.path(),
            "cargo-failed",
            "test result: FAILED. 1 failed",
            101,
        ),
        dir.path(),
        &["address".to_owned()],
    );
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::FailingTest)
    );
}

#[test]
fn every_sanitizer_the_configuration_names_is_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let done = sanitized(
        &cargo(dir.path(), "cargo-ok", "test result: ok", 0),
        dir.path(),
        &["address".to_owned(), "leak".to_owned()],
    );
    assert_eq!(done.ran, ["address", "leak"]);
}
