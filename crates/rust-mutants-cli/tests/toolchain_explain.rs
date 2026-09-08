// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything one run established about one mutant, read back from what it stored.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and a document \
              this test caused to be written is one it may index"
)]

use std::process::{Command, Output};

use mjutest_devkit::fixture::Fixture;

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.args(args);
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--offline", "--locked"]);
    command.output().expect("rust-mutants runs")
}

fn measured(fixture: &Fixture) {
    let output = against(
        fixture,
        &[
            "run",
            "--tier",
            "all",
            "--no-coverage",
            "--jobs",
            "1",
            "--ui",
            "quiet",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn explain_reads_the_stored_run_and_says_what_it_established() {
    let fixture = Fixture::copy("fixture-simple");
    measured(&fixture);
    let output = against(&fixture, &["explain", "e5e8"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    for said in [
        "MUTANT",
        "RULE      gt-to-ge@1 (comparison)",
        "WHERE     src/lib.rs:11:10",
        "OUTCOME   not_run",
        "ROUTE     discharged",
        "PROVED    fixture-simple/lib/fixture_simple: never-infected",
        "REPRODUCE rust-mutants run --mutant e5e872bfbcb2afbbf7a1",
    ] {
        assert!(text.contains(said), "{said} in {text}");
    }
    assert!(
        text.contains("-    if a > b { a } else { b }")
            && text.contains("+    if a >= b { a } else { b }"),
        "the mutation is a change to the file, not two quoted strings: {text}"
    );
    assert!(
        text.lines().filter(|line| line.starts_with(' ')).count() >= 4,
        "with the lines around it a reader needs: {text}"
    );
}

#[test]
fn the_explanation_validates_against_the_schema_published_with_it() {
    let fixture = Fixture::copy("fixture-simple");
    measured(&fixture);
    let output = against(&fixture, &["explain", "e5e8", "--json"]);
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the explanation is JSON");
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            mjutest_devkit::paths::workspace_root().join("schema/rust-mutants-explain-v1.json"),
        )
        .expect("the schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let problems: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect();
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn a_file_that_changed_since_the_run_is_said_rather_than_diffed_against() {
    let fixture = Fixture::copy("fixture-simple");
    measured(&fixture);
    let path = fixture.root().join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(&path, format!("// a comment nobody measured\n{source}")).expect("write");
    let output = against(&fixture, &["explain", "e5e8"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("DIFF      none: the file has changed since the run"),
        "a diff against a file the run never saw would be a lie: {text}"
    );
}

#[test]
fn a_prefix_that_names_more_than_one_says_what_it_could_have_meant() {
    let fixture = Fixture::copy("fixture-simple");
    measured(&fixture);
    let output = against(&fixture, &["explain", ""]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(complaint.contains("11 mutants"), "{complaint}");
}

#[test]
fn list_why_skipped_instrument_and_catalog_each_answer_about_one_thing() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = String::from_utf8_lossy(
        &against(&fixture, &["list", "--tier", "all", "--file", "src/lib.rs"]).stdout,
    )
    .into_owned();
    assert!(
        listed.lines().all(|line| line.contains("src/lib.rs")),
        "a file names its own candidates: {listed}"
    );

    let places = String::from_utf8_lossy(
        &against(
            &fixture,
            &[
                "why-skipped",
                "--tier",
                "all",
                "--file",
                "src/lib.rs",
                "--line",
                "11",
            ],
        )
        .stdout,
    )
    .into_owned();
    assert!(
        places.lines().all(|line| line.starts_with("11:")),
        "and one line its own places: {places}"
    );
    assert!(places.contains("gt-to-ge"), "{places}");

    let short = listed
        .lines()
        .find(|line| line.contains("gt-to-ge"))
        .and_then(|line| line.split_whitespace().next())
        .expect("a gt-to-ge mutant")
        .to_owned();
    let guard = String::from_utf8_lossy(
        &against(
            &fixture,
            &[
                "instrument",
                "--tier",
                "all",
                "--file",
                "src/lib.rs",
                "--mutant",
                &short,
            ],
        )
        .stdout,
    )
    .into_owned();
    assert!(guard.contains("FORM      C"), "{guard}");
    assert!(guard.contains("::active("), "{guard}");

    let refused = String::from_utf8_lossy(
        &against(
            &fixture,
            &["catalog", "--tier", "all", "--rejections", "--no-verify"],
        )
        .stdout,
    )
    .into_owned();
    assert!(
        refused.contains("the compiler refused nothing"),
        "and a catalog with no refusals says so rather than printing itself: {refused}"
    );
}

#[test]
fn explain_names_the_tests_a_route_put_the_mutation_to() {
    let fixture = Fixture::copy("fixture-coverage");
    let output = against(
        &fixture,
        &["run", "--tier", "all", "--jobs", "1", "--ui", "quiet"],
    );
    assert!(
        output.status.code().is_some_and(|code| code <= 1),
        "{output:?}"
    );
    let document: serde_json::Value =
        serde_json::from_str(&mjutest_devkit::fixture::stored_report(fixture.root()))
            .expect("the report is a document");
    let narrowed = document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .find(|row| row["route"]["granularity"] == "test")
        .expect("a mutation the guards put to some of a target's tests");
    let short = narrowed["display_id"].as_str().expect("a short identity");
    let said = against(&fixture, &["explain", short]);
    assert_eq!(said.status.code(), Some(0), "{said:?}");
    let text = String::from_utf8_lossy(&said.stdout);
    assert!(text.contains("ROUTE     test "), "{text}");
    assert!(
        text.contains("TESTS     fixture-coverage/lib/fixture_coverage: tests::"),
        "a reader asking why a mutation went where it did is asking which tests: {text}"
    );
}
