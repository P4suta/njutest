// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a failure says, and what it says to do about it.

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = fixture.root().to_string_lossy().into_owned();
    asked(
        &environment(fixture),
        &args
            .iter()
            .copied()
            .chain(["--root", root.as_str()])
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
    mjutest_devkit::process::answered(code, out, err)
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: mjutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

/// A reserved variable is about the environment a process inherits, so this one starts a process.
///
/// Driven in this process instead, the variable reaches every child the
/// command starts, and under a measurement of this workspace this target's
/// baseline failed: what came back on the command's own stderr was the
/// instrumented runtime refusing an activation with no catalog beside it,
/// rather than the run refusing to inherit one. The rule was no longer the
/// rule under test. The environment a process is given is `main.rs`'s to
/// compose, and this is the one claim that is about that.
#[test]
fn a_reserved_variable_names_itself_and_says_what_to_do() {
    let fixture = Fixture::copy("fixture-simple");
    let output = mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")))
        .args(["list", "--root", &fixture.root().to_string_lossy()])
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .env("RUST_MUTANTS_ACTIVE", "0".repeat(64))
        .output()
        .expect("rust-mutants runs");
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(complaint.contains("RM0006"), "{complaint}");
    assert!(
        complaint.contains("try: unset the RUST_MUTANTS_ variable"),
        "a reader is told the next step, not only the trouble: {complaint}"
    );
}

#[test]
fn an_unknown_target_lists_the_targets_there_are() {
    let fixture = Fixture::copy("fixture-simple");
    let listed =
        String::from_utf8_lossy(&against(&fixture, &["list", "--tier", "all"]).stdout).into_owned();
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
    let complaint = String::from_utf8_lossy(&output.stderr);
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
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(complaint.contains("RM2008"), "{complaint}");
    assert!(
        complaint.contains("try: write the marker as"),
        "{complaint}"
    );
}
