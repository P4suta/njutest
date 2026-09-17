// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the command line answers about a real tree: what it lists, what it catalogs, and what it exits with.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};
use std::process::Output;

/// Runs the binary against a fixture, with the environment a real run has.
fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = fixture.root().to_string_lossy().into_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied().take(1))
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all"])
            .chain(["--offline", "--locked"])
            .chain(args.iter().copied().skip(1))
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
    let rows: Vec<&str> = text.lines().filter(|line| line.contains(" => ")).collect();
    assert_eq!(rows.len(), 11, "{text}");
    assert!(
        rows.iter().all(|line| line.contains("src/lib.rs:")),
        "{text}"
    );
    assert!(
        text.contains("11 candidates, which is what the rules propose"),
        "a list says how many it listed and what listing them is not: {text}"
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

    let output = against(&fixture, &["explain", &short, "--fresh", "--no-verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains(&format!("SHORT     {short}")), "{text}");
    assert!(
        text.contains("RULE      return-default@1 (return-replacement)"),
        "{text}"
    );
    assert!(
        text.contains("OUTCOME   no stored run answers for it"),
        "a tree prepared for the purpose says what the mutation is, not what became of it: \
         {text}"
    );
    assert!(
        text.contains("REPRODUCE rust-mutants run --mutant"),
        "{text}"
    );
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
    let refusal = String::from_utf8_lossy(&missing.stderr).into_owned();
    assert!(
        refusal.contains("RM0004") && refusal.contains("src/nope.rs"),
        "a path the workspace does not hold is a value the flag cannot take, which is \
         what a person mistyping a name needs to read; it used to be reported as a tree \
         that could not be written, which sends them looking at the disk: {refusal}"
    );
    assert!(
        refusal.contains("--file"),
        "and the flag is named: {refusal}"
    );
}
#[test]
fn the_catalog_document_validates_against_its_schema() {
    let schema_path =
        njutest_devkit::paths::workspace_root().join("schema/rust-mutants-catalog-v1.json");
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
            if njutest_devkit::reproducible::builds_the_same_twice() {
                columns[1] == "identical" || columns[1] == "differs"
            } else {
                columns[1] == "not-established"
            },
            "an answer is about what the compiler rendered, not about what the tests could \
             notice — and on a machine that renders one unchanged tree two ways there is no \
             answer to give: {row}"
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
fn the_equivalence_tally_counts_the_rows_it_printed() {
    let fixture = Fixture::copy("fixture-equivalent");
    let output = against(&fixture, &["equivalence"]);
    let text = stdout(&output);
    let rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|line| line.contains('\t') && !line.starts_with("EQUIVALENCE"))
        .map(|line| line.split('\t').collect())
        .collect();
    let identical = rows
        .iter()
        .filter(|columns| columns.get(1) == Some(&"identical"))
        .count();
    if njutest_devkit::reproducible::builds_the_same_twice() {
        assert!(
            identical > 0,
            "this fixture is built at an optimisation level where the compiler renders \
             `x + 0` and `x - 0` the same, which is the whole reason it exists: {text}"
        );
    } else {
        assert!(
            rows.iter()
                .all(|columns| columns.get(1) == Some(&"not-established")),
            "a machine that renders one unchanged tree two ways establishes nothing here, \
             and a row saying the compiler rendered a mutation would be reporting the \
             machine's own difference as the mutation's: {text}"
        );
    }
    assert!(
        text.contains(&format!(
            "asked={} of {}\tidentical={identical}",
            rows.len(),
            rows.len()
        )),
        "the tally is the count of the rows above it, and a tally that counted none \
         would read exactly like a tree the compiler renders every mutation of: {text}"
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

#[test]
fn cargo_rust_mutants_drops_the_word_cargo_gave_it_and_reads_the_rest() {
    let asked = rust_mutants_cli::cli::parse(
        ["cargo-rust-mutants", "rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("cargo calls its subcommands with their own name in argv[1]");
    let direct = rust_mutants_cli::cli::parse(
        ["rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("and a person calls it without");

    assert_eq!(
        format!("{asked:?}"),
        format!("{direct:?}"),
        "`cargo rust-mutants list` and `rust-mutants list` are one command asked for two \
         ways"
    );
}

#[test]
fn a_binary_that_is_not_a_cargo_subcommand_keeps_every_argument_it_was_given() {
    let refused = rust_mutants_cli::cli::parse(
        ["rust-mutants", "rust-mutants", "list"]
            .into_iter()
            .map(OsString::from),
    );
    assert!(
        refused.is_err(),
        "dropping a repeated word is cargo's convention and not this program's: called \
         directly, `rust-mutants rust-mutants list` is a mistake and is said to be one"
    );
}

#[test]
fn the_word_cargo_repeats_is_dropped_once_and_only_where_it_is_the_subcommand() {
    let twice = rust_mutants_cli::cli::parse(
        ["cargo-rust-mutants", "rust-mutants", "rust-mutants", "list"]
            .into_iter()
            .map(OsString::from),
    );
    assert!(
        twice.is_err(),
        "one repeat is cargo's; a second is a mistake, and dropping both would run a \
         command nobody asked for"
    );

    let elsewhere = rust_mutants_cli::cli::parse(
        ["cargo-rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("cargo may be called without the repeat, and the rest is still the command");
    let direct = rust_mutants_cli::cli::parse(
        ["rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("which is this command");
    assert_eq!(format!("{elsewhere:?}"), format!("{direct:?}"));
}
