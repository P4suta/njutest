// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree whose own test fails, which is a tree no mutation of can be measured against.

#![expect(
    clippy::expect_used,
    reason = "the helper that starts the engine is not itself a test"
)]

use std::process::Output;

use mjutest_devkit::fixture::Fixture;

fn run(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    command.args(args);
    command.output().expect("rust-mutants runs")
}

#[test]
fn a_baseline_that_fails_its_own_test_refuses_the_session_with_rm5002() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let output = run(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a run that could not establish anything is not a run that found nothing"
    );
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("RM5002"), "{said}");
    assert!(
        said.contains("fixture-verify-fails/lib/fixture_verify_fails"),
        "the refusal names the target that failed: {said}"
    );
    assert!(
        said.contains("doubling_two_is_five"),
        "and what it said: {said}"
    );
    assert!(
        !said.contains("which the pristine tree passes"),
        "the run type-checked the pristine tree and never ran it, so it cannot say the tree \
         passes: {said}"
    );
    assert!(
        said.contains("--no-verify"),
        "the refusal says what to do about it: {said}"
    );
    assert!(
        !fixture.root().join("reports/mutation").exists(),
        "a run that established nothing writes no report"
    );
}

#[test]
fn without_verification_the_same_test_kills_every_mutant_it_touches() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let output = run(&fixture, &["--no-verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("killed=4 survived=0"),
        "a target that was already failing reports every mutation as killed, which is what \
         verification exists to stop: {text}"
    );
}
