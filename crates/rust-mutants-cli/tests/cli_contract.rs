// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command-line contract of the `rust-mutants` binary: what `--version`
//! and `--help` print, and the exit codes of usage errors.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;
use std::process::{Command, Output};

fn rust_mutants(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .args(args)
        .env_clear()
        .env("NO_COLOR", "1")
        .output()
        .expect("rust-mutants runs")
}

#[test]
fn version_flag_prints_the_binary_name_and_its_version() {
    let output = rust_mutants(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("rust-mutants {}\n", rust_mutants::VERSION)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_flag_matches_the_recorded_help_text() {
    let output = rust_mutants(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/help.golden");
    mjutest_devkit::golden::golden(&golden, &output.stdout).expect("help text is the recorded one");
}

#[test]
fn no_arguments_prints_the_usage_to_stderr_and_exits_2() {
    let output = rust_mutants(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn an_unknown_subcommand_is_a_usage_error() {
    let output = rust_mutants(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("frobnicate"),
        "names the offending argument: {stderr}"
    );
}
