// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What two real builds render, compared byte for byte.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::equivalence::artifacts::Identity;
use rust_mutants::equivalence::{ProveOptions, Prover};
use rust_mutants::runner::Cancel;
use rust_mutants::trace::Recorder;
use rust_mutants::workspace::OpenOptions;

static REGISTRY: rust_mutants::rule::Registry = rust_mutants::rule::Registry::canonical();

fn prover(fixture: &Fixture, cancel: &Cancel) -> Prover {
    Prover::open(
        fixture.root(),
        &ProveOptions {
            open: OpenOptions {
                cargo: Some(mjutest_devkit::paths::cargo_binary()),
                env: std::env::vars_os().collect(),
                temp_directory: fixture.temp().to_path_buf(),
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
    let fixture = Fixture::copy("fixture-equivalent");
    let cancel = Cancel::new();
    let mut prover = prover(&fixture, &cancel);
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let source = std::fs::read(fixture.root().join("src/lib.rs")).expect("the library");
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
