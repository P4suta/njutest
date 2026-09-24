// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest measure` and `njutest select`, end to end, against a real workspace.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest::cli::Environment;
use njutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;

/// A throwaway copy of a fixture, so what it measures and edits is nobody else's.
struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-select-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

/// One command, driven in this process against the environment a toolchain run composes.
fn asked(root: &Path, args: &[&str]) -> Output {
    let cache = njutest_devkit::paths::cache_beside(root).expect("a cache directory");
    let environment = Environment {
        cache_directory: cache,
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[]),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment,
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        njutest_devkit::process::strict_utf8(&output.stdout),
        njutest_devkit::process::strict_utf8(&output.stderr)
    )
}

fn measured(fixture: &Fixture) {
    let output = asked(&fixture.root, &["measure", "--offline", "--locked"]);
    assert_eq!(output.status.code(), Some(0), "{}", said(&output));
}

fn selected(fixture: &Fixture, format: &str) -> (Output, String) {
    let output = asked(
        &fixture.root,
        &["select", "--offline", "--locked", "--format", format],
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout).into_owned();
    (output, text)
}

fn skippable(fixture: &Fixture) -> BTreeSet<String> {
    let (output, text) = selected(fixture, "skippable");
    assert_eq!(output.status.code(), Some(0), "{}", said(&output));
    text.lines().map(str::to_owned).collect()
}

fn edit(fixture: &Fixture, path: &str, from: &str, to: &str) {
    let path = fixture.root.join(path);
    let text = std::fs::read_to_string(&path).expect("the fixture's file");
    assert!(text.contains(from), "{from} is in {}", path.display());
    std::fs::write(&path, text.replacen(from, to, 1)).expect("the edit is written");
}

const LIBRARY: &str = "fixture-hollow/lib/fixture_hollow";
const SMOKE: &str = "fixture-hollow/test/smoke";

#[test]
fn a_change_to_one_body_runs_the_target_that_entered_it_and_proves_the_other_unable_to_notice_it() {
    let fixture = fixture("fixture-hollow");
    measured(&fixture);
    assert_eq!(
        skippable(&fixture),
        BTreeSet::from([LIBRARY.to_owned(), SMOKE.to_owned()]),
        "nothing changed, so every target whose reach held is proved unable to notice it; the \
         doc target is never measured and runs"
    );
    let kept: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest::reach::directory(
                &fixture
                    .root
                    .join(njutest::config::DEFAULT_REPORTS_DIRECTORY),
            )
            .join(njutest::reach::DOCUMENT_NAME),
        )
        .expect("the measurement is kept where a selection reads it"),
    )
    .expect("the measurement is JSON");
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/njutest-reach-v1.json"),
        )
        .expect("the schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let refused: Vec<String> = validator
        .iter_errors(&kept)
        .map(|error| error.to_string())
        .collect();
    assert!(refused.is_empty(), "{refused:?}");
    edit(&fixture, "src/lib.rs", "n * 2", "n * 3");
    assert_eq!(
        skippable(&fixture),
        BTreeSet::from([LIBRARY.to_owned()]),
        "only smoke entered `double`, and the library's own test entered only `sign`"
    );
    let (_, human) = selected(&fixture, "human");
    assert!(
        human
            .lines()
            .any(|line| line.starts_with(&format!("RUN\t{SMOKE}\t"))),
        "{human}"
    );
    assert!(
        human.lines().any(|line| line == format!("SKIP\t{LIBRARY}")),
        "{human}"
    );
    assert!(
        human
            .lines()
            .any(|line| line == format!("RUN\t{SMOKE}\tits tests entered src/lib.rs:double")),
        "a reader is told which items it entered, which is what they look at next: {human}"
    );
    let (told, nextest) = selected(&fixture, "nextest");
    assert_eq!(
        nextest.trim_end(),
        "not (binary_id(=fixture-hollow))",
        "the library's binary is the one nextest may leave out"
    );
    assert!(
        njutest_devkit::process::strict_utf8(&told.stderr)
            .lines()
            .any(|line| line.starts_with("DOCTESTS\tfixture-hollow/doc/fixture_hollow\t")),
        "nextest runs no documentation, so a doc target a selection runs is said where a CI \
         that only runs nextest still sees it: {}",
        said(&told)
    );
}

#[test]
fn a_change_no_measurement_can_place_runs_every_target() {
    let fixture = fixture("fixture-hollow");
    measured(&fixture);
    edit(&fixture, "Cargo.toml", "[package]", "[package]\n");
    assert!(
        skippable(&fixture).is_empty(),
        "a manifest decides what every target compiles to"
    );
    let (_, human) = selected(&fixture, "human");
    assert!(
        human.contains("everything runs: Cargo.toml"),
        "and the reason is said: {human}"
    );
}

#[test]
fn a_new_file_in_a_measured_tree_is_a_change_git_would_not_have_to_be_told_about() {
    let fixture = fixture("fixture-hollow");
    measured(&fixture);
    std::fs::write(fixture.root.join("tests/data.txt"), "read at run time\n")
        .expect("a file the tree did not have");
    assert!(
        skippable(&fixture).is_empty(),
        "a file nobody compiled can still be read by a test"
    );
}

#[test]
fn a_selection_with_nothing_measured_refuses_rather_than_selecting_nothing() {
    let fixture = fixture("fixture-hollow");
    let (output, _) = selected(&fixture, "skippable");
    assert_eq!(output.status.code(), Some(3), "{}", said(&output));
    assert!(said(&output).contains("NJ6021"), "{}", said(&output));
}

#[test]
fn a_second_measurement_keeps_only_the_measured_bytes_it_names() {
    let fixture = fixture("fixture-hollow");
    let blobs = njutest::reach::directory(
        &fixture
            .root
            .join(njutest::config::DEFAULT_REPORTS_DIRECTORY),
    )
    .join("blobs");
    let kept = || -> BTreeSet<String> {
        std::fs::read_dir(&blobs)
            .expect("the measured bytes are kept")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .into_string()
                    .expect("a digest is ASCII")
            })
            .collect()
    };
    measured(&fixture);
    let first = kept();
    edit(&fixture, "src/lib.rs", "n * 2", "n * 3");
    measured(&fixture);
    let second = kept();
    assert_eq!(
        first.len(),
        second.len(),
        "one version of each source file is kept, not every version ever measured"
    );
    assert_ne!(
        first, second,
        "and the edited file's measured bytes are the new ones"
    );
}

#[test]
fn a_measured_file_kept_wrong_is_written_again_rather_than_kept() {
    let fixture = fixture("fixture-hollow");
    measured(&fixture);
    let blobs = njutest::reach::directory(
        &fixture
            .root
            .join(njutest::config::DEFAULT_REPORTS_DIRECTORY),
    )
    .join("blobs");
    let one = std::fs::read_dir(&blobs)
        .expect("the measured bytes are kept")
        .next()
        .expect("one of them")
        .expect("an entry")
        .path();
    std::fs::write(&one, "not the bytes this is named for").expect("corrupt it");
    measured(&fixture);
    let bytes = std::fs::read(&one).expect("the file is there again");
    let name = one
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a digest name");
    assert_eq!(
        rust_mutants::id::HexDigest::of(&bytes).as_str(),
        name,
        "a kept file that is not the bytes it is named for would make every later selection of \
         it unproven for as long as it stayed"
    );
}

#[test]
fn a_target_that_reads_a_source_file_as_text_runs_for_an_edit_to_it() {
    let fixture = fixture("fixture-reads-tree");
    measured(&fixture);
    edit(&fixture, "src/quiet.rs", "\"hush\"", "\"shh\"");
    let skipped = skippable(&fixture);
    assert!(
        !skipped.contains("fixture-reads-tree/test/reads"),
        "`reads` never enters `quiet`, yet it reads the file and fails on this edit: {skipped:?}"
    );
    assert!(
        skipped.contains("fixture-reads-tree/test/enters"),
        "and a target that neither enters nor reads it is still skipped: {skipped:?}"
    );
}

#[test]
fn an_edit_to_code_that_runs_inside_the_compiler_runs_every_target() {
    let fixture = fixture("fixture-macros");
    measured(&fixture);
    edit(
        &fixture,
        "crates/derive/src/lib.rs",
        "repeats(2) > 1",
        "repeats(2) > 0",
    );
    let skipped = skippable(&fixture);
    assert!(
        skipped.is_empty(),
        "`noop` runs inside the compiler while every crate that derives it is built, so no test \
         entering it or not says whether an edit to it changes a target: {skipped:?}"
    );
    let (_, human) = selected(&fixture, "human");
    assert!(
        human.contains("everything runs: crates/derive/src/lib.rs"),
        "and the reason names the file the compiler ran: {human}"
    );
}

#[test]
fn cargo_configuration_outside_the_tree_that_changed_runs_every_target() {
    let fixture = fixture("fixture-hollow");
    measured(&fixture);
    let above = fixture
        .root
        .parent()
        .expect("the copy has a parent")
        .join(".cargo");
    std::fs::create_dir_all(&above).expect("mkdir");
    std::fs::write(
        above.join("config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"from_above\"]\n",
    )
    .expect("a configuration the build reads from an ancestor");
    assert!(
        skippable(&fixture).is_empty(),
        "cargo reads configuration from every ancestor of the tree, which no survey of the \
         tree sees, and the flags it adds compile every target differently"
    );
}
