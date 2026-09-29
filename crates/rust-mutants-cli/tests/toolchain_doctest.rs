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
        output
            .status
            .code()
            .is_some_and(|code| code < i32::from(rust_mutants::run::EXIT_FAILED)),
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
        output
            .status
            .code()
            .is_some_and(|code| code < i32::from(rust_mutants::run::EXIT_FAILED)),
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

fn events(directory: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(directory.join("trace.jsonl"))
        .expect("the recording")
        .lines()
        .map(|line| njutest_devkit::strictjson::decode_str(line).expect("a trace event"))
        .collect()
}

fn mutants_at(document: &serde_json::Value, line: u64) -> Vec<&serde_json::Value> {
    document["mutants"]
        .as_array()
        .expect("the mutants")
        .iter()
        .filter(|mutant| mutant["line"].as_u64() == Some(line))
        .collect()
}

#[test]
fn running_the_documentation_again_rebuilds_nothing() {
    let fixture = Fixture::copy("fixture-doctest");
    let directory = fixture.temp().join("recording");
    let output = against(
        &fixture,
        &["--no-seal", &format!("--trace={}", directory.display())],
    );
    assert!(
        output
            .status
            .code()
            .is_some_and(|code| code < i32::from(rust_mutants::run::EXIT_FAILED)),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let documentation: Vec<serde_json::Value> = events(&directory)
        .into_iter()
        .filter(|event| event["payload"]["type"].as_str() == Some("exec"))
        .filter(|event| {
            let argv = event["payload"]["exec"]["argv"].as_array();
            argv.is_some_and(|argv| {
                argv.iter().any(|arg| arg.as_str() == Some("--doc"))
                    && !argv
                        .iter()
                        .any(|arg| arg.as_str() == Some(rust_mutants::sealed::TARGET))
            })
        })
        .collect();
    assert!(
        documentation.len() > 1,
        "the documentation ran natively for its baseline and for a mutant"
    );
    for exec in documentation.iter().skip(1) {
        let printed = std::fs::read_to_string(
            directory.join(
                exec["payload"]["exec"]["output_path"]
                    .as_str()
                    .expect("a path"),
            ),
        )
        .expect("the recorded output");
        assert!(
            !printed
                .lines()
                .any(|line| line.trim_start().starts_with("Compiling ")),
            "a documentation run after the first that compiles the library again is a build the \
             tests did not run against, and it rewrites the files a test running beside it \
             reads:\n{printed}"
        );
    }
}

#[test]
fn a_mutation_that_stops_a_doctest_from_panicking_is_detected_sealed() {
    let fixture = Fixture::copy("fixture-doctest-alone");
    let output = against(&fixture, &[]);
    assert!(
        output
            .status
            .code()
            .is_some_and(|code| code < i32::from(rust_mutants::run::EXIT_FAILED)),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document = report(&fixture);
    let condition = mutants_at(&document, 29);
    let stopped: Vec<&&serde_json::Value> = condition
        .iter()
        .filter(|mutant| mutant["rule"].as_str() == Some("condition-to-false"))
        .collect();
    assert_eq!(stopped.len(), 1, "{condition:?}");
    let evidence = &stopped[0]["evidence"];
    assert_eq!(evidence["kind"].as_str(), Some("sealed"), "{evidence}");
    assert_eq!(
        evidence["executions"][0]["test"]
            .as_str()
            .map(|test| test.replace('\\', "/")),
        Some("src/lib.rs - divide (line 25)".to_owned()),
        "rustdoc names a doctest by its file as the host spells it: {evidence}"
    );
    assert_eq!(
        evidence["executions"][0]["came_to"].as_str(),
        Some("failed"),
        "an example that should panic and returned is a test that failed: {evidence}"
    );
    let always: Vec<&&serde_json::Value> = condition
        .iter()
        .filter(|mutant| mutant["rule"].as_str() == Some("condition-to-true"))
        .collect();
    assert_eq!(
        always.first().and_then(|mutant| mutant["outcome"].as_str()),
        Some("survived"),
        "a mutation that panics for every divisor still panics where the example asks it to: \
         {always:?}"
    );
}

#[test]
fn what_only_an_example_the_sealed_target_ignores_reaches_is_unproven() {
    let fixture = Fixture::copy("fixture-doctest-host-only");
    let output = against(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(i32::from(rust_mutants::run::EXIT_UNESTABLISHED)),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document = report(&fixture);
    let documentation = document["targets"]
        .as_array()
        .expect("the targets")
        .iter()
        .find(|target| target["kind"].as_str() == Some("doc"))
        .expect("the documentation target");
    let uncontrolled: Vec<(String, &str)> = documentation["sealed"]["uncontrolled"]
        .as_array()
        .expect("the uncontrolled tests")
        .iter()
        .map(|one| {
            (
                one["test"].as_str().expect("a test").replace('\\', "/"),
                one["reason"].as_str().expect("a reason"),
            )
        })
        .collect();
    assert_eq!(
        uncontrolled,
        [("src/lib.rs - twice (line 8)".to_owned(), "not-held")],
        "the report names the example the sealed build does not hold: {documentation}"
    );
    let printed = njutest_devkit::process::strict_utf8(&output.stdout).replace('\\', "/");
    assert!(
        printed.contains("SEALED") && printed.contains("src/lib.rs - twice (line 8) (not-held)"),
        "the run says which test it could not seal, where a reader of an unproven mutant \
         looks for why: {printed}"
    );
    for mutant in mutants_at(&document, 12) {
        let evidence = &mutant["evidence"];
        assert_eq!(evidence["kind"].as_str(), Some("unproven"), "{mutant}");
        assert!(
            evidence["reasons"].as_array().is_some_and(|reasons| reasons
                .iter()
                .any(|one| one.as_str() == Some("test-absent"))),
            "the example that reaches it runs only natively, and the sealed build holds it only as \
             an example it ignores: {mutant}"
        );
    }
}

#[test]
fn an_example_the_sealed_host_refuses_is_one_uncontrolled_test_of_its_merged_binary() {
    let fixture = Fixture::copy("fixture-doctest-refused");
    let output = against(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(i32::from(rust_mutants::run::EXIT_UNESTABLISHED)),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document = report(&fixture);
    let documentation = document["targets"]
        .as_array()
        .expect("the targets")
        .iter()
        .find(|target| target["kind"].as_str() == Some("doc"))
        .expect("the documentation target");
    assert_eq!(
        documentation["sealed"]["state"].as_str(),
        Some("sealed"),
        "an example that fails sealed is one test without a control, not a library the sealed \
         build cannot answer for: {documentation}"
    );
    let uncontrolled: Vec<(String, &str)> = documentation["sealed"]["uncontrolled"]
        .as_array()
        .expect("the uncontrolled tests")
        .iter()
        .map(|one| {
            (
                one["test"].as_str().expect("a test").replace('\\', "/"),
                one["reason"].as_str().expect("a reason"),
            )
        })
        .collect();
    assert_eq!(
        uncontrolled,
        [("src/lib.rs - double (line 17)".to_owned(), "panicked")],
        "the example that starts a thread fails its control, and it is the only one without a \
         control: {documentation}"
    );
    for (line, example, when) in [
        (12, "src/lib.rs - after (line 8)", "before"),
        (31, "src/lib.rs - half (line 27)", "after"),
    ] {
        for mutant in mutants_at(&document, line) {
            let evidence = &mutant["evidence"];
            assert_eq!(evidence["kind"].as_str(), Some("sealed"), "{mutant}");
            assert_eq!(
                evidence["executions"][0]["test"]
                    .as_str()
                    .map(|test| test.replace('\\', "/")),
                Some(example.to_owned()),
                "the example the merged binary holds {when} the one that stopped it answers \
                 sealed: {mutant}"
            );
        }
    }
}
