// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target that answers by exiting, and a target somebody said never to start.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helpers that start the engine are not themselves tests, and a document this \
              test caused to be written is one it may index"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn against(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all", "--offline", "--locked"])
            .chain(args.iter().copied())
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

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    }
}

fn report(fixture: &Fixture) -> serde_json::Value {
    njutest_devkit::strictjson::decode_str(&njutest_devkit::fixture::stored_report(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    ))
    .expect("the report is a document")
}

#[test]
fn a_harness_free_target_answers_by_exit_code_alone() {
    let fixture = Fixture::copy("fixture-custom-harness");
    let output = against(&fixture, &[]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document = report(&fixture);
    assert_eq!(
        document["accounting"]["inconclusive"].as_u64(),
        Some(0),
        "a target with no libtest prints no summary, and reading its silence as 'nothing ran' \
         left every mutation of this library undecided: {document}"
    );
    assert_eq!(document["accounting"]["killed"].as_u64(), Some(4));
    for row in document["mutants"].as_array().expect("the rows") {
        assert_eq!(
            row["target"].as_str(),
            Some("fixture-custom-harness/test/by_exit_code"),
            "{row}"
        );
        assert_eq!(
            row["tests_run"],
            serde_json::Value::Null,
            "and how many of its tests ran is something only a harness could have said: {row}"
        );
    }
}

#[test]
fn a_skipped_target_is_never_started_and_is_listed_as_a_limitation() {
    let fixture = Fixture::copy("fixture-custom-harness");
    let directory = fixture.temp().join("recording");
    let output = against(
        &fixture,
        &[
            "--skip-target",
            "fixture-custom-harness/test/by_exit_code",
            &format!("--trace={}", directory.display()),
        ],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document = report(&fixture);
    for row in document["mutants"].as_array().expect("the rows") {
        assert_ne!(
            row["target"].as_str(),
            Some("fixture-custom-harness/test/by_exit_code"),
            "a target somebody said never to start is never started: {row}"
        );
    }
    let text = std::fs::read_to_string(directory.join("trace.jsonl")).expect("the recording");
    let events: Vec<serde_json::Value> = text
        .lines()
        .map(|line| njutest_devkit::strictjson::decode_str(line).expect("a trace event"))
        .collect();
    assert!(
        !events.iter().any(|event| {
            event["payload"]["type"].as_str() == Some("mutant-exec")
                && event["payload"]["mutant"]["target"].as_str()
                    == Some("fixture-custom-harness/test/by_exit_code")
        }),
        "and the recording holds no execution against it"
    );
    let build = events
        .iter()
        .find(|event| event["payload"]["type"].as_str() == Some("build"))
        .expect("the build event");
    let one = build["payload"]["build"]["details"]
        .as_array()
        .expect("the details")
        .iter()
        .find(|detail| detail["id"].as_str() == Some("fixture-custom-harness/test/by_exit_code"))
        .expect("the target the build produced");
    assert!(
        one["limitations"]
            .as_array()
            .is_some_and(|limitations| limitations
                .iter()
                .any(|one| one.as_str() == Some("target-skipped-by-configuration"))),
        "the build still produced it, and the run says why it asked it nothing: {one}"
    );
}

#[test]
fn the_recording_says_what_each_target_is_and_which_one_was_skipped() {
    let fixture = Fixture::copy("fixture-custom-harness");
    let directory = fixture.temp().join("recording");
    let output = against(&fixture, &[&format!("--trace={}", directory.display())]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = std::fs::read_to_string(directory.join("trace.jsonl")).expect("the recording");
    let build = text
        .lines()
        .map(|line| {
            njutest_devkit::strictjson::decode_str::<serde_json::Value>(line)
                .expect("a trace event")
        })
        .find(|event| event["payload"]["type"].as_str() == Some("build"))
        .expect("the build event");
    let details = build["payload"]["build"]["details"]
        .as_array()
        .expect("the details");
    let one = details
        .iter()
        .find(|detail| detail["id"].as_str() == Some("fixture-custom-harness/test/by_exit_code"))
        .expect("the target");
    assert_eq!(one["kind"].as_str(), Some("test"), "{one}");
    assert_eq!(
        one["harness"].as_bool(),
        Some(false),
        "the recording says what kind of answer this target gives: {one}"
    );
    assert!(
        one["limitations"]
            .as_array()
            .is_some_and(|limitations| limitations
                .iter()
                .any(|one| one.as_str() == Some("custom-harness"))),
        "{one}"
    );
}
