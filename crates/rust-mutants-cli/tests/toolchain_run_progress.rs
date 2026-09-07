// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is happening.

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

#[test]
fn a_plain_run_names_every_phase_every_mutant_and_the_tally_it_ends_with() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--ui", "plain"]);
    let text = said(&output);
    for phase in [
        "open", "pristine", "discover", "plan", "validate", "build", "verify",
    ] {
        assert!(
            text.lines().any(|line| line.starts_with(phase)),
            "a reader watching wants to know what preparing was doing: {phase} in {text}"
        );
    }
    assert!(
        text.lines().any(|line| line.starts_with("run  ")),
        "and how much there is to do: {text}"
    );
    let judged: Vec<&str> = text.lines().filter(|line| line.starts_with('[')).collect();
    assert_eq!(judged.len(), 11, "one line per mutant: {text}");
    assert!(
        judged
            .iter()
            .all(|line| line.contains("killed") || line.contains("survived")),
        "each says what the tests made of it: {judged:?}"
    );
    assert!(
        text.contains("killed ") && text.contains("survived ") && text.contains("elapsed "),
        "and the run ends with the tally it reached: {text}"
    );
}

#[test]
fn a_quiet_run_says_nothing_until_the_summary() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--ui", "quiet"]);
    let text = said(&output);
    assert!(
        !text.lines().any(|line| line.starts_with('[')),
        "nothing about one mutant: {text}"
    );
    assert!(
        text.contains("MUTANTS   cataloged="),
        "and the summary all the same: {text}"
    );
}

#[test]
fn colour_is_off_unless_it_is_asked_for_and_the_stream_can_take_it() {
    let fixture = Fixture::copy("fixture-simple");
    let plain = said(&run(&fixture, &["--ui", "plain"]));
    assert!(
        !plain.contains('\u{1b}'),
        "a pipe is not a terminal, and NO_COLOR is set besides: {plain:?}"
    );
    let painted = said(&run(&fixture, &["--ui", "plain", "--color", "always"]));
    assert!(
        painted.contains("\u{1b}[32mkilled\u{1b}[0m"),
        "asked for, it paints what a reader is scanning for: {painted:?}"
    );
    let never = said(&run(&fixture, &["--ui", "plain", "--color", "never"]));
    assert!(!never.contains('\u{1b}'), "{never:?}");
}
