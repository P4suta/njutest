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

/// A cargo that dates `path` of the tree it runs in back to 2000 and then runs the toolchain's own, which is what a clock that disagrees with the file's time looks like to cargo: it keeps what it built before and says the unit is fresh.
#[cfg(unix)]
fn dating_cargo(dir: &std::path::Path, path: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(dir).expect("the wrapper's directory");
    let wrapper = dir.join("cargo");
    let cargo = njutest_devkit::paths::cargo_binary();
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ntouch -c -t 200001010000 '{path}'\nexec '{}' \"$@\"\n",
            cargo.display()
        ),
    )
    .expect("the wrapper");
    let mut permissions = std::fs::metadata(&wrapper)
        .expect("the wrapper's metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).expect("the wrapper runs");
    std::os::unix::fs::symlink(cargo.with_file_name("rustc"), dir.join("rustc"))
        .expect("the toolchain's rustc beside the wrapper");
    wrapper
}

#[test]
#[cfg(unix)]
fn a_spliced_unit_cargo_reused_is_never_compared() {
    let fixture = Fixture::copy("fixture-shared-path");
    let cancel = Cancel::new();
    let mut prover = Prover::open(
        fixture.root(),
        &ProveOptions {
            open: OpenOptions {
                cargo: Some(dating_cargo(
                    &fixture.temp().join("dating"),
                    "shared/util.rs",
                )),
                env: toolchain_env(),
                temp_directory: fixture.temp().to_path_buf(),
                locked: true,
                offline: true,
                ..OpenOptions::default()
            },
            ..ProveOptions::default()
        },
        &cancel,
        &Recorder::disabled(),
    )
    .expect("the tree is copied and built");
    let source = std::fs::read(fixture.root().join("shared/util.rs")).expect("the shared file");
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let candidate = rust_mutants::syntax::discover_file("shared/util.rs", &source, &selection)
        .expect("discover")
        .candidates
        .into_iter()
        .find(|found| found.candidate.rule.name == "le-to-lt")
        .expect("a mutation the compiler renders at every level")
        .candidate;
    let answer = prover.identical(&candidate, &cancel).expect("an answer");
    prover.close().expect("the tree goes away");
    assert_eq!(
        answer,
        Identity::NotEstablished(rust_mutants::equivalence::NOT_RECOMPILED),
        "`n <= bound` and `n < bound` are two programs, and cargo said every unit that read the \
         spliced file was fresh, so the executables compared are the ones it built before the \
         splice: comparing them says nothing about the mutation"
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

#[test]
fn an_original_that_does_not_build_cannot_establish_a_mutation() {
    let fixture = Fixture::copy("fixture-equivalent");
    let path = fixture.root().join("src/lib.rs");
    let mut source = std::fs::read(&path).expect("the original source");
    let refusal = b"\ncompile_error!(\"the original does not build\");\n";
    let start = u32::try_from(source.len()).expect("the source length fits a span");
    let end = start
        .checked_add(u32::try_from(refusal.len()).expect("the refusal length fits a span"))
        .expect("the span fits u32");
    source.extend_from_slice(refusal);
    std::fs::write(&path, &source).expect("the original refuses to build");
    let candidate = Candidate {
        path: "src/lib.rs".to_owned(),
        rule: REGISTRY.lookup("return-default").expect("a rule"),
        span: rust_mutants::span::Span::new(start, end).expect("the refusal span"),
        original: refusal.to_vec(),
        replacement: Vec::new(),
        source_digest: "0".repeat(64),
    };
    let cancel = Cancel::new();
    let mut prover = prover(&fixture, &cancel);
    assert_eq!(
        prover.identical(&candidate, &cancel).expect("an answer"),
        Identity::NotEstablished(rust_mutants::equivalence::DID_NOT_BUILD),
        "a mutation that removes the compile failure cannot be compared with an original that never built"
    );
    prover.close().expect("close");
}
