// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The edition before the current one, which resolves paths differently.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

fn prepared(fixture: &Fixture) -> Session {
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open");
    workspace
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
fn an_edition_2021_crate_with_extern_crate_is_measured() {
    let fixture = Fixture::copy("fixture-2021");
    let session = prepared(&fixture);
    assert!(
        session.rejections().is_empty(),
        "the generated runtime module has to name std the way this edition resolves it: {:?}",
        session.rejections()
    );
    let one = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| one.candidate.rule.name == "gt-to-ge")
        .expect("a gt-to-ge mutant")
        .display_id
        .clone();
    let result = session
        .exec(&Request::new(one), &Cancel::new())
        .expect("exec");
    assert_eq!(result.outcome, Outcome::Killed);
    session.close().expect("close");
}
