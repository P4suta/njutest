// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as a program reads it: one JSON object per line, as it happens.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::Path;
use std::process::Output;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::report::stream::{Line, read};

fn run(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut command = mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
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

#[test]
fn the_stream_opens_before_anything_is_prepared() {
    let fixture = Fixture::copy("fixture-simple");
    let path = fixture.temp().join("stream.jsonl");
    let file = std::fs::File::create(&path).expect("somewhere to stream to");
    let mut child = mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")))
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .args(["run", "--json", "--offline", "--locked", "--no-coverage"])
        .args(["--root", &fixture.root().to_string_lossy()])
        .stdout(std::process::Stdio::from(file))
        .spawn()
        .expect("rust-mutants starts");
    let opened = std::time::Instant::now();
    let first = loop {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if let Some(line) = text.lines().next() {
            break line.to_owned();
        }
        assert!(
            opened.elapsed() < std::time::Duration::from_secs(120),
            "a consumer that hears nothing cannot tell a slow run from a hung one"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let line: serde_json::Value = serde_json::from_str(&first).expect("the first line is JSON");
    assert_eq!(
        line.get("type").and_then(serde_json::Value::as_str),
        Some("run-start"),
        "{first}"
    );
    let _finished = child.wait().expect("rust-mutants finishes");
}

#[test]
fn a_run_started_from_inside_the_tree_still_names_the_tree() {
    let fixture = Fixture::copy("fixture-simple");
    let output = mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")))
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .current_dir(fixture.root())
        .args(["run", "--json", "--offline", "--locked", "--no-coverage"])
        .args(["--root", "."])
        .output()
        .expect("rust-mutants runs");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let first = text.lines().next().expect("the stream opens");
    let line: serde_json::Value = serde_json::from_str(first).expect("the first line is JSON");
    assert_eq!(
        line.get("root_name").and_then(serde_json::Value::as_str),
        Some("fixture-simple"),
        "a root spelled as a dot is still a directory with a name: {first}"
    );
}
