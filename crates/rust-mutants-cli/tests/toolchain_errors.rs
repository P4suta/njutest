// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a failure says, and what it says to do about it.

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root());
    asked(
        &environment(fixture),
        &args
            .iter()
            .copied()
            .chain(["--root", root])
            .chain(["--offline", "--locked"])
            .collect::<Vec<&str>>(),
    )
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
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

/// A reserved variable is about the environment a process inherits, so this one starts a process.
#[test]
fn a_reserved_variable_names_itself_and_says_what_to_do() {
    let fixture = Fixture::copy("fixture-simple");
    let root = njutest_devkit::paths::utf8(fixture.root());
    let output = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")))
        .args(["list", "--root", root])
        .env("NO_COLOR", "1")
        .envs(njutest_devkit::paths::temporary_directory(fixture.temp()))
        .env("XDG_CACHE_HOME", fixture.cache())
        .env_remove("RUST_MUTANTS_ACTIVE")
        .env_remove("RUST_MUTANTS_CATALOG")
        .env("RUST_MUTANTS_TOUCH", "not-a-run")
        .output()
        .expect("rust-mutants runs");
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(complaint.contains("RM0006"), "{complaint}");
    assert!(complaint.contains("RUST_MUTANTS_TOUCH"), "{complaint}");
    assert!(
        complaint.contains("try: unset the RUST_MUTANTS_ variable"),
        "a reader is told the next step, not only the trouble: {complaint}"
    );
}

#[test]
fn a_target_left_out_by_a_name_that_is_not_one_lists_the_targets_there_are() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(
        &fixture,
        &[
            "run",
            "--tier",
            "all",
            "--skip-target",
            "fixture-simple/lib/no-such-target",
            "--ui",
            "quiet",
        ],
    );
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a target nobody builds is not a target left out: the run measured everything \
         the reader meant to leave out and said nothing. {complaint}"
    );
    assert!(complaint.contains("RM5004"), "{complaint}");
    assert!(
        complaint.contains("no-such-target")
            && complaint.contains("fixture-simple/lib/fixture_simple"),
        "naming what was asked for and the targets there are, which is what --target \
         already does with the same names: {complaint}"
    );
}

#[test]
fn a_target_this_run_did_not_build_is_still_one_the_workspace_declares() {
    let fixture = Fixture::copy("fixture-workspace");
    let output = against(
        &fixture,
        &[
            "run",
            "--tier",
            "all",
            "--package",
            "fixture-core",
            "--skip-target",
            "fixture-app/test/cli",
            "--ui",
            "quiet",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "a configuration is written once and a run is narrowed every day, so a name the \
         workspace declares is a name this run may be told to leave out even when it \
         built no such target: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
}

#[test]
fn an_unknown_target_lists_the_targets_there_are() {
    let fixture = Fixture::copy("fixture-simple");
    let listed_output = against(&fixture, &["list", "--tier", "all"]);
    let listed = njutest_devkit::process::strict_utf8(&listed_output.stdout);
    let short = listed
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .expect("a mutant")
        .to_owned();
    let output = against(
        &fixture,
        &[
            "run",
            "--tier",
            "all",
            "--mutant",
            &short,
            "--target",
            "no-such-target",
        ],
    );
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(complaint.contains("RM5004"), "{complaint}");
    assert!(
        complaint.contains("fixture-simple/lib/fixture_simple"),
        "a name that is not one names the ones there are: {complaint}"
    );
}

#[test]
fn a_marker_without_a_reason_says_how_to_write_one() {
    let fixture = Fixture::copy("fixture-simple");
    let path = fixture.root().join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(&path, format!("// rust-mutants: skip\n{source}")).expect("write");
    let output = against(&fixture, &["list"]);
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(complaint.contains("RM2008"), "{complaint}");
    assert!(
        complaint.contains("try: write the marker as"),
        "{complaint}"
    );
}

#[test]
fn the_harness_arguments_the_configuration_holds_reach_the_baseline() {
    let fixture = Fixture::copy("fixture-ignored");
    let plain = against(&fixture, &["run", "--tier", "all", "--ui", "quiet"]);
    assert!(
        njutest_devkit::process::strict_utf8(&plain.stdout).contains("not_run=4"),
        "every test of this fixture is `#[ignore]`d, so its one target runs nothing and \
         its four mutations reach nothing: {}",
        njutest_devkit::process::strict_utf8(&plain.stdout)
    );

    let told = Fixture::copy("fixture-ignored");
    std::fs::write(
        told.root().join(".rust-mutants.toml"),
        "version = 1\n\n[execution]\ntest_binary_args = [\"--include-ignored\"]\n",
    )
    .expect("a configuration");
    let output = against(&told, &["run", "--tier", "all", "--ui", "quiet"]);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        said.contains("killed=4"),
        "and the one argument this configuration holds is the one that runs them. The \
         baseline is one run of this project's suite, and a mutation put to a test the \
         baseline never ran is a kill nothing vouched for: {said}{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
}
