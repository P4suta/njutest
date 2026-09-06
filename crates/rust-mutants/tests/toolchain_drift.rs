// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test that writes into the snapshot every later mutation is measured against.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepare(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
            env: std::env::vars_os().collect(),
            locked: true,
            offline: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            tier: Tier::All,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

#[test]
fn a_test_that_writes_into_the_tree_is_reported_as_drift() {
    let fixture = Fixture::copy("fixture-writes-tree");
    let session = prepare(&fixture);
    let mutant = session
        .catalog()
        .mutants()
        .first()
        .expect("the fixture catalogs a mutation")
        .clone();
    let cancel = Cancel::new();
    let _result = session
        .exec(&Request::new(mutant.id), &cancel)
        .expect("exec");
    let drift = session.changes().expect("changes");
    let written: Vec<String> = drift
        .iter()
        .map(|one| format!("{}: {}", one.kind().name(), one.rel_path()))
        .collect();
    assert!(
        written.iter().any(|one| one.contains("src/note.txt")),
        "the test wrote a file into the snapshot and nothing said so: {written:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_tree_nobody_wrote_to_drifts_in_nothing() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture);
    let mutant = session
        .catalog()
        .mutants()
        .first()
        .expect("the fixture catalogs a mutation")
        .clone();
    let cancel = Cancel::new();
    let _result = session
        .exec(&Request::new(mutant.id), &cancel)
        .expect("exec");
    let drift = session.changes().expect("changes");
    assert!(
        drift.is_empty(),
        "no test wrote into the tree: {:?}",
        drift
            .iter()
            .map(|one| (one.kind().name(), one.rel_path()))
            .collect::<Vec<_>>()
    );
    session.close().expect("close");
}
