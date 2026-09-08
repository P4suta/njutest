// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one bug report carries: everything a reader re-decides a run from, and nothing its owner did not choose to publish.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::path::Path;
use std::process::{Command, Output};

use mjutest_devkit::fixture::Fixture;

const SECRET: &str = "a-value-nobody-meant-to-publish";

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .env("RUST_MUTANTS_NOTHING", SECRET)
        .args(args)
        .args(["--root", &fixture.root().to_string_lossy()])
        .output()
        .expect("rust-mutants runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn measured() -> Fixture {
    let fixture = Fixture::copy("fixture-simple");
    let ran = against(
        &fixture,
        &["run", "--offline", "--locked", "--trace", "--coverage"],
    );
    assert!(
        ran.status.code().is_some_and(|code| code <= 1),
        "the run establishes something: {ran:?}"
    );
    fixture
}

fn manifest(bundle: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(bundle.join("bundle.json")).expect("the manifest");
    serde_json::from_str(&text).expect("the manifest is JSON")
}

fn bundle_of(output: &Output) -> std::path::PathBuf {
    let text = stdout(output);
    let first = text.lines().next().expect("the bundle is named first");
    std::path::PathBuf::from(first)
}

#[test]
fn a_bundle_holds_the_report_the_evidence_the_recording_and_the_state_of_the_machine() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    assert_eq!(gathered.status.code(), Some(0), "{gathered:?}");
    let bundle = bundle_of(&gathered);
    assert!(bundle.is_dir(), "{}", bundle.display());
    for name in [
        "run-report-v1.json",
        "doctor-v1.json",
        "toolchain.txt",
        "environment.txt",
        "bundle.json",
    ] {
        assert!(
            bundle.join(name).is_file(),
            "{name} is not in {}",
            bundle.display()
        );
    }
    assert!(bundle.join("trace").is_dir(), "the recording travels too");
    let document = manifest(&bundle);
    let held: Vec<&str> = document["held"]
        .as_array()
        .expect("what it holds")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(held.contains(&"run-report-v1.json"), "{held:?}");
    assert!(held.contains(&"trace"), "{held:?}");
    assert!(document["run_id"].as_str().is_some_and(|it| !it.is_empty()));
}

#[test]
fn what_the_run_did_not_leave_is_named_rather_than_passed_over() {
    let fixture = Fixture::copy("fixture-simple");
    let ran = against(&fixture, &["run", "--offline", "--locked", "--no-coverage"]);
    assert!(ran.status.code().is_some_and(|code| code <= 1), "{ran:?}");
    let gathered = against(&fixture, &["diagnostics"]);
    let bundle = bundle_of(&gathered);
    let document = manifest(&bundle);
    let absent: Vec<&str> = document["absent"]
        .as_array()
        .expect("what it does not hold")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(
        absent.contains(&"trace"),
        "a run that recorded nothing left no recording, and a reader is told so: {absent:?}"
    );
    assert!(
        stdout(&gathered).contains("absent\ttrace"),
        "{}",
        stdout(&gathered)
    );
    assert!(!bundle.join("trace").exists());
}

#[test]
fn a_bundle_carries_no_environment_value() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    let bundle = bundle_of(&gathered);
    let names = std::fs::read_to_string(bundle.join("environment.txt")).expect("the names");
    assert!(
        names.contains("RUST_MUTANTS_NOTHING"),
        "the name a reader needs is there: {names}"
    );
    for entry in walk(&bundle) {
        let bytes = std::fs::read(&entry).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(SECRET),
            "{} carries a value nobody published",
            entry.display()
        );
    }
}

#[test]
fn a_bundle_manifest_validates_against_the_schema_it_answers_to() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    let document = manifest(&bundle_of(&gathered));
    checked(&document, "rust-mutants-diagnostics-v1.json");
}

#[test]
fn the_doctor_a_bundle_carries_validates_against_the_schema_it_answers_to() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    let text = std::fs::read_to_string(bundle_of(&gathered).join("doctor-v1.json"))
        .expect("the doctor document");
    let document: serde_json::Value = serde_json::from_str(&text).expect("it is JSON");
    checked(&document, "rust-mutants-doctor-v1.json");
}

#[test]
fn the_measurement_a_coverage_run_kept_validates_against_the_schema_it_answers_to() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    let path = bundle_of(&gathered).join("reached-v1.json");
    let text = std::fs::read_to_string(&path).expect("the measurement");
    let document: serde_json::Value = serde_json::from_str(&text).expect("it is JSON");
    checked(&document, "rust-mutants-reached-v1.json");
}

#[test]
fn a_run_nothing_stored_is_named_rather_than_bundled_empty() {
    let fixture = Fixture::copy("fixture-simple");
    let gathered = against(&fixture, &["diagnostics", "no-such-run"]);
    assert_eq!(gathered.status.code(), Some(2), "{gathered:?}");
    let said = String::from_utf8_lossy(&gathered.stderr).into_owned();
    assert!(said.contains("no-such-run"), "{said}");
}

fn checked(document: &serde_json::Value, name: &str) {
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            mjutest_devkit::paths::workspace_root()
                .join("schema")
                .join(name),
        )
        .expect("the schema"),
    )
    .expect("the schema is a document");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let errors: Vec<String> = validator
        .iter_errors(document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}\n{document}");
}

fn walk(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            found.extend(walk(&entry.path()));
        } else {
            found.push(entry.path());
        }
    }
    found
}
