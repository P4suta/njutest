// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The phase the program exists for, end to end.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document by the names \
              its own fixture put there"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("mjutest-mutation-")
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
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index).expect("the index")).expect("JSON");
    let path = fixture
        .root
        .join(value["directory"].as_str().expect("a directory"))
        .join(mjutest_cli::app::reports::DOCUMENT_NAME);
    serde_json::from_str(&std::fs::read_to_string(path).expect("the document")).expect("JSON")
}

#[test]
fn a_suite_that_notices_every_change_is_assured() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = document(&fixture);
    assert_eq!(report["verdict"], "ASSURED");
    let mutants = &report["accounting"]["mutants"];
    assert_eq!(mutants["cataloged"], 4);
    assert_eq!(mutants["executed"], 4);
    assert_eq!(mutants["killed"], 4);
    assert_eq!(mutants["survived"], 0);
    assert_eq!(mutants["unreached"], 0);
    assert_eq!(report["findings"].as_array().expect("findings").len(), 0);

    for mutant in report["mutants"].as_array().expect("mutants") {
        assert_eq!(mutant["outcome"], "killed", "{mutant}");
        assert!(
            mutant["killed_by"]
                .as_str()
                .is_some_and(|by| !by.is_empty()),
            "a kill names the test that noticed: {mutant}"
        );
    }
}

#[test]
fn a_gap_the_suite_cannot_see_is_insufficient_and_named() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = document(&fixture);
    assert_eq!(report["verdict"], "INSUFFICIENT");
    let mutants = &report["accounting"]["mutants"];
    assert_eq!(mutants["cataloged"], 7);
    assert_eq!(mutants["killed"], 5);
    assert_eq!(
        mutants["survived"], 2,
        "the two boundary mutations only the ignored test would have caught"
    );

    let findings = report["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 2);
    let rules: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["outcome"] == "survived")
        .filter_map(|mutant| mutant["rule"].as_str())
        .collect();
    assert_eq!(
        rules,
        ["gt-to-ge@1", "lt-to-le@1"],
        "the two sides of the zero nobody tests"
    );
    for finding in findings {
        assert_eq!(finding["kind"], "surviving-mutant");
        assert!(
            finding["position"]["line"].as_u64().unwrap_or_default() > 0,
            "a finding names where to look: {finding}"
        );
    }
}

#[test]
fn a_mutant_a_reviewer_accepted_stops_being_a_finding() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let survivors: Vec<String> = document(&fixture)["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["outcome"] == "survived")
        .filter_map(|mutant| mutant["id"].as_str().map(ToOwned::to_owned))
        .collect();
    assert_eq!(survivors.len(), 2);

    let mut configuration = String::from("version = 1\n");
    for id in &survivors {
        let _written = write!(
            configuration,
            "\n[[acceptance]]\nid = \"{id}\"\nreason = \"the boundary is checked by an ignored test\"\n"
        );
    }
    std::fs::write(fixture.root.join(".mjutest.toml"), configuration).expect("a configuration");

    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "an accepted survivor is a decision somebody made, not a gap: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = document(&fixture);
    assert_eq!(report["verdict"], "ASSURED");
    assert_eq!(report["accounting"]["mutants"]["accepted"], 2);
    assert_eq!(
        report["accounting"]["mutants"]["survived"], 2,
        "an acceptance does not rewrite what was measured"
    );
    assert_eq!(report["findings"].as_array().expect("findings").len(), 0);
}

#[test]
fn every_mutant_is_routed_to_the_tests_that_reach_it_and_no_others() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &["--trace"]).status.code(), Some(0));

    let recording = std::fs::read_dir(fixture.root.join(".mjutest/trace"))
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path())
        .next()
        .expect("one recording");
    let events = mjutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(recording.join(mjutest_cli::trace::FILE_NAME)).expect("the stream"),
    ))
    .expect("the events");

    let routes: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            mjutest_cli::trace::Payload::Note { note } if note.kind == "route" => {
                Some(note.detail.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(routes.len(), 4, "one route per mutant: {routes:?}");
    for route in &routes {
        assert!(
            route.contains("block 1 targets"),
            "each mutation is reached by exactly the one test that covers it: {route}"
        );
    }
}
