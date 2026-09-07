// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as a program reads it: one JSON object per line, as it happens.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::process::{Command, Output};

use mjutest_devkit::fixture::Fixture;
use rust_mutants::report::stream::{Line, read};

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

#[test]
fn a_json_run_streams_one_object_per_event_as_it_happens() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--json"]);
    let text = String::from_utf8_lossy(&output.stdout);
    let lines = read(&text).expect("every line reads back through the engine's own reader");

    assert!(
        matches!(lines.first(), Some(Line::RunStart { schema, .. }) if schema == "rust-mutants-run-stream-v1"),
        "the first line says what the stream is: {:?}",
        lines.first()
    );
    assert!(
        matches!(lines.last(), Some(Line::RunEnd { exit_code: 1, .. })),
        "the last says how it ended: {:?}",
        lines.last()
    );
    let judged: Vec<&Line> = lines
        .iter()
        .filter(|line| matches!(line, Line::Mutant { .. }))
        .collect();
    assert_eq!(judged.len(), 11, "one line per mutant");
    let mut seen = 0;
    for line in &judged {
        let Line::Mutant {
            completed, mutant, ..
        } = line
        else {
            continue;
        };
        seen += 1;
        assert_eq!(*completed, seen, "the count is what has been delivered");
        assert!(
            !mutant.id.is_empty() && !mutant.rule.is_empty(),
            "{mutant:?}"
        );
        assert!(
            mutant.line > 0,
            "a reader is told where to look: {mutant:?}"
        );
    }
    assert!(
        lines
            .iter()
            .any(|line| matches!(line, Line::Finding { .. })),
        "and what stops the run from being clean"
    );
    assert!(
        lines
            .iter()
            .any(|line| matches!(line, Line::PhaseEnd { phase, .. } if phase == "verify")),
        "preparing is in the stream too: {lines:?}"
    );
}

#[test]
fn every_line_validates_against_the_schema_published_with_it() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--json"]);
    let text = String::from_utf8_lossy(&output.stdout);
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            mjutest_devkit::paths::workspace_root().join("schema/rust-mutants-run-stream-v1.json"),
        )
        .expect("the schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    for (at, line) in text.lines().filter(|line| !line.is_empty()).enumerate() {
        let value: serde_json::Value = serde_json::from_str(line).expect("a line of JSON");
        let problems: Vec<String> = validator
            .iter_errors(&value)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();
        assert!(problems.is_empty(), "line {}: {problems:?}", at + 1);
    }
}

#[test]
fn a_stream_and_a_display_are_two_ways_of_saying_one_thing_and_never_both() {
    let fixture = Fixture::copy("fixture-simple");
    let output = run(&fixture, &["--json", "--ui", "plain"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(complaint.contains("--ui"), "{complaint}");
}
