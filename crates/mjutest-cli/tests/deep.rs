// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `deep-v1` soundness phase: what interpreting the suite establishes, and what it refuses to call a pass.

#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use mjutest_cli::assure::deep::{Interpreted, Interpreting, interpret};
use mjutest_cli::error::RunnerError;
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

fn interpreted(said: &str, code: i32, dir: &Path) -> Result<Interpreted, RunnerError> {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let cargo = cargo();
    interpret(
        &Interpreting {
            root: dir,
            cargo: &cargo,
            env: saying(said, code),
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    )
}

#[test]
fn a_suite_the_interpreter_passes_is_one_the_run_says_was_interpreted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "   Compiling demo v0.1.0\ntest result: ok. 3 passed; 0 failed; 0 ignored";
    let done = interpreted(said, 0, dir.path()).expect("ran");
    assert_eq!(
        done,
        Interpreted {
            executed: true,
            findings: Vec::new(),
            limitations: Vec::new(),
        }
    );
}

#[test]
fn undefined_behaviour_is_a_defect_and_not_a_gap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: Undefined Behavior: trying to retag from <1234> for Unique permission";
    let done = interpreted(said, 1, dir.path()).expect("ran");
    assert!(done.executed);
    let finding = done.findings.first().expect("a finding");
    assert_eq!(finding.kind, FindingKind::UndefinedBehaviour);
    assert!(finding.kind.is_defect());
    assert!(finding.detail.contains("Undefined Behavior"), "{finding:?}");
    assert!(done.limitations.is_empty(), "{done:?}");
}

#[test]
fn what_the_interpreter_will_not_interpret_is_stated_and_never_read_as_a_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: unsupported operation: can't call foreign function `getrandom`";
    let done = interpreted(said, 1, dir.path()).expect("ran");
    assert!(done.executed);
    assert_eq!(
        done.limitations.first().map(|one| one.name.clone()),
        Some("miri-unsupported".to_owned())
    );
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::NotMeasured),
        "a suite that was not interpreted whole is not one that was found sound"
    );
}

#[test]
fn a_test_that_fails_under_the_interpreter_is_a_failing_test() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "test result: FAILED. 2 passed; 1 failed; 0 ignored";
    let done = interpreted(said, 101, dir.path()).expect("ran");
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::FailingTest)
    );
}

#[test]
fn a_toolchain_with_no_interpreter_cannot_answer_a_contract_that_promises_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: 'cargo-miri' is not installed for the toolchain 'nightly'";
    let refused = interpreted(said, 1, dir.path()).expect_err("no interpreter");
    assert_eq!(refused.code().code, "MJ7001", "{refused}");
    assert!(matches!(refused, RunnerError::MiriMissing { .. }));
}

#[test]
fn a_cargo_that_is_not_there_is_a_toolchain_with_no_interpreter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let refused = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: &dir.path().join("nothing"),
            env: saying("", 0),
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    )
    .expect_err("no interpreter");
    assert_eq!(refused.code().code, "MJ7001", "{refused}");
}

#[test]
fn what_a_run_promised_cargo_it_would_not_do_is_said_to_this_cargo_too() {
    let dir = tempfile::tempdir().expect("a directory");
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

    let _done = interpret(
        &Interpreting {
            root: dir.path(),
            cargo: &cargo,
            env: saying("no undefined behaviour", 0),
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
            locked: true,
        },
        Watch::new(&cancel, &trace),
    );

    let argv: Vec<String> = trace
        .events()
        .iter()
        .find_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::Exec { exec } => Some(exec.argv.clone()),
            _ => None,
        })
        .expect("the command it started");
    assert!(
        argv.contains(&"--offline".to_owned()) && argv.contains(&"--locked".to_owned()),
        "`--locked` and `--offline` are promises about every cargo command a run starts, \
         and this is one of them: an interpretation that let cargo change the lock file \
         would measure a dependency set the baseline never saw, and would write into the \
         tree it is measuring: {argv:?}"
    );
}
