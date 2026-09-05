// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command-line contract of the `rust-mutants` binary: what `--version`
//! and `--help` print, and the exit codes of usage errors.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;
use std::process::{Command, Output};

fn rust_mutants(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .args(args)
        .env_clear()
        .env("NO_COLOR", "1")
        .output()
        .expect("rust-mutants runs")
}

#[test]
fn version_flag_prints_the_binary_name_and_its_version() {
    let output = rust_mutants(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("rust-mutants {}\n", rust_mutants::VERSION)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_flag_matches_the_recorded_help_text() {
    let output = rust_mutants(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/help.golden");
    mjutest_devkit::golden::golden(&golden, &output.stdout).expect("help text is the recorded one");
}

#[test]
fn no_arguments_prints_the_usage_to_stderr_and_exits_2() {
    let output = rust_mutants(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn an_unknown_subcommand_is_a_usage_error() {
    let output = rust_mutants(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("frobnicate"),
        "names the offending argument: {stderr}"
    );
}

// --- the commands ---------------------------------------------------------------

use std::path::PathBuf;

/// A throwaway copy of a fixture, so the tree the command line opens is one
/// nothing else is reading.
struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-cli-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy_dir(&mjutest_devkit::paths::fixtures_dir().join(name), &root);
    Fixture { root, _dir: dir }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        if entry.file_name() == "target" {
            continue;
        }
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), &destination).expect("copy");
        }
    }
}

/// Runs the binary against a fixture, with the environment a real run has.
fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.arg(args[0]);
    command.args(["--root", &fixture.root.to_string_lossy()]);
    command.args(["--tier", "all"]);
    command.args(["--offline", "--locked"]);
    command.args(&args[1..]);
    command.output().expect("rust-mutants runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn list_names_every_candidate_without_building_anything() {
    let fixture = fixture("fixture-simple");
    let output = against(&fixture, &["list"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 6, "{text}");
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
    let fixture = fixture("fixture-simple");
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
    let fixture = fixture("fixture-rejectable");
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
    let fixture = fixture("fixture-simple");
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
    assert_eq!(document["engine"], rust_mutants::VERSION);
    assert_eq!(document["catalog_digest"].as_str().map(str::len), Some(64));
    let mutants = document["mutants"].as_array().expect("mutants");
    assert_eq!(mutants.len(), 6);
    let first = &mutants[0];
    for key in [
        "index",
        "id",
        "display_id",
        "path",
        "family",
        "rule",
        "start_byte",
        "end_byte",
        "original",
        "replacement",
    ] {
        assert!(first.get(key).is_some(), "{key} is missing from {first}");
    }
    assert!(
        document["rejections"]
            .as_array()
            .expect("rejections")
            .is_empty()
    );
    assert_eq!(document["skips"].as_array().expect("skips").len(), 2);
}

#[test]
fn explain_says_everything_known_about_one_mutant() {
    let fixture = fixture("fixture-simple");
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
    let fixture = fixture("fixture-simple");
    let listed = stdout(&against(&fixture, &["list"]));
    let short_of = |rule: &str| -> String {
        listed
            .lines()
            .find(|line| line.contains(rule))
            .and_then(|line| line.split_whitespace().next())
            .unwrap_or_else(|| panic!("a {rule} mutant"))
            .to_owned()
    };

    // A mutant the tests notice: exit 0, and the outcome says so.
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

    // A mutant nothing notices: exit 1, so a script can act on it.
    let survived = against(&fixture, &["run", "--mutant", &short_of("gt-to-ge")]);
    assert_eq!(survived.status.code(), Some(1), "{}", stdout(&survived));
    assert!(stdout(&survived).contains("survived"));

    // A prefix that names nothing is refused by code, on stderr.
    let unknown = against(&fixture, &["run", "--mutant", "ffffffff"]);
    assert_eq!(unknown.status.code(), Some(2));
    let said = String::from_utf8_lossy(&unknown.stderr);
    assert!(said.contains("RM5003"), "{said}");
}

#[test]
fn instrument_prints_one_file_as_the_engine_rewrites_it() {
    let fixture = fixture("fixture-simple");
    let output = against(&fixture, &["instrument", "--file", "src/lib.rs"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(
        text.contains("__rm::active(") && text.contains("mod __rm {"),
        "{text}"
    );
    assert!(text.contains("#[allow(warnings)] pub fn max"), "{text}");

    // Printing rewrites nothing: the workspace is exactly as it was.
    let source = std::fs::read_to_string(fixture.root.join("src/lib.rs")).expect("read");
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
