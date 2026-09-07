// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind, what a later command finds, and what puts one finding back to the tests.

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
    command.output().expect("rust-mutants runs")
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_run_can_be_named_and_its_report_is_called_that() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
            "--run-id",
            "monday",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        fixture
            .root()
            .join("reports/mutation/monday/run-report-v1.json")
            .is_file(),
        "a run a person named is a run they can find again: {}",
        said(&output)
    );
    let refused = against(
        &fixture,
        &["run", "--offline", "--locked", "--run-id", "../escape"],
    );
    assert_eq!(refused.status.code(), Some(2), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--run-id"),
        "{refused:?}"
    );
}

#[test]
fn cache_says_what_the_store_holds_and_clear_outcomes_empties_it() {
    let fixture = Fixture::copy("fixture-simple");
    let _measured = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
        ],
    );
    let listed = said(&against(&fixture, &["cache"]));
    assert!(listed.contains("outcomes    "), "{listed}");
    assert!(listed.contains("kept        0"), "{listed}");
    let cleared = said(&against(&fixture, &["cache", "--clear-outcomes"]));
    assert!(
        cleared.contains("outcomes    ") && cleared.contains("removed"),
        "a store emptied says how much was in it: {cleared}"
    );
    let after = said(&against(&fixture, &["cache"]));
    assert!(after.contains("outcomes    0 records"), "{after}");
}

#[test]
fn a_kept_snapshot_outlives_the_run_and_cache_names_the_run_that_kept_it() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
            "--keep-temp",
            "--run-id",
            "kept-one",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let listed = said(&against(&fixture, &["cache"]));
    assert!(
        listed.contains("(kept-one)"),
        "a directory a run was told to keep is one a later command can find: {listed}"
    );
    let swept = said(&against(&fixture, &["cache", "--gc"]));
    assert!(
        swept.contains("(kept-one)"),
        "and a sweep leaves it alone unless asked: {swept}"
    );
    let removed = said(&against(&fixture, &["cache", "--gc", "--kept"]));
    assert!(
        removed.contains("kept        2 removed"),
        "the snapshot and the build cache are two directories one run kept: {removed}"
    );
    let after = said(&against(&fixture, &["cache"]));
    assert!(after.contains("kept        0"), "{after}");
}

#[test]
fn replaying_a_recorded_outcome_asks_the_question_the_run_asked() {
    let fixture = Fixture::copy("fixture-simple");
    let _measured = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
        ],
    );
    let output = against(
        &fixture,
        &["replay", "--offline", "--locked", "--tier", "all", "e5e8"],
    );
    let text = said(&output);
    assert!(
        text.contains("REPLAY    e5e872bfbcb2afbbf7a1 still survived"),
        "a replay says whether the answer is still the same: {text}"
    );
    assert_eq!(output.status.code(), Some(1), "{text}");
}
