// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The shapes a crate written today is made of, and what the engine can say about each.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::{Fixture, stated_fates};
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Session};
use rust_mutants::syntax::SkipReason;
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
fn every_construct_in_the_modern_fixture_is_discovered_and_measured() {
    let fixture = Fixture::copy("fixture-modern");
    let session = prepared(&fixture);

    let unsupported = session
        .skips()
        .iter()
        .filter(|skip| skip.reason == SkipReason::UnsupportedSite)
        .count();
    assert_eq!(
        unsupported, 0,
        "every place these rules target is a place a guard can be written"
    );

    let refused: Vec<&str> = session
        .rejections()
        .iter()
        .map(|one| one.rule.as_str())
        .collect();
    assert!(
        refused
            .iter()
            .all(|rule| *rule == "return-default" || *rule == "return-some-default"),
        "the only candidates the compiler refuses here are return replacements on a return \
         type the syntax cannot default; anything else is a shape the walker got wrong: \
         {refused:?}"
    );

    let stated = stated_fates("fixture-modern");
    assert!(stated.stated, "the README states what a run of it does");
    assert_eq!(
        stated.rows.len(),
        session
            .accepted()
            .len()
            .saturating_add(session.rejections().len()),
        "the fate ledger accounts for every mutant the compiler took and every one it refused"
    );
    session.close().expect("close");
}
