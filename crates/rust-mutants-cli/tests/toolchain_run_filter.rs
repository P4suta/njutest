// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Narrowing a run: which mutants it is about, when it stops, and what it would cost.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::process::{Command, Output};

use mjutest_devkit::fixture::Fixture;

fn run(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--tier", "all"]);
    command.args(["--offline", "--locked", "--no-coverage", "--jobs", "1"]);
    command.args(extra);
    command.output().expect("rust-mutants runs")
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The outcome of every mutant the lines named.
fn judged(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.strip_prefix('['))
        .filter_map(|line| line.split_once("] "))
        .filter_map(|(_, rest)| rest.split_once(' '))
        .map(|(id, rest)| {
            (
                id.to_owned(),
                rest.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_filtered_out_mutant_is_not_run_for_the_stated_reason_and_is_not_a_finding() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--ui", "plain", "--rule", "gt-to-ge"]);
    let text = said(&output);
    assert_eq!(
        judged(&text).len(),
        1,
        "only what the rule names ran: {text}"
    );
    assert!(
        text.contains("not_run=11"),
        "and the rest are accounted for rather than left out: {text}"
    );
    assert!(
        !text.contains("not-run-mutant"),
        "a mutant nobody selected is not a hole in the tests: {text}"
    );
    assert!(
        text.contains("discharged-mutant"),
        "a mutant nobody ran because a proof said running it establishes nothing is: {text}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "the one that was selected is a gap in the tests, proved rather than measured: {text}"
    );
}

#[test]
fn a_family_a_file_and_an_identity_each_narrow_the_same_way() {
    let fixture = Fixture::copy("fixture-simple");
    let by_family = said(&run(&fixture, &["--ui", "plain", "--family", "comparison"]));
    assert_eq!(judged(&by_family).len(), 2, "{by_family}");
    let by_lines = said(&run(
        &fixture,
        &["--ui", "plain", "--file", "src/lib.rs:16-16"],
    ));
    assert_eq!(judged(&by_lines).len(), 6, "{by_lines}");
    let skipped = said(&run(
        &fixture,
        &["--ui", "plain", "--skip-family", "literal"],
    ));
    assert_eq!(judged(&skipped).len(), 8, "{skipped}");
    let bad = run(&fixture, &["--file", "src/lib.rs:nine"]);
    assert_eq!(bad.status.code(), Some(2), "{bad:?}");
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("--file"),
        "a value a flag cannot take names the flag: {bad:?}"
    );
}

#[test]
fn fail_fast_stops_at_the_first_finding_and_states_why_the_rest_did_not_run() {
    let fixture = Fixture::copy("fixture-coverage");
    let output = run(&fixture, &["--ui", "plain", "--fail-fast"]);
    let text = said(&output);
    let judged = judged(&text);
    assert!(
        judged
            .iter()
            .any(|(_, outcome)| outcome == "survived" || outcome == "not_run"),
        "it stopped at something a reader has to act on — a mutation the tests did not \
         notice, or one a proof says they could not have: {text}"
    );
    assert!(
        judged.len() < 14,
        "and did not measure the rest: {} of 14",
        judged.len()
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        !text.contains("INTERRUPTED"),
        "a run that stopped because it was asked to is not one that was killed: {text}"
    );
}

#[test]
fn a_dry_run_says_what_it_would_cost_without_executing_a_mutant() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--dry-run"]);
    let text = said(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(
        text.contains("WOULD START"),
        "it says the size of the job in work rather than in time: {text}"
    );
    assert!(
        text.contains("(11 mutants against"),
        "it says how many mutants and how many targets: {text}"
    );
    assert!(
        text.contains("REMOVED BY"),
        "and what a proof already took off the bill: {text}"
    );
    assert!(
        text.lines().filter(|line| line.starts_with('#')).count() == 11,
        "and what each one is: {text}"
    );
    let roughly = text
        .lines()
        .position(|line| line.starts_with("ROUGHLY"))
        .expect("a guess at the time");
    let would = text
        .lines()
        .position(|line| line.starts_with("WOULD START"))
        .expect("a count of the work");
    assert!(
        would < roughly,
        "the count comes first and the guess about this machine comes last: {text}"
    );
    assert!(
        !text.contains("OUTCOMES"),
        "nothing was executed, so there is nothing to report: {text}"
    );
}

#[test]
fn from_report_reruns_what_the_last_run_left() {
    let fixture = Fixture::copy("fixture-coverage");
    let first = run(&fixture, &["--ui", "plain"]);
    assert_eq!(first.status.code(), Some(1), "{first:?}");
    let left = judged(&said(&first))
        .into_iter()
        .filter(|(_, outcome)| outcome == "survived")
        .count();
    assert!(left > 0, "the fixture leaves something to measure again");
    let again = run(&fixture, &["--ui", "plain", "--from-report", "--no-cache"]);
    let text = said(&again);
    assert_eq!(
        judged(&text).len(),
        left,
        "only what the last run left is measured again: {text}"
    );
}

#[test]
fn from_report_that_names_nothing_measures_nothing_rather_than_everything() {
    let fixture = Fixture::copy("fixture-simple");
    let first = run(&fixture, &["--ui", "quiet"]);
    assert_eq!(first.status.code(), Some(1), "{first:?}");
    let again = run(&fixture, &["--ui", "plain", "--from-report", "--no-cache"]);
    let text = said(&again);
    assert_eq!(
        judged(&text).len(),
        0,
        "a run whose survivors the last one has none of is a run with nothing to measure \
         again, not a run of the whole catalog: {text}"
    );
    assert!(
        text.contains("not_run=11"),
        "and every mutant says why it was not measured: {text}"
    );
    assert_eq!(again.status.code(), Some(0), "{text}");
}
