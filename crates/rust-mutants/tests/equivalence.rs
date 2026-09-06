// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether the compiler renders a mutation identically to the program it mutates.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::{Path, PathBuf};

use rust_mutants::equivalence::artifacts::{
    Artifacts, DIFFERENT_TARGETS, Identity, NOTHING_TO_COMPARE, compare,
};
use rust_mutants::equivalence::{ProveOptions, Prover};
use rust_mutants::runner::Cancel;
use rust_mutants::trace::Recorder;
use rust_mutants::workspace::OpenOptions;

fn artifacts(entries: &[(&str, &str)]) -> Artifacts {
    entries
        .iter()
        .map(|(id, digest)| ((*id).to_owned(), (*digest).to_owned()))
        .collect()
}

#[test]
fn an_empty_artifact_set_is_not_a_proof() {
    assert_eq!(
        compare(&Artifacts::new(), &Artifacts::new()),
        Identity::NotEstablished(NOTHING_TO_COMPARE),
        "two empty sets are equal and are not two equal programs; a build that produced \
         nothing is a build this run learned nothing from"
    );
    assert_eq!(
        compare(&artifacts(&[("a", "1")]), &Artifacts::new()),
        Identity::NotEstablished(NOTHING_TO_COMPARE)
    );
}

#[test]
fn two_builds_of_different_targets_are_not_two_programs_to_compare() {
    assert_eq!(
        compare(
            &artifacts(&[("a", "1")]),
            &artifacts(&[("a", "1"), ("b", "2")])
        ),
        Identity::NotEstablished(DIFFERENT_TARGETS)
    );
}

#[test]
fn the_same_bytes_are_the_same_program_and_different_bytes_are_not() {
    assert_eq!(
        compare(
            &artifacts(&[("a", "1"), ("b", "2")]),
            &artifacts(&[("a", "1"), ("b", "2")])
        ),
        Identity::Identical
    );
    assert_eq!(
        compare(
            &artifacts(&[("a", "1"), ("b", "2")]),
            &artifacts(&[("a", "1"), ("b", "3")])
        ),
        Identity::Differs
    );
}

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-equivalence-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy(&source, &root);
    Fixture { root, _dir: dir }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the directory");
    for entry in std::fs::read_dir(from).expect("the fixture") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("a copy");
        }
    }
}

fn prover(fixture: &Fixture, cancel: &Cancel) -> Prover {
    Prover::open(
        &fixture.root,
        &ProveOptions {
            open: OpenOptions {
                cargo: Some(mjutest_devkit::paths::cargo_binary()),
                env: std::env::vars_os().collect(),
                temp_directory: fixture.root.parent().unwrap_or(&fixture.root).to_path_buf(),
                locked: true,
                offline: true,
                ..OpenOptions::default()
            },
            ..ProveOptions::default()
        },
        cancel,
        &Recorder::disabled(),
    )
    .expect("the tree is copied and built")
}

#[test]
fn a_mutation_the_compiler_renders_identically_is_identical_and_one_it_renders_is_not() {
    let fixture = fixture("fixture-equivalent");
    let cancel = Cancel::new();
    let mut prover = prover(&fixture, &cancel);
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let source = std::fs::read(fixture.root.join("src/lib.rs")).expect("the library");
    let discovery =
        rust_mutants::syntax::discover_file("src/lib.rs", &source, &selection).expect("discover");
    let by_rule = |rule: &str| {
        discovery
            .candidates
            .iter()
            .find(|found| found.candidate.rule.name == rule)
            .unwrap_or_else(|| panic!("a {rule} candidate"))
            .candidate
            .clone()
    };

    assert_eq!(
        prover
            .identical(&by_rule("add-to-sub"), &cancel)
            .expect("a comparison"),
        Identity::Identical,
        "`n + 0` and `n - 0` are the same instructions at opt-level 2"
    );
    assert_eq!(
        prover
            .identical(&by_rule("mul-to-div"), &cancel)
            .expect("a comparison"),
        Identity::Differs,
        "`n * 2` and `n / 2` are not"
    );
    assert!(
        !prover.withdrawn(),
        "and the original built to the same bytes every time it was asked to"
    );
    prover.close().expect("the tree goes away");
}

static REGISTRY: rust_mutants::rule::Registry = rust_mutants::rule::Registry::canonical();
