// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running the suite under a sanitizer: what it finds, and what it says when it cannot run.

#![expect(
    clippy::expect_used,
    reason = "the helpers that build one request and read back what the process it started saw are not themselves tests, and a setup that did not happen is reported by panicking"
)]
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use njutest_cli::assure::sanitize::{Sanitizing, sanitize};
use njutest_cli::report::FindingKind;
use njutest_cli::trace::Recorder;
use njutest_cli::watch::Watch;
use rust_mutants::runner::Cancel;

/// The cargo every test here drives, read rather than written: see the script's own note.
fn cargo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/fake-cargo.sh")
}

/// What that cargo is told to say, and how it is told to end.
fn saying(said: &str, code: i32) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter(|(name, _)| njutest_devkit::paths::same_name(name, std::ffi::OsStr::new("PATH")))
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
) -> njutest_cli::assure::sanitize::Sanitized {
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
    .expect("the fixture sanitizer output is valid UTF-8")
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
        njutest_cli::trace::Sink::Memory(njutest_cli::trace::MemorySink::unbounded()),
        njutest_cli::trace::Clock::stepping(
            jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
            Duration::from_secs(1),
        ),
        njutest_cli::trace::StartRecord::of(
            "20260909T000000Z-000001",
            njutest_cli::report::RunKind::Full,
            njutest_cli::config::Contract::DeepV1,
        ),
    );
    let cargo = cargo();
    let packages = ["core".to_owned()];

    let done = sanitize(
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
    )
    .expect("the fixture sanitizer output is valid UTF-8");
    assert_eq!(done.ran, ["address"], "the recorded command completed");

    let exec = trace
        .events()
        .iter()
        .find_map(|event| {
            njutest_cli::testkit::payload::of(&event.payload)
                .exec()
                .cloned()
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

/// What the cargo a run started actually saw in its environment.
fn as_started(dir: &Path, base: Vec<(std::ffi::OsString, std::ffi::OsString)>) -> String {
    let seen = dir.join("environment");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    let mut env = base;
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_ENV_OUT"),
        std::ffi::OsString::from(seen.display().to_string()),
    ));
    let done = sanitize(
        &Sanitizing {
            root: dir,
            cargo: &cargo,
            host: "x86_64-unknown-linux-gnu",
            env,
            packages: &[],
            sanitizers: &["address".to_owned()],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the fixture sanitizer output is valid UTF-8");
    assert_eq!(done.ran, ["address"], "the environment probe completed");
    std::fs::read_to_string(&seen).expect("what the cargo it started saw")
}

#[test]
fn the_flags_a_sanitizer_needs_are_the_ones_the_process_it_started_had() {
    let dir = tempfile::tempdir().expect("tempdir");
    let seen = as_started(dir.path(), saying("test result: ok", 0));
    assert!(
        seen.contains("RUSTFLAGS=-Zsanitizer=address\n"),
        "a sanitizer is a compiler flag, so a run that composed it and did not hand it \
         over builds an uninstrumented suite and reports that it found nothing: {seen}"
    );

    let mut carrying = saying("test result: ok", 0);
    carrying.push((
        std::ffi::OsString::from("RUSTFLAGS"),
        std::ffi::OsString::from("--cfg mine"),
    ));
    carrying.push((
        std::ffi::OsString::from("CARGO_ENCODED_RUSTFLAGS"),
        std::ffi::OsString::from("--cfg\u{1f}theirs"),
    ));
    let seen = as_started(dir.path(), carrying);
    assert!(
        seen.contains("RUSTFLAGS=--cfg mine -Zsanitizer=address\n"),
        "and what the run was already building with is still there beside it, separated \
         the way a compiler reads them: a phase that dropped it would build something \
         other than the workspace under measurement: {seen}"
    );
    assert!(
        seen.contains("CARGO_ENCODED_RUSTFLAGS=<unset>"),
        "while the encoded form is taken away, because cargo reads that one instead of \
         the flags this phase just composed, and a suite built without the sanitizer \
         that reports no finding is the worst answer this phase can give: {seen}"
    );
}

#[test]
fn a_run_that_named_no_package_asks_the_sanitizer_for_the_whole_workspace() {
    let dir = tempfile::tempdir().expect("a directory");
    let cancel = Cancel::new();
    let trace = Recorder::new(
        njutest_cli::trace::Sink::Memory(njutest_cli::trace::MemorySink::unbounded()),
        njutest_cli::trace::Clock::stepping(
            jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
            Duration::from_secs(1),
        ),
        njutest_cli::trace::StartRecord::of(
            "20260909T000000Z-000001",
            njutest_cli::report::RunKind::Full,
            njutest_cli::config::Contract::DeepV1,
        ),
    );
    let cargo = cargo();
    let done = sanitize(
        &Sanitizing {
            root: dir.path(),
            cargo: &cargo,
            host: "x86_64-unknown-linux-gnu",
            env: saying("test result: ok", 0),
            packages: &[],
            sanitizers: &["address".to_owned()],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the fixture sanitizer output is valid UTF-8");
    assert_eq!(done.ran, ["address"], "the recorded command completed");
    let exec = trace
        .events()
        .iter()
        .find_map(|event| {
            njutest_cli::testkit::payload::of(&event.payload)
                .exec()
                .cloned()
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
            "--workspace".to_owned(),
            "--offline".to_owned(),
            "--locked".to_owned(),
        ],
        "a run that named no package asked about all of them, and one that asked the \
         sanitizer about cargo's default instead would report a clean workspace having \
         checked one package of it"
    );
}

#[test]
fn what_a_sanitizer_could_not_say_is_said_in_words_a_person_can_act_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let done = sanitized("test result: ok", 0, dir.path(), &["address".to_owned()]);
    let stated = done.limitations.first().expect("a limitation");
    assert!(
        stated
            .detail
            .contains("standard library the suite links is not built with the sanitizer"),
        "every sanitizer run says what it did not instrument, in the words rather than \
         only by the name: a name is a code a reader looks up and a sentence is one they \
         act on: {stated:?}"
    );

    let refused = sanitized(
        "error: the option `Z` is only accepted on the nightly compiler",
        1,
        dir.path(),
        &["address".to_owned()],
    );
    assert!(
        refused
            .limitations
            .iter()
            .any(|one| one.detail.contains("address was asked for")
                && one.detail.contains("the toolchain would not run it")),
        "a sanitizer that could not run says which one and why, because the two \
         together are the whole of what somebody would change: {refused:?}"
    );
    assert!(
        refused
            .findings
            .iter()
            .any(|one| one.detail.contains("the suite was not run under address")),
        "and the finding says the same thing to whoever reads findings rather than \
         limitations: {refused:?}"
    );

    let failing = sanitized(
        "test result: FAILED. 1 failed",
        101,
        dir.path(),
        &["address".to_owned()],
    );
    assert!(
        failing
            .findings
            .iter()
            .any(|one| one.detail == "a test fails under address that passes without it"),
        "and a suite that fails only under the sanitizer says that it is the sanitizer \
         that makes the difference, which is the whole finding: {failing:?}"
    );
}

#[test]
fn a_sanitizer_that_runs_out_of_time_has_not_checked_anything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    let mut env = saying("test result: ok", 0);
    env.push((
        std::ffi::OsString::from("FAKE_CARGO_SLEEP"),
        std::ffi::OsString::from("5"),
    ));
    let done = sanitize(
        &Sanitizing {
            root: dir.path(),
            cargo: &cargo,
            host: "x86_64-unknown-linux-gnu",
            env,
            packages: &[],
            sanitizers: &["address".to_owned()],
            timeout: Some(Duration::from_millis(200)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("the fixture sanitizer output is valid UTF-8");
    assert!(
        done.ran.is_empty(),
        "a sanitizer run that was stopped is not a sanitizer run: counting it makes a \
         suite nobody finished checking into one that came back clean: {done:?}"
    );
    assert!(
        done.limitations
            .iter()
            .any(|one| one.detail.contains("address was asked for")
                && one.detail.contains("it ran out of time")),
        "and it says which of the ways it could fail this was, because more time and a \
         different toolchain are different things to do: {done:?}"
    );
    assert!(
        done.findings
            .iter()
            .any(|one| one.kind == FindingKind::NotMeasured),
        "and what was asked for and not done is a finding: {done:?}"
    );
}
