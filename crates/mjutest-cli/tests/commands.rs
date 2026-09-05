// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind: `report`, `trace`,
//! `diagnostics`, and the one that says what a run would do without doing
//! it, `plan`.
//!
//! All four are about a run that already happened, or one that has not
//! happened yet, so none of them may invent anything. A command asked about
//! a run that is not there says so and exits 3 rather than answering about
//! some other run.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("mjutest-commands-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy(&source, &root);
    Fixture { root, _dir: dir }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the directory");
    for entry in std::fs::read_dir(from).expect("the fixture") {
        let entry = entry.expect("an entry");
        let kind = entry.file_type().expect("a file type");
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy(&entry.path(), &target);
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target).expect("a copy");
        }
    }
}

fn mjutest(fixture: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(args)
        .current_dir(&fixture.root)
        .env_clear()
        .env("NO_COLOR", "1")
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        }))
        .output()
        .expect("mjutest runs")
}

/// A fixture with one completed, traced run behind it.
fn verified(name: &str) -> Fixture {
    let fixture = fixture(name);
    let output = mjutest(&fixture, &["verify", "--offline", "--locked", "--trace"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// --- report --------------------------------------------------------------------------

#[test]
fn report_prints_the_records_of_the_latest_run() {
    let fixture = verified("fixture-baseline");
    let output = mjutest(&fixture, &["report"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.starts_with("RUN\t"), "{text}");
    assert!(text.ends_with("VERDICT\tINSUFFICIENT\n"), "{text}");
}

#[test]
fn report_json_prints_the_document_the_run_wrote_byte_for_byte() {
    let fixture = verified("fixture-baseline");
    let output = mjutest(&fixture, &["report", "--format", "json"]);
    assert_eq!(output.status.code(), Some(0));

    let index = fixture.root.join(mjutest_cli::app::reports::LATEST_ANY);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index).expect("the index")).expect("JSON");
    let path = fixture
        .root
        .join(value["directory"].as_str().expect("a directory"))
        .join(mjutest_cli::app::reports::DOCUMENT_NAME);
    assert_eq!(
        stdout(&output),
        std::fs::read_to_string(path).expect("the document"),
        "a reader piping this and a reader opening the file see the same bytes"
    );
}

#[test]
fn report_names_a_run_that_is_not_there_rather_than_answering_about_another() {
    let fixture = verified("fixture-baseline");
    let output = mjutest(&fixture, &["report", "20200101T000000Z-000000"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("20200101T000000Z-000000"), "{stderr}");
    assert!(stderr.contains("MJ6005"), "{stderr}");
}

#[test]
fn report_without_a_run_at_all_says_so() {
    let fixture = fixture("fixture-baseline");
    let output = mjutest(&fixture, &["report"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("MJ6005"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// --- trace ---------------------------------------------------------------------------

#[test]
fn trace_summary_counts_the_events_and_finds_nothing_wrong_with_a_complete_recording() {
    let fixture = verified("fixture-baseline");
    let output = mjutest(&fixture, &["trace", "summary"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("run-start"), "{text}");
    assert!(text.contains("exec"), "{text}");
    assert!(text.contains("phase"), "{text}");
    assert!(
        text.contains("no problems"),
        "a complete recording has none: {text}"
    );
}

#[test]
fn trace_summary_reports_a_recording_that_lost_its_end() {
    let fixture = verified("fixture-baseline");
    let stream = trace_stream(&fixture);
    let text = std::fs::read_to_string(&stream).expect("the stream");
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.contains("\"run-end\""))
        .collect();
    let mut cut = kept.join("\n");
    cut.push('\n');
    std::fs::write(&stream, cut).expect("a truncated recording");

    let output = mjutest(&fixture, &["trace", "summary"]);
    assert_eq!(output.status.code(), Some(2), "a problem is not a success");
    assert!(stdout(&output).contains("run-end"), "{}", stdout(&output));
}

#[test]
fn trace_diff_says_which_phases_moved() {
    let fixture = verified("fixture-baseline");
    let first = only_recording(&fixture);
    let second = mjutest(&fixture, &["verify", "--offline", "--locked", "--trace"]);
    assert_eq!(second.status.code(), Some(2));
    let names = recordings(&fixture);
    assert_eq!(names.len(), 2, "{names:?}");

    let other = names
        .iter()
        .find(|name| *name != &first)
        .expect("the second recording");
    let output = mjutest(&fixture, &["trace", "diff", &first, other]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("baseline"), "{text}");
    assert!(text.contains(&first), "{text}");
}

fn recordings(fixture: &Fixture) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixture.root.join(".mjutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn only_recording(fixture: &Fixture) -> String {
    let names = recordings(fixture);
    assert_eq!(names.len(), 1, "{names:?}");
    names[0].clone()
}

fn trace_stream(fixture: &Fixture) -> PathBuf {
    fixture
        .root
        .join(".mjutest/trace")
        .join(only_recording(fixture))
        .join(mjutest_cli::trace::FILE_NAME)
}

// --- diagnostics ---------------------------------------------------------------------

#[test]
fn diagnostics_bundles_the_report_and_the_recording_of_one_run() {
    let fixture = verified("fixture-baseline");
    let run = only_recording(&fixture);
    let output = mjutest(&fixture, &["diagnostics", &run]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle = fixture.root.join(".mjutest/diagnostics").join(&run);
    assert!(bundle.is_dir(), "{}", bundle.display());
    assert!(
        bundle
            .join(mjutest_cli::app::reports::DOCUMENT_NAME)
            .is_file()
    );
    assert!(bundle.join(mjutest_cli::trace::FILE_NAME).is_file());
    assert!(
        bundle.join("bundle.json").is_file(),
        "what the bundle holds and what it could not find"
    );
    assert!(
        stdout(&output).contains(&bundle.display().to_string()),
        "it says where it put it: {}",
        stdout(&output)
    );
}

#[test]
fn diagnostics_of_a_run_that_never_happened_is_an_error() {
    let fixture = verified("fixture-baseline");
    let output = mjutest(&fixture, &["diagnostics", "20200101T000000Z-000000"]);
    assert_eq!(output.status.code(), Some(3));
}

// --- plan ----------------------------------------------------------------------------

#[test]
fn plan_names_every_target_a_run_would_measure_without_measuring_one() {
    let fixture = fixture("fixture-baseline");
    let output = mjutest(&fixture, &["plan", "--offline", "--locked"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("sign_names_both_sides_of_zero"), "{text}");
    assert!(text.contains("doubling_is_addition_twice"), "{text}");
    assert!(text.contains("TARGETS\t3"), "{text}");
    assert!(
        !fixture.root.join("reports").exists(),
        "a plan is not a run: it writes no report"
    );
}

#[test]
fn plan_why_says_what_put_each_target_in_scope() {
    let fixture = fixture("fixture-baseline");
    let output = mjutest(&fixture, &["plan", "--offline", "--locked", "--why"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(
        text.contains("every workspace member"),
        "the scope a reader did not have to guess: {text}"
    );
    assert!(
        text.contains("ignored"),
        "and which targets libtest will not run: {text}"
    );
}
