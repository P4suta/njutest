// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one bug report carries: everything a reader re-decides a run from, and nothing its owner did not choose to publish.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::ffi::OsString;
use std::path::Path;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

include!("support/metadata.rs");
include!("support/missing.rs");

const SECRET: &str = "a-value-nobody-meant-to-publish";

/// What one command said, driven in this process.
#[derive(Debug)]
struct Said {
    code: u8,
    out: String,
    err: String,
}

fn against(fixture: &Fixture, args: &[&str]) -> Said {
    let mut vars: rust_mutants::vars::Variables = njutest_devkit::paths::environment_for_a_run()
        .into_iter()
        .collect();
    vars.set("RUST_MUTANTS_NOTHING", SECRET);
    let environment = Environment {
        vars,
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", njutest_devkit::paths::utf8(fixture.root())])
            .map(OsString::from),
        &environment,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    Said {
        code,
        out: njutest_devkit::process::strict_utf8(&out).into_owned(),
        err: njutest_devkit::process::strict_utf8(&err).into_owned(),
    }
}

fn measured() -> Fixture {
    let fixture = Fixture::copy("fixture-simple");
    let ran = against(
        &fixture,
        &["run", "--offline", "--locked", "--trace", "--coverage"],
    );
    assert!(
        ran.code <= 1,
        "the run establishes something: {}{}",
        ran.out,
        ran.err
    );
    fixture
}

fn manifest(bundle: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(bundle.join("bundle.json")).expect("the manifest");
    njutest_devkit::strictjson::decode_str(&text).expect("the manifest is JSON")
}

fn bundle_of(said: &Said) -> std::path::PathBuf {
    let first = said.out.lines().next().expect("the bundle is named first");
    std::path::PathBuf::from(first)
}

#[test]
fn a_bundle_holds_the_report_the_evidence_the_recording_and_the_state_of_the_machine() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    assert_eq!(gathered.code, 0, "{}{}", gathered.out, gathered.err);
    let bundle = bundle_of(&gathered);
    assert!(test_metadata(&bundle).is_dir(), "{}", bundle.display());
    for name in [
        "run-report-v1.json",
        "doctor-v1.json",
        "toolchain.txt",
        "environment.txt",
        "bundle.json",
    ] {
        assert!(
            test_metadata(&bundle.join(name)).is_file(),
            "{name} is not in {}",
            bundle.display()
        );
    }
    assert!(
        test_metadata(&bundle.join("trace")).is_dir(),
        "the recording travels too"
    );
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
    assert!(ran.code <= 1, "{}{}", ran.out, ran.err);
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
    assert!(gathered.out.contains("absent\ttrace"), "{}", gathered.out);
    assert!(test_missing(&bundle.join("trace")));
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
        let bytes =
            std::fs::read(&entry).unwrap_or_else(|error| panic!("{}: {error}", entry.display()));
        assert!(
            !bytes
                .windows(SECRET.len())
                .any(|window| window == SECRET.as_bytes()),
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
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("it is JSON");
    checked(&document, "rust-mutants-doctor-v1.json");
}

#[test]
fn the_measurement_a_coverage_run_kept_validates_against_the_schema_it_answers_to() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    let path = bundle_of(&gathered).join("reached-v1.json");
    let text = std::fs::read_to_string(&path).expect("the measurement");
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("it is JSON");
    checked(&document, "rust-mutants-reached-v1.json");
}

#[test]
fn a_run_nothing_stored_is_named_rather_than_bundled_empty() {
    let fixture = Fixture::copy("fixture-simple");
    let gathered = against(&fixture, &["diagnostics", "no-such-run"]);
    assert_eq!(gathered.code, 2, "{}{}", gathered.out, gathered.err);
    assert!(gathered.err.contains("no-such-run"), "{}", gathered.err);
}

#[test]
fn a_bundle_goes_where_it_was_asked_to_go_and_holds_the_same_thing_there() {
    let fixture = measured();
    let elsewhere = fixture.temp().join("to-send");
    let gathered = against(
        &fixture,
        &[
            "diagnostics",
            "--output",
            njutest_devkit::paths::utf8(&elsewhere),
        ],
    );
    assert_eq!(gathered.code, 0, "{}{}", gathered.out, gathered.err);
    assert_eq!(
        bundle_of(&gathered),
        elsewhere,
        "a person sending a bug report says where to put it, and a bundle written \
         beside the run instead is one they have to go and find: {}",
        gathered.out
    );
    assert!(
        test_metadata(&elsewhere.join("bundle.json")).is_file()
            && test_metadata(&elsewhere.join("run-report-v1.json")).is_file(),
        "and it holds what a bundle holds wherever it is: {}",
        gathered.out
    );
    assert!(
        test_missing(
            &rust_mutants_cli::app::stored::Store::read(fixture.root())
                .root()
                .join(
                    manifest(&elsewhere)["run_id"]
                        .as_str()
                        .expect("the run it is about"),
                )
                .join("diagnostics"),
        ),
        "and nothing was written beside the run as well, or the value nobody published \
         is in two places instead of one"
    );
}

#[test]
fn a_bundle_accounts_for_every_part_it_was_gathered_from() {
    let fixture = measured();
    let gathered = against(&fixture, &["diagnostics"]);
    let bundle = bundle_of(&gathered);
    let document = manifest(&bundle);
    let named: Vec<&str> = ["held", "absent"]
        .iter()
        .flat_map(|key| {
            document[*key]
                .as_array()
                .expect("a manifest says both")
                .iter()
                .filter_map(serde_json::Value::as_str)
        })
        .collect();
    for part in [
        "run-report-v1.json",
        "catalog-v1.json",
        "reached-v1.json",
        "trace",
        ".rust-mutants.toml",
        "doctor-v1.json",
        "toolchain.txt",
        "environment.txt",
    ] {
        assert!(
            named.contains(&part),
            "every part a bundle is gathered from is either held or named absent, and \
             one that is neither is one a reader cannot tell from a part this release \
             stopped gathering: {part} is in neither of {named:?}"
        );
    }
    assert!(
        !named.contains(&"bundle.json"),
        "the manifest does not list itself: {named:?}"
    );
    for name in &named {
        assert_eq!(
            !test_missing(&bundle.join(name)),
            document["held"]
                .as_array()
                .expect("what it holds")
                .iter()
                .any(|held| held == name),
            "and what the manifest says is held is what is on the disk: {name}"
        );
    }
}

#[test]
fn the_run_a_bundle_is_about_is_the_one_that_was_named() {
    let fixture = measured();
    let second = against(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--run-id",
            "the-earlier-one",
        ],
    );
    assert!(second.code <= 1, "{}{}", second.out, second.err);

    let newest = manifest(&bundle_of(&against(&fixture, &["diagnostics"])));
    assert_eq!(
        newest["run_id"], "the-earlier-one",
        "with nothing named, a bundle is about the run that just happened: {newest}"
    );

    let named = manifest(&bundle_of(&against(
        &fixture,
        &["diagnostics", "the-earlier-one"],
    )));
    assert_eq!(
        named["run_id"], "the-earlier-one",
        "and naming one is how a person sends the run they are talking about rather \
         than the one they made while working out how to send it: {named}"
    );
}

fn checked(document: &serde_json::Value, name: &str) {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root()
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
    for entry in entries {
        let entry = entry.expect("diagnostics directory entry");
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            found.extend(walk(&entry.path()));
        } else {
            found.push(entry.path());
        }
    }
    found
}

#[test]
fn a_part_the_bundle_does_not_hold_is_not_a_directory_in_it_either() {
    let fixture = Fixture::copy("fixture-simple");
    let ran = against(&fixture, &["run", "--offline", "--locked", "--no-coverage"]);
    assert!(ran.code <= 1, "{}{}", ran.out, ran.err);

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
        "a run that recorded nothing left no recording: {absent:?}"
    );
    for name in &absent {
        assert!(
            test_missing(&bundle.join(name)),
            "and nothing of it is in the bundle: a directory that is there beside a \
             manifest that says it is absent is two answers to one question, and the one \
             a person opening the bundle reads first is the directory: {name}"
        );
    }
}
