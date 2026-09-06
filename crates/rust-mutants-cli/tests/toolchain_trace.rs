// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `--trace` records about a real run, and what `trace summary`, `trace check`, and `trace diff` say about a recording afterwards.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "the helpers that start the binary and read a recording are not themselves tests: a \
              recording that cannot be read leaves nothing to assert"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mjutest_devkit::fixture::Fixture;

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.current_dir(fixture.root());
    command.arg(args[0]);
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--offline", "--locked"]);
    command.args(&args[1..]);
    command.output().expect("rust-mutants runs")
}

/// A reading of a recording, which names its own root rather than taking one before the subcommand.
fn reading(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.current_dir(fixture.root());
    command.arg("trace");
    command.arg(args[0]);
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(&args[1..]);
    command.output().expect("rust-mutants runs")
}

/// Where a fixture's reports are stored.
fn reports(fixture: &Fixture) -> PathBuf {
    fixture.root().join("reports/mutation")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Every event of a recording, as the JSON a reader would parse.
fn recorded(directory: &Path) -> Vec<serde_json::Value> {
    let path = directory.join("trace.jsonl");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.lines()
        .map(|line| serde_json::from_str(line).expect("every line is an event"))
        .collect()
}

/// The one directory a run wrote its report into.
fn only_run(reports: &Path) -> PathBuf {
    let mut runs: Vec<PathBuf> = std::fs::read_dir(reports)
        .expect("reports")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("run-report-v1.json").is_file())
        .collect();
    runs.sort();
    runs.pop().expect("one stored run")
}

fn types(events: &[serde_json::Value]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| event.get("type")?.as_str().map(str::to_owned))
        .collect()
}

#[test]
fn run_with_trace_records_under_the_run_directory_and_ends_with_run_end() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["run", "--trace", "--tier", "balanced"]);
    let run = only_run(&reports(&fixture));
    let events = recorded(&run.join("trace"));
    let names = types(&events);
    assert_eq!(
        names.first().map(String::as_str),
        Some("run-start"),
        "{names:?}"
    );
    assert_eq!(
        names.last().map(String::as_str),
        Some("run-end"),
        "a run that finished says so: {names:?}"
    );
    let end = events.last().expect("run-end");
    assert_eq!(
        end.pointer("/run/outcome")
            .and_then(serde_json::Value::as_str),
        Some(if output.status.code() == Some(0) {
            "detected"
        } else {
            "undetected"
        }),
        "{end}"
    );
    assert_eq!(
        end.pointer("/run/events_dropped")
            .and_then(serde_json::Value::as_u64),
        Some(0),
        "{end}"
    );
    for wanted in ["open", "prepare", "build", "verify"] {
        assert!(
            events.iter().any(|event| {
                event.get("type").and_then(serde_json::Value::as_str) == Some("phase-end")
                    && event
                        .pointer("/phase/name")
                        .and_then(serde_json::Value::as_str)
                        == Some(wanted)
            }),
            "{wanted} left no phase: {names:?}"
        );
    }
}

#[test]
fn list_with_a_named_trace_directory_records_open_and_preview() {
    let fixture = Fixture::copy("fixture-simple");
    let directory = fixture.temp().join("named");
    let output = against(&fixture, &["list", "--trace", &directory.to_string_lossy()]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let events = recorded(&directory);
    let names = types(&events);
    assert!(names.contains(&"open".to_owned()), "{names:?}");
    assert!(names.contains(&"discover-file".to_owned()), "{names:?}");
    assert!(
        !names.contains(&"build".to_owned()),
        "list builds nothing: {names:?}"
    );
    assert_eq!(
        names.last().map(String::as_str),
        Some("run-end"),
        "{names:?}"
    );
}

#[test]
fn every_judged_mutant_leaves_one_route_record_and_an_unreached_one_leaves_no_exec() {
    let fixture = Fixture::copy("fixture-unreached");
    let output = against(
        &fixture,
        &["run", "--trace", "--coverage", "--tier", "balanced"],
    );
    let run = only_run(&reports(&fixture));
    let events = recorded(&run.join("trace"));
    let routes: Vec<&serde_json::Value> = events
        .iter()
        .filter(|event| event.get("type").and_then(serde_json::Value::as_str) == Some("route"))
        .collect();
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(run.join("run-report-v1.json")).expect("the report"),
    )
    .expect("a report");
    let judged = report
        .pointer("/mutants")
        .and_then(serde_json::Value::as_array)
        .expect("the rows")
        .len();
    assert_eq!(
        routes.len(),
        judged,
        "one route per judged mutant: {}",
        stdout(&output)
    );
    let unreached: Vec<&&serde_json::Value> = routes
        .iter()
        .filter(|route| {
            route
                .pointer("/route/granularity")
                .and_then(serde_json::Value::as_str)
                == Some("unreached")
        })
        .collect();
    assert!(
        !unreached.is_empty(),
        "the fixture exists for its unreached mutation: {routes:?}"
    );
    for route in unreached {
        let index = route
            .pointer("/route/index")
            .and_then(serde_json::Value::as_u64);
        assert!(
            route
                .pointer("/route/executed")
                .and_then(serde_json::Value::as_array)
                .is_none_or(Vec::is_empty),
            "an unreached mutant runs nothing: {route}"
        );
        assert!(
            !events.iter().any(|event| {
                event.get("type").and_then(serde_json::Value::as_str) == Some("mutant-exec")
                    && event
                        .pointer("/mutant/index")
                        .and_then(serde_json::Value::as_u64)
                        == index
            }),
            "an unreached mutant leaves no execution: {route}"
        );
    }
}

#[test]
fn trace_summary_reads_the_newest_run_and_names_the_slowest_command() {
    let fixture = Fixture::copy("fixture-simple");
    let run = against(&fixture, &["run", "--trace", "--tier", "balanced"]);
    assert!(
        run.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&run)
    );
    let output = reading(&fixture, &["summary"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("EVENTS\t"), "{text}");
    assert!(text.contains("OUTCOME\t"), "{text}");
    assert!(text.contains("PHASE\tprepare"), "{text}");
    assert!(text.contains("SLOWEST\t"), "{text}");
    assert!(text.contains("PROGRAM\tcargo"), "{text}");
}

#[test]
fn trace_check_exits_1_on_a_recording_without_run_end() {
    let fixture = Fixture::copy("fixture-simple");
    let directory = fixture.temp().join("named");
    let listed = against(&fixture, &["list", "--trace", &directory.to_string_lossy()]);
    assert_eq!(listed.status.code(), Some(0), "{}", stderr(&listed));
    let whole = reading(&fixture, &["check", "--dir", &directory.to_string_lossy()]);
    assert_eq!(whole.status.code(), Some(0), "{}", stdout(&whole));
    assert!(stdout(&whole).contains("COMPLETE"), "{}", stdout(&whole));

    let path = directory.join("trace.jsonl");
    let text = std::fs::read_to_string(&path).expect("the recording");
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.contains("\"run-end\""))
        .collect();
    std::fs::write(&path, format!("{}\n", kept.join("\n"))).expect("truncating the recording");
    let cut = reading(&fixture, &["check", "--dir", &directory.to_string_lossy()]);
    assert_eq!(cut.status.code(), Some(1), "{}", stdout(&cut));
    assert!(
        stdout(&cut).contains("does not end with run-end"),
        "{}",
        stdout(&cut)
    );
}

#[test]
fn trace_diff_between_two_runs_reports_the_moved_columns() {
    let fixture = Fixture::copy("fixture-simple");
    let first = against(
        &fixture,
        &["run", "--trace", "--no-coverage", "--tier", "balanced"],
    );
    assert!(
        first.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&first)
    );
    let second = against(
        &fixture,
        &[
            "run",
            "--trace",
            "--coverage",
            "--tier",
            "balanced",
            "--no-cache",
        ],
    );
    assert!(
        second.status.code().is_some_and(|code| code < 2),
        "{}",
        stderr(&second)
    );
    let stored = reports(&fixture);
    let mut runs: Vec<String> = std::fs::read_dir(&stored)
        .expect("reports")
        .flatten()
        .filter(|entry| entry.path().join("run-report-v1.json").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    runs.sort();
    assert_eq!(runs.len(), 2, "{runs:?}");
    let output = reading(&fixture, &["diff", &runs[0], &runs[1]]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains(&format!("A\t{}", runs[0])), "{text}");
    assert!(
        text.contains("CHANGED\troute block\t0\t"),
        "a measured run routes by block where an unmeasured one routes to everything: {text}"
    );
    assert!(text.contains("CHANGED\tphase-start\t"), "{text}");
    assert!(text.contains("CHANGED\tevents\t"), "{text}");
}

#[test]
fn a_trace_directory_that_cannot_be_created_costs_one_line_on_stderr_not_the_run() {
    let fixture = Fixture::copy("fixture-simple");
    let blocked = fixture.temp().join("blocked");
    std::fs::write(&blocked, "not a directory").expect("the file in the way");
    let output = against(&fixture, &["list", "--trace", &blocked.to_string_lossy()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a recording that could not be made is not the command: {}",
        stderr(&output)
    );
    let said = stderr(&output);
    assert_eq!(said.lines().count(), 1, "{said}");
    assert!(said.contains("not recording into"), "{said}");
    assert!(!stdout(&output).is_empty(), "the command still answered");
}

#[test]
fn a_recording_never_costs_a_stored_run_its_place_and_neither_grows_forever() {
    let fixture = Fixture::copy("fixture-simple");
    fixture.write(
        ".rust-mutants.toml",
        b"version = 1\n\n[reports]\nkeep = 2\n",
    );
    for _ in 0..3 {
        let output = against(&fixture, &["run", "--trace", "--tier", "balanced"]);
        assert!(
            output.status.code().is_some_and(|code| code < 2),
            "{}",
            stderr(&output)
        );
    }
    for _ in 0..3 {
        let output = against(
            &fixture,
            &["run", "--trace", "--no-report", "--tier", "balanced"],
        );
        assert!(
            output.status.code().is_some_and(|code| code < 2),
            "{}",
            stderr(&output)
        );
    }
    for _ in 0..3 {
        let output = against(&fixture, &["list", "--trace"]);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    }
    let stored = reports(&fixture);
    let runs: Vec<PathBuf> = std::fs::read_dir(&stored)
        .expect("reports")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("run-report-v1.json").is_file())
        .collect();
    assert_eq!(runs.len(), 2, "keep = 2 keeps two stored runs: {runs:?}");
    let recordings: Vec<PathBuf> = std::fs::read_dir(&stored)
        .expect("reports")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !path.join("run-report-v1.json").is_file())
        .filter(|path| path.file_name().is_some_and(|name| name != "traces"))
        .collect();
    assert!(
        recordings.len() <= 2,
        "a run that recorded and wrote no report is bounded by the same number rather than \
         growing forever: {recordings:?}"
    );
    let traces: Vec<PathBuf> = std::fs::read_dir(stored.join("traces"))
        .expect("the traces directory")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert!(
        traces.len() <= 2,
        "and so is every other command's recording: {traces:?}"
    );
}
