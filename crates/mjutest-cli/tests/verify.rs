// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest verify`, end to end, against a real workspace.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A throwaway copy of a fixture, so the run writes its reports somewhere nothing else is reading.
struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("mjutest-verify-")
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

fn verify(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["verify", "--offline", "--locked"];
    args.extend_from_slice(extra);
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(args)
        .current_dir(&fixture.root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env("XDG_CACHE_HOME", fixture.root.join(".cache"))
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        }))
        .output()
        .expect("mjutest runs")
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let index = fixture.root.join(mjutest_cli::app::reports::LATEST_ANY);
    let text = std::fs::read_to_string(&index).expect("the latest index");
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let directory = value["directory"].as_str().expect("a directory");
    let path = fixture
        .root
        .join(directory)
        .join(mjutest_cli::app::reports::DOCUMENT_NAME);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).expect("the report is JSON")
}

#[test]
fn a_suite_with_a_gap_it_cannot_see_is_insufficient() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.ends_with("VERDICT\tINSUFFICIENT\n"),
        "the verdict is the last record: {stdout}"
    );
    assert!(
        stdout.contains("FINDING\tsurviving-mutant"),
        "and the report names what nobody noticed: {stdout}"
    );
}

#[test]
fn the_report_is_written_where_a_reader_will_look_and_validates_against_the_schema() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(2));

    let report = document(&fixture);
    let schema_path =
        mjutest_devkit::paths::workspace_root().join("schema/mjutest-assurance-report-v1.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(schema_path).expect("the schema"))
            .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let problems: Vec<String> = validator
        .iter_errors(&report)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect();
    assert!(problems.is_empty(), "{problems:?}");

    assert_eq!(report["verdict"], "INSUFFICIENT");
    assert_eq!(report["accounting"]["targets"]["selected"], 3);
    assert_eq!(report["accounting"]["targets"]["passed"], 2);
    assert_eq!(report["accounting"]["targets"]["skipped"], 1);
    assert_eq!(report["findings"].as_array().expect("findings").len(), 2);
    assert_eq!(
        report["toolchain"]["target"].as_str().unwrap_or_default(),
        report["toolchain"]["target"].as_str().unwrap_or("x"),
        "the triple is recorded"
    );
    assert!(
        report["repository"]["configuration_digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "the effective configuration is identified: {report}"
    );
}

#[test]
fn the_targets_are_named_and_ordered_slowest_first() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let report = document(&fixture);
    let targets = report["targets"].as_array().expect("targets");
    assert_eq!(targets.len(), 3);

    let durations: Vec<u64> = targets
        .iter()
        .map(|target| target["duration_ms"].as_u64().unwrap_or_default())
        .collect();
    let mut sorted = durations.clone();
    sorted.sort_unstable();
    sorted.reverse();
    assert_eq!(durations, sorted, "slowest first: {durations:?}");

    let names: Vec<&str> = targets
        .iter()
        .filter_map(|target| target["name"].as_str())
        .collect();
    assert!(
        names.iter().any(|name| name.contains("sign_names")),
        "{names:?}"
    );
}

#[test]
fn a_run_that_asked_for_a_trace_leaves_one_that_reads_back() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &["--trace"]).status.code(), Some(2));

    let traces = fixture.root.join(".mjutest/trace");
    let recording = std::fs::read_dir(&traces)
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path())
        .next()
        .expect("one recording");
    let stream = recording.join(mjutest_cli::trace::FILE_NAME);
    let events = mjutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&stream).expect("the stream"),
    ))
    .expect("the events read back");

    let problems = mjutest_cli::trace::check(&events);
    assert!(problems.is_empty(), "{problems:?}");
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.payload.type_name())
        .collect();
    assert_eq!(kinds.first(), Some(&"run-start"));
    assert_eq!(kinds.last(), Some(&"run-end"));
    assert!(kinds.contains(&"exec"), "{kinds:?}");
    assert!(kinds.contains(&"phase-end"), "{kinds:?}");
}

#[test]
fn progress_goes_to_the_error_stream_so_a_redirected_report_is_a_report() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("== baseline"), "{stderr}");
    assert!(
        !stdout.contains("== baseline"),
        "the output stream carries the report alone: {stdout}"
    );
    for line in stdout.lines() {
        let kind = line.split('\t').next().unwrap_or_default();
        assert_eq!(kind, kind.to_uppercase(), "not a record: {line:?}");
    }
}

#[test]
fn the_jsonl_interface_writes_one_object_per_line() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &["--ui", "jsonl"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.trim().is_empty(), "it said something");
    for line in stderr.lines() {
        let value: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|error| panic!("{line:?}: {error}"));
        assert!(value.get("type").is_some(), "{line}");
    }
}

#[test]
fn a_workspace_that_does_not_compile_is_a_defect_that_names_itself() {
    let fixture = fixture("fixture-baseline");
    std::fs::write(
        fixture.root.join("src/lib.rs"),
        b"// SPDX-FileCopyrightText: 2026 mjutest contributors\n\
          // SPDX-License-Identifier: MIT OR Apache-2.0\n\
          //! Broken on purpose.\npub fn sign() -> i32 { \"not an integer\" }\n",
    )
    .expect("break it");

    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = document(&fixture);
    assert_eq!(report["verdict"], "DEFECT");
    let findings = report["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["kind"], "build-failure");
    assert!(
        findings[0]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("mismatched types")),
        "the compiler's own words: {findings:?}"
    );
}

#[test]
fn the_report_of_a_known_workspace_is_the_recorded_one() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(2));

    let normalized = mjutest_devkit::report::normalize(&document(&fixture));
    let mut text = serde_json::to_string_pretty(&normalized).expect("one document");
    text.push('\n');
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/verify.golden.json");
    mjutest_devkit::golden::golden(&golden, text.as_bytes()).expect("the recorded report");
}

#[test]
fn a_workspace_with_no_tests_at_all_observed_nothing_and_says_so() {
    let repo = mjutest_devkit::repo::Repo::new();
    repo.package("silent")
        .lib("/// Nothing tests this.\npub const fn one() -> i32 {\n    1\n}\n");
    let fixture = Fixture {
        root: repo.root().to_path_buf(),
        _dir: tempfile::Builder::new()
            .prefix("mjutest-unused-")
            .tempdir()
            .expect("a temporary directory"),
    };
    let output = verify(&fixture, &[]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TARGETS\tselected=0"), "{stdout}");
    assert!(
        stdout.ends_with("VERDICT\tINSUFFICIENT\n"),
        "a suite with nothing in it assures nothing: {stdout}"
    );
    drop(repo);
}
