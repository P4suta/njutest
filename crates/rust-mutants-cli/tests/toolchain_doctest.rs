// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library's documented examples: a target of their own, a switch, and a routing that says what it rests on.

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
        vars: njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect(),
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
fn doctests_can_be_switched_off() {
    let fixture = Fixture::copy("fixture-doctest");
    let directory = fixture.temp().join("recording");
    let output = against(
        &fixture,
        &["--no-doctests", &format!("--trace={}", directory.display())],
    );
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
    let targets: Vec<&str> = build["payload"]["build"]["targets"]
        .as_array()
        .expect("the targets")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(
        !targets.iter().any(|target| target.contains("/doc/")),
        "a documentation target costs a `cargo test --doc` for every mutation nothing else \
         noticed, which is why it is a switch: {targets:?}"
    );
}

#[test]
fn a_documentation_target_reaches_every_mutation_of_its_own_library_under_coverage() {
    let fixture = Fixture::copy("fixture-doctest");
    let directory = fixture.temp().join("recording");
    let output = against(
        &fixture,
        &["--coverage", &format!("--trace={}", directory.display())],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = std::fs::read_to_string(directory.join("trace.jsonl")).expect("the recording");
    let routes: Vec<serde_json::Value> = text
        .lines()
        .map(|line| {
            njutest_devkit::strictjson::decode_str::<serde_json::Value>(line)
                .expect("a trace event")
        })
        .filter(|event| event["payload"]["type"].as_str() == Some("route"))
        .collect();
    assert!(!routes.is_empty(), "the fixture judges something");
    let reaching_doc = routes.iter().filter(|route| {
        route["payload"]["route"]["reaching"]
            .as_array()
            .is_some_and(|reaching| {
                reaching
                    .iter()
                    .any(|target| target.as_str().is_some_and(|one| one.contains("/doc/")))
            })
    });
    assert!(
        reaching_doc.count() > 0,
        "a coverage build never instruments a documented example, so a measurement says \
         nothing about what one reached and the route has to keep it: {routes:?}"
    );
}

#[test]
fn a_library_without_examples_costs_no_run() {
    let fixture = Fixture::copy("fixture-custom-harness");
    let directory = fixture.temp().join("recording");
    let output = against(&fixture, &[&format!("--trace={}", directory.display())]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = std::fs::read_to_string(directory.join("trace.jsonl")).expect("the recording");
    let events: Vec<serde_json::Value> = text
        .lines()
        .map(|line| njutest_devkit::strictjson::decode_str(line).expect("a trace event"))
        .collect();
    let build = events
        .iter()
        .find(|event| event["payload"]["type"].as_str() == Some("build"))
        .expect("the build event");
    let doc = build["payload"]["build"]["details"]
        .as_array()
        .expect("the details")
        .iter()
        .find(|detail| detail["kind"].as_str() == Some("doc"))
        .expect("a library has a documentation target");
    assert!(
        doc["limitations"]
            .as_array()
            .is_some_and(|limitations| limitations
                .iter()
                .any(|one| one.as_str() == Some("doctests-routed-by-file"))),
        "{doc}"
    );
    let ran: Vec<&str> = events
        .iter()
        .filter(|event| event["payload"]["type"].as_str() == Some("mutant-exec"))
        .filter_map(|event| event["payload"]["mutant"]["target"].as_str())
        .collect();
    assert!(
        !ran.iter().any(|target| target.contains("/doc/")),
        "the library has no documented examples, so its documentation target answers nothing \
         and paying a `cargo test --doc` for it is pure waste: {ran:?}"
    );
    let document = report(&fixture);
    assert_eq!(
        document["accounting"]["inconclusive"].as_u64(),
        Some(0),
        "and a target that answers nothing must not be what decides a mutation: {document}"
    );
}
