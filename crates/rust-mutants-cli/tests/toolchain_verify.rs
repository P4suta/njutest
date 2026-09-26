// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A tree whose own test fails, which is a tree no mutation of can be measured against.

use std::ffi::OsString;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

include!("support/missing.rs");

fn run(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all", "--offline", "--locked"])
            .chain(args.iter().copied())
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

#[test]
fn a_baseline_that_fails_its_own_test_refuses_the_session_with_rm5002() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let output = run(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a run that could not establish anything is not a run that found nothing"
    );
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
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
        test_missing(&rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),),
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
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        text.contains("killed=4 survived=0"),
        "a target that was already failing reports every mutation as killed, which is what \
         verification exists to stop: {text}"
    );
}

#[test]
fn a_refusal_names_every_target_that_failed_rather_than_the_first() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let output = run(&fixture, &[]);
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(
        said.contains("fixture-verify-fails/lib/fixture_verify_fails"),
        "{said}"
    );
    assert!(
        said.contains("fixture-verify-fails/test/beside"),
        "the second target failed too, and a run that stopped at the first would send the \
         reader round the loop again: {said}"
    );
    assert!(
        said.contains("--skip-target fixture-verify-fails/lib/fixture_verify_fails")
            && said.contains("--skip-target fixture-verify-fails/test/beside"),
        "what it says to do about it covers every one of them: {said}"
    );
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    }
}
