// SPDX-FileCopyrightText: 2026 njutest contributors
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
use std::process::Output;

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = fixture.root().to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied().take(1))
            .chain(["--root", root.as_str()])
            .chain(["--offline", "--locked"])
            .chain(args.iter().copied().skip(1))
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

/// A reading of a recording, which names its own root rather than taking one before the subcommand.
fn reading(fixture: &Fixture, args: &[&str]) -> Output {
    let root = fixture.root().to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["trace"])
            .chain(args.iter().copied().take(1))
            .chain(["--root", root.as_str()])
            .chain(args.iter().copied().skip(1))
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

/// Where a fixture's reports are stored.
fn reports(fixture: &Fixture) -> PathBuf {
    rust_mutants_cli::app::stored::Store::read(fixture.root()).root()
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
    let output = against(
        &fixture,
        &["list", &format!("--trace={}", directory.display())],
    );
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
    let listed = against(
        &fixture,
        &["list", &format!("--trace={}", directory.display())],
    );
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
        &[
            "run",
            "--trace",
            "--no-coverage",
            "--no-touch",
            "--tier",
            "balanced",
        ],
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
            "--no-touch",
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
    assert!(
        text.contains("CHANGED\troute all\t"),
        "and the run that measured nothing routed every mutation to every target: {text}"
    );
}

#[test]
fn a_trace_directory_that_cannot_be_created_costs_one_line_on_stderr_not_the_run() {
    let fixture = Fixture::copy("fixture-simple");
    let blocked = fixture.temp().join("blocked");
    std::fs::write(&blocked, "not a directory").expect("the file in the way");
    let output = against(
        &fixture,
        &["list", &format!("--trace={}", blocked.display())],
    );
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

#[test]
fn a_mutation_a_run_leaves_out_records_why_it_was_left_out() {
    let fixture = Fixture::copy("fixture-coverage");
    let output = against(
        &fixture,
        &["run", "--trace", "--rule", "le-to-lt", "--tier", "all"],
    );
    let run = only_run(&reports(&fixture));
    let events = recorded(&run.join("trace"));
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(run.join("run-report-v1.json")).expect("the report"),
    )
    .expect("a report");
    let unselected: Vec<String> = report
        .pointer("/mutants")
        .and_then(serde_json::Value::as_array)
        .expect("the rows")
        .iter()
        .filter(|one| one["not_run_reason"] == "unselected")
        .filter_map(|one| one["display_id"].as_str().map(str::to_owned))
        .collect();
    assert!(
        !unselected.is_empty(),
        "this run named one rule, so the rest of the catalog was left out: {}",
        stdout(&output)
    );
    let otherwise: Vec<String> = report
        .pointer("/mutants")
        .and_then(serde_json::Value::as_array)
        .expect("the rows")
        .iter()
        .filter(|one| one["outcome"] == "not_run" && one["not_run_reason"] != "unselected")
        .filter_map(|one| one["display_id"].as_str().map(str::to_owned))
        .collect();
    assert!(
        !otherwise.is_empty(),
        "and a proof removed others, which is a second reason to say why: {}",
        stdout(&output)
    );
    for named in unselected.into_iter().chain(otherwise) {
        assert!(
            events.iter().any(|event| {
                event.get("type").and_then(serde_json::Value::as_str) == Some("select")
                    && event
                        .pointer("/select/mutant")
                        .and_then(serde_json::Value::as_str)
                        == Some(named.as_str())
            }),
            "the record is the only place a reader sees why a mutation was not put to the \
             tests, so a run that leaves one out says so, whichever of its reasons it was: \
             {named}"
        );
    }
}

#[test]
fn a_filtered_run_compiler_validates_only_the_mutants_it_selected() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(
        &fixture,
        &[
            "run",
            "--trace",
            "--rule",
            "gt-to-ge",
            "--tier",
            "all",
            "--no-coverage",
            "--jobs",
            "1",
        ],
    );
    let run = only_run(&reports(&fixture));
    let events = recorded(&run.join("trace"));
    let guarded: Vec<u64> = events
        .iter()
        .filter(|event| event.get("type").and_then(serde_json::Value::as_str) == Some("instrument"))
        .filter_map(|event| event.pointer("/instrument/guards")?.as_u64())
        .filter(|guards| *guards > 0)
        .collect();
    assert!(
        !guarded.is_empty() && guarded.iter().all(|guards| *guards == 1),
        "the one selected mutation, rather than the complete catalog, is what every validation \
         attempt places in the tree: {guarded:?}\n{}{}",
        stdout(&output),
        stderr(&output)
    );

    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(run.join("run-report-v1.json")).expect("the report"),
    )
    .expect("a report");
    let selected: Vec<u64> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["not_run_reason"] != "unselected")
        .filter_map(|mutant| mutant["index"].as_u64())
        .collect();
    let witnessed: Vec<u64> = events
        .iter()
        .filter(|event| event.get("type").and_then(serde_json::Value::as_str) == Some("witness"))
        .filter_map(|event| event.pointer("/witness/index")?.as_u64())
        .collect();
    assert_eq!(
        selected, witnessed,
        "the proof compiler pass asks about the selected candidate only: {events:?}"
    );
    assert_eq!(report["accounting"]["cataloged"], 11, "{report}");
    assert_eq!(
        report["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|mutant| mutant["not_run_reason"] == "unselected")
            .count(),
        10,
        "the candidates not compiled remain visible as an explicit selection decision: {report}"
    );
}

#[test]
fn a_mutation_a_run_puts_to_the_tests_leaves_the_execution_that_ran_it() {
    let fixture = Fixture::copy("fixture-unreached");
    let output = against(&fixture, &["run", "--trace", "--tier", "balanced"]);
    let run = only_run(&reports(&fixture));
    let events = recorded(&run.join("trace"));
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(run.join("run-report-v1.json")).expect("the report"),
    )
    .expect("a report");
    let executed: Vec<u64> = report
        .pointer("/mutants")
        .and_then(serde_json::Value::as_array)
        .expect("the rows")
        .iter()
        .filter(|one| one["outcome"] != "not_run")
        .filter_map(|one| one["index"].as_u64())
        .collect();
    assert!(
        !executed.is_empty(),
        "this run put some of them to the tests: {}",
        stdout(&output)
    );
    for index in executed {
        assert!(
            events.iter().any(|event| {
                event.get("type").and_then(serde_json::Value::as_str) == Some("mutant-exec")
                    && event
                        .pointer("/mutant/index")
                        .and_then(serde_json::Value::as_u64)
                        == Some(index)
            }),
            "a mutation that ran leaves the execution that ran it, or a recording says a run \
             removed work it did: {index}"
        );
    }
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}
