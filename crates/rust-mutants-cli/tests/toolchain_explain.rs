// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything one run established about one mutant, read back from what it stored.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and a document \
              this test caused to be written is one it may index"
)]

use std::ffi::OsString;
use std::process::Output;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = fixture.root().to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", root.as_str()])
            .chain(["--offline", "--locked"])
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    mjutest_devkit::process::answered(code, out, err)
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: mjutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
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

#[test]
fn a_tree_with_no_stored_run_explains_a_mutation_by_preparing_one_when_asked() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = against(&fixture, &["list"]);
    assert_eq!(
        listed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let text = String::from_utf8_lossy(&listed.stdout).into_owned();
    let id = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .expect("a candidate")
        .to_owned();
    assert!(
        !fixture.root().join("reports/mutation").exists(),
        "nothing has stored a run in this tree"
    );

    let refused = against(&fixture, &["explain", &id]);
    assert_ne!(
        refused.status.code(),
        Some(0),
        "with nothing stored there is nothing to read back, and an explanation invented \
         from a tree the reader did not ask about would be about a catalog no run used"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("RM0007"),
        "and says which report it looked for: {}",
        String::from_utf8_lossy(&refused.stderr)
    );

    let explained = against(&fixture, &["explain", &id, "--fresh"]);
    assert_eq!(
        explained.status.code(),
        Some(0),
        "`--fresh` is the word for preparing the tree again, which is how a person \
         reading code asks what a mutation is before any run has judged it: {}",
        String::from_utf8_lossy(&explained.stderr)
    );
    let said = String::from_utf8_lossy(&explained.stdout).into_owned();
    assert!(
        said.contains(&id) && said.contains("src/lib.rs"),
        "the explanation is about the mutation that was named, and says where it is: \
         {said}"
    );
    assert!(
        !said.contains("killed") && !said.contains("survived"),
        "and says nothing about an outcome, because no run has established one and a \
         word here would be read as one that had: {said}"
    );
}
