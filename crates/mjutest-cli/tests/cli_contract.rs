// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command-line contract of the `mjutest` binary: what `--version` and
//! `--help` print, and the exit code of a usage error, which is `3` — the
//! code of invalid input — and not clap's default.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, and asserts with panics"
)]

use std::path::Path;
use std::process::{Command, Output};

fn mjutest(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(args)
        .env_clear()
        .env("NO_COLOR", "1")
        .output()
        .expect("mjutest runs")
}

#[test]
fn version_flag_prints_the_binary_name_and_its_version() {
    let output = mjutest(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("mjutest {}\n", mjutest_cli::VERSION)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_flag_matches_the_recorded_help_text() {
    let output = mjutest(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/help.golden");
    mjutest_devkit::golden::golden(&golden, &output.stdout).expect("help text is the recorded one");
}

#[test]
fn a_bare_invocation_prints_the_help_to_stdout_and_exits_0() {
    // goatest: "A bare `goatest` prints the help text."
    let output = mjutest(&[]);
    assert_eq!(output.status.code(), Some(0));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/help.golden");
    mjutest_devkit::golden::golden(&golden, &output.stdout).expect("bare invocation is the help");
}

#[test]
fn an_unknown_subcommand_is_invalid_input_and_exits_3() {
    let output = mjutest(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("mjutest: "),
        "diagnostics carry the program prefix: {stderr}"
    );
    assert!(
        stderr.contains("frobnicate"),
        "names the offending argument: {stderr}"
    );
}

#[test]
fn an_unknown_flag_is_invalid_input_and_exits_3() {
    let output = mjutest(&["--no-such-flag"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--no-such-flag"), "{stderr}");
}
