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
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
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

/// The same, in a directory of its own, for a command that writes.
fn mjutest_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("NO_COLOR", "1")
        .output()
        .expect("mjutest runs")
}

fn golden_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/testdata/{name}"))
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

// --- the command tree --------------------------------------------------------------

#[test]
fn every_subcommand_has_its_own_recorded_help() {
    for name in ["verify", "init", "doctor"] {
        let output = mjutest(&[name, "--help"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        mjutest_devkit::golden::golden(
            &golden_path(&format!("help-{name}.golden")),
            &output.stdout,
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn a_subcommand_given_a_flag_it_does_not_know_is_invalid_input() {
    let output = mjutest(&["init", "--no-such-flag"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("mjutest: "), "{stderr}");
    assert!(stderr.contains("--no-such-flag"), "{stderr}");
}

// --- doctor ------------------------------------------------------------------------

#[test]
fn doctor_names_every_tool_a_run_needs_and_whether_it_is_there() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let output = Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(["doctor"])
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("mjutest runs");
    let stdout = String::from_utf8_lossy(&output.stdout);

    for tool in ["cargo", "rustc", "llvm-profdata", "llvm-cov", "git"] {
        assert!(stdout.contains(tool), "{tool} is not reported: {stdout}");
    }
    assert!(
        stdout.contains("required") && stdout.contains("optional"),
        "a reader must be able to tell what a missing line costs: {stdout}"
    );
    assert!(
        matches!(output.status.code(), Some(0 | 3)),
        "0 when a run could go ahead, 3 when it could not: {:?}",
        output.status.code()
    );
}

#[test]
fn doctor_without_a_toolchain_says_so_and_refuses_rather_than_guessing() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let output = Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(["doctor"])
        .current_dir(dir.path())
        .env_clear()
        .env("NO_COLOR", "1")
        .env("PATH", "/nonexistent")
        .output()
        .expect("mjutest runs");
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("missing"), "{stdout}");
}

// --- init --------------------------------------------------------------------------

#[test]
fn init_writes_a_skeleton_that_loads_as_exactly_the_defaults() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let output = mjutest_in(dir.path(), &["init"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let path = dir.path().join(mjutest_cli::config::FILE_NAME);
    let written = std::fs::read_to_string(&path).expect("the skeleton");
    assert_eq!(written, mjutest_cli::config::skeleton());
    assert_eq!(
        mjutest_cli::config::Config::load(dir.path()).expect("it loads"),
        mjutest_cli::config::Config::default(),
        "the untouched skeleton is the defaults, written down"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(mjutest_cli::config::FILE_NAME),
        "it says what it wrote"
    );
}

#[test]
fn init_refuses_to_write_over_a_configuration_somebody_edited() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(mjutest_cli::config::FILE_NAME);
    std::fs::write(&path, "version = 1\n").expect("their configuration");

    let output = mjutest_in(dir.path(), &["init"]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "version = 1\n",
        "refused without touching it"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("MJ1005"), "{stderr}");
    assert!(
        stderr.contains("--force"),
        "and says how to mean it: {stderr}"
    );
}

#[test]
fn init_force_replaces_what_is_there() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(mjutest_cli::config::FILE_NAME);
    std::fs::write(&path, "version = 1\n").expect("their configuration");

    let output = mjutest_in(dir.path(), &["init", "--force"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        mjutest_cli::config::skeleton()
    );
}
