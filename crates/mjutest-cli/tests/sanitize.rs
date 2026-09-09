// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running the suite under a sanitizer: what it finds, and what it says when it cannot run.
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use mjutest_cli::assure::sanitize::{Sanitizing, sanitize};
use mjutest_cli::report::FindingKind;
use mjutest_cli::trace::Recorder;
use mjutest_cli::watch::Watch;
use rust_mutants::runner::Cancel;

/// The cargo every test here drives, read rather than written: see the script's own note.
fn cargo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/fake-cargo.sh")
}

/// What that cargo is told to say, and how it is told to end.
fn saying(said: &str, code: i32) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter(|(name, _)| name == "PATH")
        .collect();
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_SAYS"),
        std::ffi::OsString::from(said),
    ));
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_CODE"),
        std::ffi::OsString::from(code.to_string()),
    ));
    env
}

fn sanitized(
    said: &str,
    code: i32,
    dir: &Path,
    sanitizers: &[String],
) -> mjutest_cli::assure::sanitize::Sanitized {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    sanitize(
        &Sanitizing {
            root: dir,
            cargo: &cargo,
            host: "x86_64-unknown-linux-gnu",
            env: saying(said, code),
            packages: &[],
            sanitizers,
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    )
}

#[test]
fn a_configuration_that_asks_for_no_sanitizer_runs_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let done = sanitized("", 0, dir.path(), &[]);
    assert!(done.ran.is_empty());
    assert!(done.findings.is_empty());
    assert!(done.limitations.is_empty());
}

#[test]
fn a_suite_a_sanitizer_passes_is_one_it_ran_under() {
    let dir = tempfile::tempdir().expect("tempdir");
    let done = sanitized(
        "test result: ok. 3 passed",
        0,
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
    let done = sanitized(said, 1, dir.path(), &["address".to_owned()]);
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
    let done = sanitized(said, 66, dir.path(), &["thread".to_owned()]);
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::UndefinedBehaviour)
    );
}

#[test]
fn a_sanitizer_that_was_asked_for_and_could_not_run_is_a_gap_and_not_a_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: the option `Z` is only accepted on the nightly compiler";
    let done = sanitized(said, 1, dir.path(), &["address".to_owned()]);
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
        "test result: FAILED. 1 failed",
        101,
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
        "test result: ok",
        0,
        dir.path(),
        &["address".to_owned(), "leak".to_owned()],
    );
    assert_eq!(done.ran, ["address", "leak"]);
}

#[test]
fn what_a_run_asks_of_the_sanitizer_is_the_whole_of_what_it_asks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::new(
        mjutest_cli::trace::Sink::Memory(mjutest_cli::trace::MemorySink::unbounded()),
        mjutest_cli::trace::Clock::stepping(
            jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
            Duration::from_secs(1),
        ),
        mjutest_cli::trace::StartRecord::of(
            "20260909T000000Z-000001",
            mjutest_cli::report::RunKind::Full,
            mjutest_cli::config::Contract::DeepV1,
        ),
    );
    let cargo = cargo();
    let packages = ["core".to_owned()];

    let _done = sanitize(
        &Sanitizing {
            root: dir.path(),
            cargo: &cargo,
            host: "x86_64-unknown-linux-gnu",
            env: saying("", 0),
            packages: &packages,
            sanitizers: &["address".to_owned()],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    );

    let exec = trace
        .events()
        .iter()
        .find_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::Exec { exec } => Some(exec.clone()),
            _ => None,
        })
        .expect("the command it started");
    assert_eq!(
        exec.argv,
        vec![
            cargo.display().to_string(),
            "+nightly".to_owned(),
            "test".to_owned(),
            "--target".to_owned(),
            "x86_64-unknown-linux-gnu".to_owned(),
            "--package".to_owned(),
            "core".to_owned(),
            "--offline".to_owned(),
            "--locked".to_owned(),
        ],
        "a sanitizer needs the standard library built with it, which is what naming the \
         host target asks for, and the rest is what the run was asked about and the two \
         promises it makes to every cargo command"
    );
    assert_eq!(
        exec.dir.as_deref(),
        Some(dir.path().display().to_string().as_str())
    );
    assert_eq!(exec.timeout_ms, Some(30_000));
}
