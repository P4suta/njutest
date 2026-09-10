// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library's documented examples: a target of their own, a switch, and a routing that says what it rests on.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helpers that start the engine are not themselves tests, and a document this \
              test caused to be written is one it may index"
)]

use mjutest_devkit::fixture::Fixture;
use std::path::Path;

fn against(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    let mut command = mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    command.args(args);
    command.output().expect("rust-mutants runs")
}

fn report(fixture: &Fixture) -> serde_json::Value {
    serde_json::from_str(&mjutest_devkit::fixture::stored_report(fixture.root()))
        .expect("the report is a document")
}

#[test]
fn doctests_can_be_switched_off() {
    let fixture = Fixture::copy("fixture-doctest");
    let directory = fixture.temp().join("recording");
    let output = against(
        &fixture,
        &["--no-doctests", "--trace", &directory.to_string_lossy()],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(directory.join("trace.jsonl")).expect("the recording");
    let build = text
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|event| event["type"].as_str() == Some("build"))
        .expect("the build event");
    let targets: Vec<&str> = build["build"]["targets"]
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
        &["--coverage", "--trace", &directory.to_string_lossy()],
    );
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(directory.join("trace.jsonl")).expect("the recording");
    let routes: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| event["type"].as_str() == Some("route"))
        .collect();
    assert!(!routes.is_empty(), "the fixture judges something");
    let reaching_doc = routes.iter().filter(|route| {
        route["route"]["reaching"]
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
    let output = against(&fixture, &["--trace", &directory.to_string_lossy()]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(directory.join("trace.jsonl")).expect("the recording");
    let events: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let build = events
        .iter()
        .find(|event| event["type"].as_str() == Some("build"))
        .expect("the build event");
    let doc = build["build"]["details"]
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
        .filter(|event| event["type"].as_str() == Some("mutant-exec"))
        .filter_map(|event| event["mutant"]["target"].as_str())
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
