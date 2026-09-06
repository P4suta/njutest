// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The branch proofs a real workspace earns, and the ones it does not.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::PrepareOptions;
use rust_mutants::workspace::{OpenOptions, Workspace};

/// A prepared session over `fixture`, measuring coverage or not.
fn prepared(fixture: &Fixture, coverage: bool) -> rust_mutants::session::Session {
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
            env: std::env::vars_os().collect(),
            offline: true,
            locked: true,
            ..OpenOptions::default()
        },
        &cancel,
    )
    .expect("the workspace opens");
    workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                coverage,
                branch_proofs: true,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares")
}

#[test]
fn the_compiler_vouches_for_a_condition_of_primitives_and_refuses_the_rest() {
    let fixture = Fixture::copy("fixture-coverage");
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
            env: std::env::vars_os().collect(),
            offline: true,
            locked: true,
            ..OpenOptions::default()
        },
        &cancel,
    )
    .expect("the workspace opens");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares");

    let proven: Vec<String> = session
        .accepted()
        .iter()
        .filter(|index| session.branch(**index).is_some())
        .filter_map(|index| session.catalog().by_index(*index))
        .map(|mutant| {
            format!(
                "{} {}",
                mutant.candidate.rule.name,
                session.position(mutant).map_or(0, |position| position.line)
            )
        })
        .collect();
    assert!(
        proven.iter().any(|one| one.starts_with("le-to-lt 8")),
        "a condition of primitives earns its proof: {proven:?}"
    );
    assert!(
        !proven.iter().any(|one| one.starts_with("le-to-lt 16")),
        "a comparison the compiler will not vouch for earns none: {proven:?}"
    );
    assert!(
        !proven.iter().any(|one| one.starts_with("le-to-lt 28")),
        "a condition that runs the program's code earns none: {proven:?}"
    );
    assert!(session.proven() > 0, "{proven:?}");

    let proof = session
        .accepted()
        .iter()
        .find_map(|index| session.branch(*index))
        .expect("a proof");
    assert!(
        proof.body_start.line < proof.body_end.line,
        "the proof names the body it gates: {proof:?}"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_witnessed_tree_is_put_back_before_anything_is_instrumented() {
    let fixture = Fixture::copy("fixture-coverage");
    let before = std::fs::read_to_string(fixture.root().join("src/lib.rs")).expect("the source");
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
            env: std::env::vars_os().collect(),
            offline: true,
            locked: true,
            ..OpenOptions::default()
        },
        &cancel,
    )
    .expect("the workspace opens");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares");
    assert_eq!(
        std::fs::read_to_string(fixture.root().join("src/lib.rs")).expect("the source"),
        before,
        "the source tree is read-only, whatever the engine writes into its own copy"
    );
    let instrumented =
        std::fs::read_to_string(session.snapshot_root().join("src/lib.rs")).expect("the copy");
    assert!(
        !instrumented.contains("rust-mutants-witness-v1"),
        "the witness tree is taken out before anything is instrumented"
    );
    assert!(
        instrumented.contains("::active("),
        "and what is left is the instrumented tree"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_target_that_never_ran_the_body_is_discharged_by_the_branch_proof() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture, true);
    let mut discharged = 0usize;
    let mut named = Vec::new();
    for mutant in session.catalog().mutants() {
        let route = session.route(mutant);
        for one in route.discharged() {
            discharged = discharged.saturating_add(1);
            named.push(one.proof);
        }
    }
    assert!(
        session.proven() > 0,
        "the fixture holds branches the compiler vouches for"
    );
    assert!(
        discharged > 0,
        "a target during which no statement of the named body ran cannot have observed the \
         mutation, and running it proves nothing and costs a process"
    );
    assert!(
        named
            .iter()
            .all(|proof| *proof == rust_mutants::session::BRANCH_NEVER_TAKEN),
        "{named:?}"
    );
    session.close().expect("close");
}

#[test]
fn exec_never_discharges_and_keeps_its_coverage_narrowing() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture, true);
    let discharging = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| !session.route(one).discharged().is_empty())
        .expect("a mutant a proof removes a target from");
    let route = session.route(discharging);
    let ran = session
        .exec(
            &rust_mutants::session::Request::new(discharging.display_id.clone()),
            &Cancel::new(),
        )
        .expect("exec");
    let narrowing = route.narrowing().expect("a measured route");
    assert!(
        narrowing.iter().any(|target| target == &ran.target),
        "exec is the question a caller with its own evidence asks, and a discharge is a proof \
         it may not share: {narrowing:?} ran {}",
        ran.target
    );
    assert!(
        route
            .discharged()
            .iter()
            .all(|one| narrowing.contains(&one.target)),
        "so what exec narrows to still holds every target the measurement placed"
    );
    session.close().expect("close");
}
