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
    command.args(["--root", njutest_devkit::paths::utf8(fixture.root())]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    command.output().expect("rust-mutants runs")
}

/// What to add to a failure when the row says the clock answered instead of the count, and nothing when it does not.
fn outran_by_the_clock(row: &serde_json::Value) -> String {
    if row["outcome"].as_str() != Some("waited") || !row["step_notice"].is_null() {
        return String::new();
    }
    format!(
        "\n\nThe clock answered before the count did after {duration}ms, and left no step \
         notice. A counting execution is only stopped by the clock when it raises no \
         boundary for a whole bound, which says the loop stopped taking its guard, or when \
         it reaches the ceiling of ten bounds still counting, which says a take cost more \
         than a tenth of a bound over ten; that one is this machine, and the number to \
         lower is `[mutation] steps` in fixtures/fixture-hang/.rust-mutants.toml",
        duration = row["duration_ms"].as_u64().unwrap_or_default()
    )
}

/// The row of the mutation one rule proposed at one line.
fn row(fixture: &Fixture, rule: &str, line: u64) -> serde_json::Value {
    let newest = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    )
    .join("run-report-v1.json");
    read(&newest, rule, line)
}

fn read(report: &Path, rule: &str, line: u64) -> serde_json::Value {
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(report).expect("the report"),
    )
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
fn a_mutant_that_never_returns_is_stopped_by_a_count_and_not_asked_again() {
    let fixture = Fixture::copy("fixture-hang");
    let output = run(&fixture, &[]);
    assert!(
        output.status.code() == Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let stopped = row(&fixture, "delete-compound-assignment", 13);
    assert_eq!(
        stopped["outcome"].as_str(),
        Some("step_limit_reached"),
        "deleting what ends the loop reaches the guard allowance. The report preserves \
         that execution fact without claiming it proved nontermination.{}: {stopped}",
        outran_by_the_clock(&stopped)
    );
    assert_eq!(stopped["step_notice"]["limit"], 10);
    assert!(
        stopped["step_notice"]["observed"]
            .as_u64()
            .is_some_and(|observed| observed > 10),
        "the boundary names the first count beyond the allowance: {stopped}"
    );
    assert_eq!(
        stopped["retried"].as_bool(),
        Some(false),
        "and it is not asked again: the serial retry exists because a clock is unreliable, \
         and a count every machine agrees on cannot disagree with itself: {stopped}"
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
                njutest_devkit::paths::utf8(&markers).to_owned(),
            ),
            ("FIXTURE_HANG_PAUSE_MS", "4000".to_owned()),
        ],
    );
    assert!(
        output.status.code() == Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let undecided = row(&fixture, "gt-to-ge", 25);
    assert_eq!(
        undecided["outcome"].as_str(),
        Some("inconclusive"),
        "a mutation that was slow once and quick again is one the run cannot decide: {undecided}"
    );
    assert_eq!(undecided["retried"].as_bool(), Some(true), "{undecided}");
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
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
        Some("step_limit_reached"),
        "the finite count and clock facts remain distinct without turning either into a \
         proof that the mutation cannot terminate: {still}"
    );
}

#[test]
fn a_test_slower_than_the_bound_is_waited_for_while_it_keeps_moving() {
    let fixture = Fixture::copy("fixture-hang");
    std::fs::write(
        fixture.root().join(".rust-mutants.toml"),
        "version = 1\n\n[mutation]\ntimeout = \"5s\"\nsteps = 1000\n",
    )
    .expect("the five-second bound");
    let output = run(&fixture, &[("FIXTURE_HANG_STRIDE_MS", "50".to_owned())]);
    assert!(
        output.status.code() == Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let moving = row(&fixture, "gt-to-ge", 25);
    assert_eq!(
        moving["outcome"].as_str(),
        Some("survived"),
        "a test that passes through the mutated site every fifty milliseconds for ten \
         seconds is slower than the five-second bound and never quiet for one, so the run \
         waits for it and it answers: {moving}"
    );
    assert_eq!(moving["retried"].as_bool(), Some(false), "{moving}");
}

#[test]
fn a_mutation_that_never_returns_is_stopped_rather_than_left_running() {
    let fixture = Fixture::copy("fixture-hang");
    let output = run(&fixture, &[]);
    assert!(
        output.status.code() == Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        text.contains("step_limit_reached=3") && text.contains("waited=0"),
        "the run ends rather than waiting on the process it started, and what ended it is \
         a count rather than this machine's clock.{}: {text}",
        outran_by_the_clock(&row(&fixture, "delete-compound-assignment", 13))
    );
}

#[test]
fn a_mutation_outside_a_loop_in_a_file_nothing_mutates_is_counted_at_the_boundary() {
    let fixture = Fixture::copy("fixture-hang");
    let output = run(&fixture, &[]);
    assert!(
        output.status.code() == Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let stopped = row(&fixture, "int-decrement", 33);
    assert_eq!(
        stopped["outcome"].as_str(),
        Some("step_limit_reached"),
        "a stride of zero never ends the loop in src/walk.rs, which the run does not mutate, \
         and the checkpoints across the crate count it there.{}: {stopped}",
        outran_by_the_clock(&stopped)
    );
    assert_eq!(
        (
            &stopped["step_notice"]["limit"],
            &stopped["step_notice"]["observed"]
        ),
        (&serde_json::json!(10), &serde_json::json!(11)),
        "the count ends it at exactly one past the allowance: {stopped}"
    );
    assert_eq!(stopped["retried"].as_bool(), Some(false), "{stopped}");
    let newest = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    )
    .join("run-report-v1.json");
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(newest).expect("the report"),
    )
    .expect("the report is a document");
    assert!(
        document["mutants"]
            .as_array()
            .expect("the rows")
            .iter()
            .all(|row| row["path"].as_str() != Some("src/walk.rs")),
        "the loop's own file is one the run does not mutate"
    );
}
