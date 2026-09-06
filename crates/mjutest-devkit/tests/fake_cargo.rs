// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the scripted toolchain answers, and what it does when nothing in its script matches.

#![expect(
    clippy::expect_used,
    reason = "the helper that starts the fake is not itself a test: a fake that will not run \
              leaves every assertion below it meaningless"
)]

use std::process::Command;

use mjutest_devkit::fake_cargo::{Invocation, Script, UNMATCHED_EXIT, install};

/// Runs the fake as `program` with `args`, in the environment its script needs.
fn run(
    installed: &mjutest_devkit::fake_cargo::Installed,
    program: &str,
    args: &[&str],
) -> std::process::Output {
    let mut command = Command::new(installed.bin().join(program));
    command.args(args);
    for (name, value) in installed.env() {
        command.env(name, value);
    }
    command.output().expect("the fake runs")
}

#[test]
fn a_scripted_command_answers_with_what_the_script_says() {
    let installed = install(
        &Script::new()
            .answering(Invocation::new("cargo", &["-vV"]).printing("cargo 1.98.0\n"))
            .answering(
                Invocation::new("cargo", &["metadata"]).failing(101, "error: no manifest\n"),
            ),
    );

    let banner = run(&installed, "cargo", &["-vV"]);
    assert_eq!(String::from_utf8_lossy(&banner.stdout), "cargo 1.98.0\n");
    assert_eq!(banner.status.code(), Some(0));

    let metadata = run(&installed, "cargo", &["metadata", "--format-version", "1"]);
    assert_eq!(metadata.status.code(), Some(101));
    assert_eq!(
        String::from_utf8_lossy(&metadata.stderr),
        "error: no manifest\n"
    );
    assert_eq!(installed.answered(), vec![0, 1], "in the order they ran");
}

#[test]
fn a_command_no_entry_matches_fails_loudly_with_its_own_command_line() {
    let installed = install(&Script::new().answering(Invocation::new("cargo", &["-vV"])));
    let output = run(&installed, "cargo", &["build", "--release"]);
    assert_eq!(
        output.status.code(),
        Some(i32::from(UNMATCHED_EXIT)),
        "an unscripted command is a test that forgot something, not a tool that ran"
    );
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("no entry of the script matches"), "{said}");
    assert!(said.contains("cargo build --release"), "{said}");
}

#[test]
fn a_scripted_binary_can_be_told_apart_by_the_mutant_it_was_started_for() {
    let installed = install(
        &Script::new()
            .answering(
                Invocation::new("demo", &[])
                    .when("RUST_MUTANTS_ACTIVE", "beef")
                    .failing(101, "")
                    .printing("test result: FAILED. 0 passed; 1 failed; 0 ignored\n"),
            )
            .answering(
                Invocation::new("demo", &[])
                    .printing("test result: ok. 1 passed; 0 failed; 0 ignored\n"),
            ),
    );

    let mut active = Command::new(installed.bin().join("demo"));
    for (name, value) in installed.env() {
        active.env(name, value);
    }
    active.env("RUST_MUTANTS_ACTIVE", "beef");
    let killed = active.output().expect("the fake runs");
    assert_eq!(killed.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&killed.stdout).contains("FAILED"));

    let survived = run(&installed, "demo", &[]);
    assert_eq!(survived.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&survived.stdout).contains("ok. 1 passed"));
}

#[test]
fn an_entry_answers_only_as_many_times_as_it_says() {
    let mut once = Invocation::new("cargo", &["-vV"]).printing("first\n");
    once.times = Some(1);
    let installed = install(
        &Script::new()
            .answering(once)
            .answering(Invocation::new("cargo", &["-vV"]).printing("second\n")),
    );
    assert_eq!(
        String::from_utf8_lossy(&run(&installed, "cargo", &["-vV"]).stdout),
        "first\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&run(&installed, "cargo", &["-vV"]).stdout),
        "second\n",
        "an entry that has answered its last time steps aside"
    );
}

#[test]
fn a_scripted_command_writes_the_files_a_build_would_have_left_behind() {
    let installed = install(
        &Script::new().answering(
            Invocation::new("cargo", &["build"])
                .writing("{{script_dir}}/out/artifact.d", "demo: src/lib.rs\n"),
        ),
    );
    let _output = run(&installed, "cargo", &["build"]);
    let written = installed
        .bin()
        .parent()
        .expect("the script directory")
        .join("out/artifact.d");
    assert_eq!(
        std::fs::read_to_string(&written).expect("the written file"),
        "demo: src/lib.rs\n"
    );
}
