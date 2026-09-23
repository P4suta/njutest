// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run compiles decides what it can measure.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::cargo::BuildConfig;
use rust_mutants::id::DisplayId;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Reachability, Request, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

fn prepared(fixture: &Fixture, build: BuildConfig) -> Session {
    let workspace = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open");
    workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                build,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare")
}

/// The mutation of `feet`, which only a run that turned `imperial` on ever tests.
fn imperial(session: &Session) -> DisplayId {
    session
        .catalog()
        .mutants()
        .iter()
        .find(|one| {
            one.candidate.rule.name == "div-to-mul"
                && session.position(one).is_some_and(|at| at.line == 13)
        })
        .expect("a div-to-mul mutant in feet")
        .display_id
        .clone()
}

#[test]
fn a_test_behind_a_feature_is_measured_only_when_the_feature_is_on() {
    let cancel = Cancel::new();

    let defaults = Fixture::copy("fixture-features");
    let session = prepared(&defaults, BuildConfig::default());
    let mutant = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| one.display_id == imperial(&session))
        .expect("the mutant")
        .clone();
    let reaches = session.reaches(&mutant);
    let outcome = session
        .exec(&Request::new(mutant.display_id.to_string()), &cancel)
        .expect("exec")
        .outcome();
    session.close().expect("close");
    assert_eq!(
        reaches,
        Reachability::Unreached,
        "the feature is off, so the only test that calls the function is not compiled, and the \
         measurement says as much"
    );
    assert_eq!(
        outcome,
        Outcome::NotRun,
        "and a mutation no measured test reaches is one nothing runs to find out again"
    );

    let asked = Fixture::copy("fixture-features");
    let session = prepared(
        &asked,
        BuildConfig {
            features: vec!["imperial".to_owned()],
            ..BuildConfig::default()
        },
    );
    let request = Request::new(imperial(&session).to_string());
    let outcome = session.exec(&request, &cancel).expect("exec").outcome();
    session.close().expect("close");
    assert_eq!(
        outcome,
        Outcome::Killed,
        "with the feature on the same edit is compiled and the same suite runs it"
    );
}
