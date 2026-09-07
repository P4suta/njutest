// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a failure says, and what it says to do about it.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::process::{Command, Output};

use mjutest_devkit::fixture::Fixture;

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.args(args);
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--offline", "--locked"]);
    command.output().expect("rust-mutants runs")
}

#[test]
fn a_reserved_variable_names_itself_and_says_what_to_do() {
    let fixture = Fixture::copy("fixture-simple");
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    let output = command
        .args(["list", "--root", &fixture.root().to_string_lossy()])
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
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
