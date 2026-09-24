// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything one run established about one mutant, read back from what it stored.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and a document \
              this test caused to be written is one it may index"
)]

use std::ffi::OsString;
use std::process::Output;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

include!("support/missing.rs");

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(["--root", root])
            .chain(["--offline", "--locked"])
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
    let output = against(&fixture, &["explain", "f0d2"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    for said in [
        "MUTANT",
        "RULE      gt-to-ge@1 (comparison)",
        "WHERE     src/lib.rs:11:10",
        "OUTCOME   not_run",
        "ROUTE     discharged",
        "PROVED    fixture-simple/lib/fixture_simple: never-infected",
        "REPRODUCE rust-mutants run --mutant src/lib.rs:max:gt-to-ge@11",
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
    let output = against(&fixture, &["explain", "f0d2", "--json"]);
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_slice(&output.stdout).expect("the explanation is JSON");
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-explain-v1.json"),
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
    let output = against(&fixture, &["explain", "f0d2"]);
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
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
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(complaint.contains("13 mutants"), "{complaint}");
}

#[test]
fn list_why_skipped_instrument_and_catalog_each_answer_about_one_thing() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = njutest_devkit::process::strict_utf8(
        &against(&fixture, &["list", "--tier", "all", "--file", "src/lib.rs"]).stdout,
    )
    .into_owned();
    assert!(
        listed
            .lines()
            .filter(|line| line.contains(" => "))
            .all(|line| line.contains("src/lib.rs")),
        "a file names its own candidates: {listed}"
    );

    let places = njutest_devkit::process::strict_utf8(
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
    let guard = njutest_devkit::process::strict_utf8(
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

    let refused = njutest_devkit::process::strict_utf8(
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
        njutest_devkit::strictjson::decode_str(&njutest_devkit::fixture::stored_report(
            &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
        ))
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
    let text = njutest_devkit::process::strict_utf8(&said.stdout);
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
        njutest_devkit::process::strict_utf8(&listed.stderr)
    );
    let text = njutest_devkit::process::strict_utf8(&listed.stdout).into_owned();
    let id = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .expect("a candidate")
        .to_owned();
    assert!(
        test_missing(&rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),),
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
        njutest_devkit::process::strict_utf8(&refused.stderr).contains("RM0007"),
        "and says which report it looked for: {}",
        njutest_devkit::process::strict_utf8(&refused.stderr)
    );

    let explained = against(&fixture, &["explain", &id, "--fresh"]);
    assert_eq!(
        explained.status.code(),
        Some(0),
        "`--fresh` is the word for preparing the tree again, which is how a person \
         reading code asks what a mutation is before any run has judged it: {}",
        njutest_devkit::process::strict_utf8(&explained.stderr)
    );
    let said = njutest_devkit::process::strict_utf8(&explained.stdout).into_owned();
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

#[test]
fn one_guard_is_shown_with_the_line_of_the_file_it_landed_on() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = njutest_devkit::process::strict_utf8(
        &against(&fixture, &["list", "--tier", "all", "--file", "src/lib.rs"]).stdout,
    )
    .into_owned();
    let short = listed
        .lines()
        .find(|line| line.contains("gt-to-ge"))
        .and_then(|line| line.split_whitespace().next())
        .expect("a gt-to-ge mutant")
        .to_owned();

    let shown = njutest_devkit::process::strict_utf8(
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

    for named in ["MUTANT", "FORM", "SITE"] {
        assert!(
            shown.contains(named),
            "a reader asking what one guard is gets its identity, the shape it was \
             written in, and where it sits: {named} is missing from {shown}"
        );
    }
    let rewrite = njutest_devkit::process::strict_utf8(
        &against(
            &fixture,
            &["instrument", "--tier", "all", "--file", "src/lib.rs"],
        )
        .stdout,
    )
    .into_owned();
    let landed = shown
        .lines()
        .last()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .expect("the line it landed on");
    assert!(
        rewrite.lines().any(|line| line.trim() == landed),
        "and the whole line of the rewrite the guard's branch sits on — the rewrite \
         rather than the file a person wrote, because what is being asked about is the \
         guard and a guard out of context is a string nobody can place: {landed:?} is in \
         no line of the rewrite"
    );
    assert!(
        landed.contains("::active("),
        "which is a line with the guard in it: {landed:?}"
    );
}

#[test]
fn a_guard_nothing_answers_to_is_said_rather_than_shown_as_an_empty_one() {
    let fixture = Fixture::copy("fixture-simple");
    let shown = njutest_devkit::process::strict_utf8(
        &against(
            &fixture,
            &[
                "instrument",
                "--tier",
                "all",
                "--file",
                "src/lib.rs",
                "--mutant",
                "ffffffffffff",
            ],
        )
        .stdout,
    )
    .into_owned();
    assert!(
        shown.contains("ffffffffffff") && shown.contains("src/lib.rs"),
        "a prefix that names no guard of the file names both, because the usual cause is \
         a mutant of another file and the answer has to say which file was searched: \
         {shown}"
    );
    assert!(
        !shown.contains("FORM"),
        "and nothing of a guard is printed, or a reader reads an empty one as the guard \
         they asked for: {shown}"
    );
}

#[test]
fn explain_reads_the_run_it_is_told_to_and_refuses_a_name_nobody_stored() {
    let fixture = Fixture::copy("fixture-simple");
    let named = against(
        &fixture,
        &[
            "run",
            "--tier",
            "all",
            "--no-coverage",
            "--jobs",
            "1",
            "--ui",
            "quiet",
            "--run-id",
            "monday",
        ],
    );
    assert_eq!(named.status.code(), Some(1), "{named:?}");

    let output = against(&fixture, &["explain", "f0d2", "--run", "monday"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        njutest_devkit::process::strict_utf8(&output.stdout).contains("WHERE     src/lib.rs:11:10"),
        "a run a person named is a run they can read one mutant out of: {output:?}"
    );

    let wrong = against(&fixture, &["explain", "f0d2", "--run", "tuesday"]);
    assert_eq!(
        wrong.status.code(),
        Some(2),
        "and a name nobody stored is refused rather than answered from another run: {}",
        njutest_devkit::process::strict_utf8(&wrong.stdout)
    );
    let message = njutest_devkit::process::strict_utf8(&wrong.stderr);
    assert!(
        message.contains("tuesday")
            && message.contains(&format!(
                "{}",
                rust_mutants_cli::config::Config::default()
                    .reports
                    .directory
                    .display()
            )),
        "naming what was asked for and where runs are kept: {message}"
    );
}

#[test]
fn a_catalog_and_a_report_that_disagree_about_a_mutant_are_said_to_rather_than_one_believed() {
    let fixture = Fixture::copy("fixture-simple");
    measured(&fixture);
    let run = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    );
    let path = run.join(rust_mutants::report::evidence::CATALOG);
    let mut catalog: rust_mutants::report::catalog::CatalogDocument =
        njutest_devkit::strictjson::decode_str(
            &std::fs::read_to_string(&path).expect("the stored catalog"),
        )
        .expect("the catalog is a document");
    let measured_one = catalog
        .mutants
        .iter()
        .find(|one| one.display_id.starts_with("f0d2"))
        .expect("the mutant the report answers for")
        .clone();
    catalog
        .rejections
        .push(rust_mutants::report::catalog::RejectionDocument {
            index: measured_one.index,
            id: measured_one.id.clone(),
            display_id: measured_one.display_id.clone(),
            path: measured_one.path.clone(),
            rule: measured_one.rule.clone(),
            code: Some("E0308".to_owned()),
            diagnostic: "error[E0308]: mismatched types at another mutant's line".to_owned(),
            isolated: false,
        });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&catalog).expect("the catalog serializes"),
    )
    .expect("the catalog rewritten");

    let output = against(&fixture, &["explain", "f0d2"]);
    let said = format!(
        "{}{}",
        njutest_devkit::process::strict_utf8(&output.stdout),
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        !said.contains("refused by the compiler"),
        "the run measured this mutant, so a refusal the catalog carries for it is not what \
         happened to it; saying so states a false fact about a mutation: {said}"
    );
    assert_ne!(
        output.status.code(),
        Some(0),
        "and two stored documents that disagree about one mutant are a defect to report, not a \
         choice to make quietly: {said}"
    );
    assert!(said.contains(&measured_one.display_id), "{said}");
}
