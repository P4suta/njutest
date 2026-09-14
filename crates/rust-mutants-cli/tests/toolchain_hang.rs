// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mutation that never returns, and one a slow test leaves a run undecided about.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "the helpers that start the engine are not themselves tests, and a document this \
              test caused to be written is one it may index"
)]

use std::path::Path;
use std::process::Output;

use njutest_devkit::fixture::Fixture;

/// Runs the fixture whole, with whatever the test wants the test processes to see.
fn run(fixture: &Fixture, env: &[(&str, String)]) -> Output {
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    for (name, value) in env {
        command.env(name, value);
    }
    command.arg("run");
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    command.output().expect("rust-mutants runs")
}

/// The row of the mutation one rule proposed at one line.
fn row(fixture: &Fixture, rule: &str, line: u64) -> serde_json::Value {
    let directory = fixture.root().join("reports/mutation");
    let mut runs: Vec<std::path::PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .flatten()
        .map(|entry| entry.path().join("run-report-v1.json"))
        .filter(|path| path.is_file())
        .collect();
    runs.sort();
    let newest = runs.pop().expect("one stored run");
    read(&newest, rule, line)
}

fn read(report: &Path, rule: &str, line: u64) -> serde_json::Value {
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report).expect("the report"))
            .expect("the report is a document");
    document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .find(|row| row["rule"].as_str() == Some(rule) && row["line"].as_u64() == Some(line))
        .cloned()
        .unwrap_or_else(|| panic!("no row for {rule} at line {line}: {document}"))
}

#[test]
fn a_mutant_that_never_returns_is_timed_out_after_a_serial_retry() {
    let fixture = Fixture::copy("fixture-hang");
    let output = run(&fixture, &[]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stopped = row(&fixture, "delete-compound-assignment", 13);
    assert_eq!(
        stopped["outcome"].as_str(),
        Some("timed_out"),
        "deleting what ends the loop leaves a function that does not return: {stopped}"
    );
    assert_eq!(
        stopped["retried"].as_bool(),
        Some(true),
        "a timeout is believed only after it repeats on its own: {stopped}"
    );
    let ordinary = row(&fixture, "delete-compound-assignment", 12);
    assert_eq!(
        ordinary["outcome"].as_str(),
        Some("killed"),
        "the other compound assignment is one a test notices: {ordinary}"
    );
    assert_eq!(ordinary["retried"].as_bool(), Some(false), "{ordinary}");
}

#[test]
fn a_timeout_that_does_not_reproduce_is_inconclusive() {
    let fixture = Fixture::copy("fixture-hang");
    let markers = fixture.temp().join("markers");
    std::fs::create_dir_all(&markers).expect("the marker directory");
    let output = run(
        &fixture,
        &[
            (
                "FIXTURE_HANG_MARKER",
                markers.to_string_lossy().into_owned(),
            ),
            ("FIXTURE_HANG_PAUSE_MS", "4000".to_owned()),
        ],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let undecided = row(&fixture, "gt-to-ge", 25);
    assert_eq!(
        undecided["outcome"].as_str(),
        Some("inconclusive"),
        "a mutation that was slow once and quick again is one the run cannot decide: {undecided}"
    );
    assert_eq!(undecided["retried"].as_bool(), Some(true), "{undecided}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("inconclusive-mutant"),
        "a run that could not decide says so: {text}"
    );
    assert!(
        text.contains("timed out once and did not do so again"),
        "and says which of the two things left it undecided: {text}"
    );
    let still = row(&fixture, "delete-compound-assignment", 13);
    assert_eq!(
        still["outcome"].as_str(),
        Some("timed_out"),
        "a mutation that never returns is not a slow one: {still}"
    );
}

#[test]
fn a_mutation_that_never_returns_is_stopped_rather_than_left_running() {
    let fixture = Fixture::copy("fixture-hang");
    let output = run(&fixture, &[]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("timed_out=2"),
        "the run ends rather than waiting on the process it started: {text}"
    );
}
