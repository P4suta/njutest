// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the command line answers about a real tree: what it lists, what it catalogs, and what it exits with.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use mjutest_devkit::fixture::Fixture;
use std::process::{Command, Output};

/// Runs the binary against a fixture, with the environment a real run has.
fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.arg(args[0]);
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--tier", "all"]);
    command.args(["--offline", "--locked"]);
    command.args(&args[1..]);
    command.output().expect("rust-mutants runs")
}
#[test]
fn list_names_every_candidate_without_building_anything() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["list"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 11, "{text}");
    assert!(
        lines
            .iter()
            .all(|line| line.contains("src/lib.rs:") && line.contains(" => ")),
        "{text}"
    );
    assert!(text.contains("gt-to-ge@1"), "{text}");
    assert!(text.contains("\">\" => \">=\""), "{text}");
}
#[test]
fn why_skipped_tallies_the_reasons_with_a_sentence_each() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["why-skipped"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("test-code"), "{text}");
    assert!(text.contains("test-only-file"), "{text}");
    assert!(
        text.contains("measures itself"),
        "the reason is explained: {text}"
    );
}
#[test]
fn catalog_says_what_compiles_and_what_the_compiler_refused() {
    let fixture = Fixture::copy("fixture-rejectable");
    let output = against(&fixture, &["catalog", "--no-verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("refused by the compiler:"), "{text}");
    assert!(text.contains("cannot subtract"), "{text}");
    assert!(text.contains("accepted, 4 refused"), "{text}");
}
#[test]
fn catalog_as_json_is_one_document_a_program_can_read() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["catalog", "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("one JSON document");
    assert_eq!(document["document_type"], "rust-mutants/catalog");
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["tool_version"], rust_mutants::VERSION);
    assert_eq!(
        document["workspace"]["catalog_digest"]
            .as_str()
            .map(str::len),
        Some(64)
    );
    assert_eq!(document["skips"].as_array().expect("skips").len(), 2);
}
#[test]
fn explain_says_everything_known_about_one_mutant() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = stdout(&against(&fixture, &["list"]));
    let short = listed
        .lines()
        .find(|line| line.contains("return-default"))
        .and_then(|line| line.split_whitespace().next())
        .expect("a return-default mutant")
        .to_owned();

    let output = against(&fixture, &["explain", &short, "--no-verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains(&format!("short     {short}")), "{text}");
    assert!(
        text.contains("rule      return-default@1 (return-replacement)"),
        "{text}"
    );
    assert!(text.contains("verdict   accepted"), "{text}");
    assert!(text.contains("fixture-simple/lib/fixture_simple"), "{text}");
}
#[test]
fn run_exits_by_what_the_tests_said() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = stdout(&against(&fixture, &["list"]));
    let short_of = |rule: &str| -> String {
        listed
            .lines()
            .find(|line| line.contains(rule))
            .and_then(|line| line.split_whitespace().next())
            .unwrap_or_else(|| panic!("a {rule} mutant"))
            .to_owned()
    };

    let killed = against(&fixture, &["run", "--mutant", &short_of("return-default")]);
    assert_eq!(
        killed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&killed.stderr)
    );
    let text = stdout(&killed);
    assert!(text.contains("killed"), "{text}");
    assert!(text.contains("fixture-simple/lib/fixture_simple"), "{text}");

    let survived = against(&fixture, &["run", "--mutant", &short_of("gt-to-ge")]);
    assert_eq!(survived.status.code(), Some(1), "{}", stdout(&survived));
    assert!(stdout(&survived).contains("survived"));

    let unknown = against(&fixture, &["run", "--mutant", "ffffffff"]);
    assert_eq!(unknown.status.code(), Some(2));
    let said = String::from_utf8_lossy(&unknown.stderr);
    assert!(said.contains("RM5003"), "{said}");
}
#[test]
fn instrument_prints_one_file_as_the_engine_rewrites_it() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["instrument", "--file", "src/lib.rs"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    let module = rust_mutants::instrument::module_name("src/lib.rs", "");
    assert!(
        text.contains(&format!("{module}::active(")) && text.contains(&format!("mod {module} {{")),
        "{text}"
    );
    assert!(text.contains("#[allow(warnings, unused, unused_qualifications, unfulfilled_lint_expectations, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery, clippy::cargo)] pub fn max"), "{text}");

    let source = std::fs::read_to_string(fixture.root().join("src/lib.rs")).expect("read");
    assert!(
        !source.contains("__rm"),
        "the source workspace is read-only"
    );

    let missing = against(&fixture, &["instrument", "--file", "src/nope.rs"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("RM5006"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}
#[test]
fn the_catalog_document_validates_against_its_schema() {
    let schema_path =
        mjutest_devkit::paths::workspace_root().join("schema/rust-mutants-catalog-v1.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).expect("the schema"))
            .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");

    for fixture_name in ["fixture-simple", "fixture-rejectable"] {
        let fixture = Fixture::copy(fixture_name);
        let output = against(&fixture, &["catalog", "--json", "--no-verify"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let document: serde_json::Value =
            serde_json::from_str(&stdout(&output)).expect("one JSON document");
        let problems: Vec<String> = validator
            .iter_errors(&document)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();
        assert!(problems.is_empty(), "{fixture_name}: {problems:?}");

        let mutants = document["mutants"].as_array().expect("mutants");
        assert!(!mutants.is_empty(), "{fixture_name}");
        assert!(
            mutants
                .iter()
                .all(|mutant| mutant["line"].as_u64().unwrap_or(0) > 0),
            "every mutant is placed in the file a person would open"
        );
        assert_eq!(document["selection"]["tier"], "all");
        assert_eq!(
            document["workspace"]["root_name"], fixture_name,
            "the workspace names itself"
        );
    }
}

#[test]
fn equivalence_says_what_the_compiler_renders_identically_and_never_says_equivalent() {
    let fixture = Fixture::copy("fixture-equivalent");
    let output = against(&fixture, &["equivalence"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    let rows: Vec<&str> = text.lines().filter(|line| line.contains('\t')).collect();
    assert!(!rows.is_empty(), "{text}");
    for row in &rows {
        let columns: Vec<&str> = row.split('\t').collect();
        if columns[0] == "EQUIVALENCE" {
            continue;
        }
        assert_eq!(columns.len(), 4, "{row}");
        assert!(
            columns[1] == "identical" || columns[1] == "differs",
            "an answer is about what the compiler rendered, not about what the tests could \
             notice: {row}"
        );
    }
    assert!(
        text.contains("identical is not equivalent"),
        "the summary says what identical does not mean: {text}"
    );
    assert!(
        !text.contains("\tequivalent\t") && !text.contains(" equivalent "),
        "a mutation of a function nothing links comes out identical for the opposite of a \
         reassuring reason, so this command never says equivalent: {text}"
    );
}

#[test]
fn equivalence_asks_about_at_most_the_limit_it_was_given() {
    let fixture = Fixture::copy("fixture-equivalent");
    let output = against(&fixture, &["equivalence", "--limit", "2"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("asked=2"), "{text}");
    let rows = text
        .lines()
        .filter(|line| line.contains('\t') && !line.starts_with("EQUIVALENCE"))
        .count();
    assert_eq!(rows, 2, "{text}");
}
