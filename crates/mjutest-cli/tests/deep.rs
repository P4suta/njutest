// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `deep-v1` soundness phase: what interpreting the suite establishes, and what it refuses to call a pass.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use mjutest_cli::assure::deep::{Interpreted, Interpreting, interpret};
use mjutest_cli::error::RunnerError;
use mjutest_cli::report::FindingKind;
use mjutest_cli::trace::Recorder;
use mjutest_cli::watch::Watch;
use rust_mutants::runner::Cancel;

/// A cargo that says `said` and ends with `code`.
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

fn interpreted(cargo: &Path, dir: &Path) -> Result<Interpreted, RunnerError> {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    interpret(
        &Interpreting {
            root: dir,
            cargo,
            env: std::env::vars_os()
                .filter(|(name, _)| name == "PATH")
                .collect(),
            packages: &[],
            flags: &[],
            timeout: Some(Duration::from_secs(30)),
            offline: true,
        },
        Watch::new(&cancel, &trace),
    )
}

#[test]
fn a_suite_the_interpreter_passes_is_one_the_run_says_was_interpreted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "   Compiling demo v0.1.0\ntest result: ok. 3 passed; 0 failed; 0 ignored";
    let done = interpreted(&cargo(dir.path(), "cargo-ok", said, 0), dir.path()).expect("ran");
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
    let done = interpreted(&cargo(dir.path(), "cargo-ub", said, 1), dir.path()).expect("ran");
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
    let done = interpreted(&cargo(dir.path(), "cargo-unsup", said, 1), dir.path()).expect("ran");
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
    let done = interpreted(&cargo(dir.path(), "cargo-failed", said, 101), dir.path()).expect("ran");
    assert_eq!(
        done.findings.first().map(|one| one.kind),
        Some(FindingKind::FailingTest)
    );
}

#[test]
fn a_toolchain_with_no_interpreter_cannot_answer_a_contract_that_promises_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let said = "error: 'cargo-miri' is not installed for the toolchain 'nightly'";
    let refused = interpreted(&cargo(dir.path(), "cargo-bare", said, 1), dir.path())
        .expect_err("no interpreter");
    assert_eq!(refused.code().code, "MJ7001", "{refused}");
    assert!(matches!(refused, RunnerError::MiriMissing { .. }));
}

#[test]
fn a_cargo_that_is_not_there_is_a_toolchain_with_no_interpreter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let refused = interpreted(&dir.path().join("nothing"), dir.path()).expect_err("no interpreter");
    assert_eq!(refused.code().code, "MJ7001", "{refused}");
}
