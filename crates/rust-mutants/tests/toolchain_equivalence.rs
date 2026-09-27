// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What two real builds render, compared byte for byte.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::catalog::Candidate;
use rust_mutants::equivalence::artifacts::Identity;
use rust_mutants::equivalence::{ProveOptions, Prover};
use rust_mutants::runner::Cancel;
use rust_mutants::testkit::opening::opening;
use rust_mutants::trace::Recorder;
use rust_mutants::workspace::OpenOptions;

static REGISTRY: rust_mutants::rule::Registry = rust_mutants::rule::Registry::canonical();

fn prover(fixture: &Fixture, cancel: &Cancel) -> Prover {
    prover_with_env(fixture, cancel, toolchain_env())
}

fn toolchain_env() -> rust_mutants::vars::Variables {
    njutest_devkit::paths::environment_for_a_toolchain_run(&[])
        .into_iter()
        .collect()
}

fn prover_with_env(
    fixture: &Fixture,
    cancel: &Cancel,
    env: rust_mutants::vars::Variables,
) -> Prover {
    Prover::open(
        fixture.root(),
        &ProveOptions {
            open: OpenOptions {
                cargo: Some(njutest_devkit::paths::cargo_binary()),
                env,
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
fn equivalence_builds_leave_an_ambient_cargo_target_directory_untouched() {
    let fixture = Fixture::copy("fixture-equivalent");
    let ambient = fixture.temp().join("ambient-cargo-target");
    std::fs::create_dir_all(&ambient).expect("the ambient target directory");
    let marker = ambient.join("untouched");
    std::fs::write(&marker, b"outside the prover").expect("the marker");
    let mut env = toolchain_env();
    env.set("CARGO_TARGET_DIR", ambient.as_os_str());
    env.set("RUSTC_WRAPPER", "");
    let cancel = Cancel::new();
    let mut prover = prover_with_env(&fixture, &cancel, env);
    let source = std::fs::read(fixture.root().join("src/lib.rs")).expect("the library");
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let candidate = rust_mutants::syntax::discover_file("src/lib.rs", &source, &selection)
        .expect("discover")
        .candidates
        .into_iter()
        .find(|found| found.candidate.rule.name == "mul-to-div")
        .expect("the rendered mutation")
        .candidate;
    prover
        .identical(&candidate, &cancel)
        .expect("the mutated build");
    let own_target = rust_mutants::workspace::target_of(fixture.temp(), fixture.root())
        .join("equivalence")
        .join("debug");
    assert!(
        std::fs::metadata(own_target)
            .expect("the prover's target directory")
            .is_dir(),
        "the prover built inside its own target directory"
    );
    prover.close().expect("the prover closes");
    let entries: Vec<_> = std::fs::read_dir(&ambient)
        .expect("the ambient directory remains")
        .map(|entry| entry.expect("an ambient entry can be read").file_name())
        .collect();
    assert_eq!(
        entries,
        [std::ffi::OsString::from("untouched")],
        "the ambient target directory holds only its original marker"
    );
    assert_eq!(
        std::fs::read(marker).expect("the marker remains"),
        b"outside the prover"
    );
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
            .expect("the requested rule has a candidate")
            .candidate
            .clone()
    };

    let rendered = prover
        .identical(&by_rule("mul-to-div"), &cancel)
        .expect("a comparison");
    assert_eq!(
        prover.withdrawn(),
        !njutest_devkit::reproducible::builds_a_reverted_change_to_the_same_bytes(),
        "what this layer can speak for is what the machine builds the same way twice, and \
         the suites that read it end to end ask that question the same way"
    );
    if prover.withdrawn() {
        assert_eq!(
            rendered,
            Identity::NotEstablished(rust_mutants::equivalence::CONTROL_DRIFTED),
            "a machine that renders one unchanged tree two ways is one this layer says \
             nothing about: reporting a difference it did not cause would be reporting \
             the machine"
        );
        prover.close().expect("the tree goes away");
        return;
    }

    assert_eq!(rendered, Identity::Differs, "`n * 2` and `n / 2` are not");
    assert_eq!(
        prover
            .identical(&by_rule("add-to-sub"), &cancel)
            .expect("a comparison"),
        Identity::Identical,
        "`n + 0` and `n - 0` are the same instructions at opt-level 2"
    );
    assert!(
        !prover.withdrawn(),
        "and the original built to the same bytes every time it was asked to"
    );
    prover.close().expect("the tree goes away");
}

#[test]
fn a_mutation_whose_tree_does_not_build_is_not_established_for_that_reason() {
    let fixture = Fixture::copy("fixture-equivalent");
    let cancel = Cancel::new();
    let mut prover = Prover::open(
        fixture.root(),
        &ProveOptions {
            open: opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
            ..ProveOptions::default()
        },
        &cancel,
        &Recorder::disabled(),
    )
    .expect("the prover opens");

    let refused = Candidate {
        path: "src/lib.rs".to_owned(),
        rule: rust_mutants::rule::Registry::canonical()
            .lookup("return-default")
            .expect("a rule"),
        span: rust_mutants::span::Span::new(0, 2).expect("a span"),
        original: b"//".to_vec(),
        replacement: b"}{".to_vec(),
        source_digest: "0".repeat(64),
    };
    let answer = prover.identical(&refused, &cancel).expect("an answer");
    assert_eq!(
        answer,
        Identity::NotEstablished(rust_mutants::equivalence::DOES_NOT_BUILD),
        "a mutation the compiler refuses is not one it renders identically: the question is \
         about two programs and there is only one"
    );
    prover.close().expect("close");
}
