// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What two real builds render, compared byte for byte.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::catalog::Candidate;
use rust_mutants::equivalence::artifacts::{Artifacts, Identity};
use rust_mutants::equivalence::{ProveOptions, Prover};
use rust_mutants::runner::Cancel;
use rust_mutants::testkit::opening::opening;
use rust_mutants::trace::Recorder;
use rust_mutants::workspace::OpenOptions;

static REGISTRY: rust_mutants::rule::Registry = rust_mutants::rule::Registry::canonical();

fn prover(fixture: &Fixture, cancel: &Cancel) -> Prover {
    Prover::open(
        fixture.root(),
        &ProveOptions {
            open: OpenOptions {
                cargo: Some(njutest_devkit::paths::cargo_binary()),
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

    let rendered = prover
        .identical(&by_rule("mul-to-div"), &cancel)
        .expect("a comparison");
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

/// What one build of the tree at `root` produced, by target name, each executable digested.
///
/// Built the way the layer builds its own tree: without the incremental state
/// that would make what the compiler emits depend on what it emitted before.
fn produced(
    cargo: &std::path::Path,
    root: &std::path::Path,
    target: &std::path::Path,
) -> Artifacts {
    let mut command = njutest_devkit::paths::command(cargo);
    let _configured = command
        .env("CARGO_INCREMENTAL", "0")
        .args(["test", "--no-run", "--message-format=json", "--offline"])
        .arg("--locked")
        .arg("--target-dir")
        .arg(target)
        .current_dir(root);
    let said = command.output().expect("cargo runs");
    assert!(
        said.status.success(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );
    let messages = rust_mutants::cargo::parse_messages(&said.stdout).expect("the message stream");
    let mut executables = Vec::new();
    for message in &messages {
        if let rust_mutants::cargo::Message::CompilerArtifact(artifact) = message
            && let Some(executable) = &artifact.executable
        {
            executables.push((artifact.target.name.as_str(), executable.as_path()));
        }
    }
    assert!(!executables.is_empty(), "a build produced no executable");
    rust_mutants::equivalence::artifacts::digests(executables).expect("the executables are read")
}

#[test]
fn a_machine_this_layer_speaks_for_is_one_that_builds_a_tree_to_the_same_bytes_twice() {
    let fixture = Fixture::copy("fixture-equivalent");
    let cargo = njutest_devkit::paths::cargo_binary();
    let target = fixture.temp().join("twice");
    let source = fixture.root().join("src/lib.rs");
    let original = std::fs::read(&source).expect("the library");

    let first = produced(&cargo, fixture.root(), &target);
    let mut changed = original.clone();
    changed.extend_from_slice(b"\npub const A_THING_NOTHING_READS: u8 = 7;\n");
    std::fs::write(&source, &changed).expect("the changed library");
    let _between = produced(&cargo, fixture.root(), &target);
    std::fs::write(&source, &original).expect("the library back");
    let again = produced(&cargo, fixture.root(), &target);

    assert_eq!(
        first, again,
        "every answer this layer gives reads the difference between two builds as the \
         mutation's doing, so a machine that renders one unchanged tree two ways renders \
         every mutation differently and the layer would be reporting the machine"
    );
}
