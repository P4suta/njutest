// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A fault fails the call a `?` asks about, and the suite is asked whether it noticed (ADR 0032).

use std::collections::BTreeMap;

use njutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

#[test]
fn a_failed_call_is_noticed_where_a_test_checks_it_and_refused_where_nothing_can_be_made() {
    let fixture = Fixture::copy("fixture-faulted");
    let session = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            operators: vec!["inject-error".to_owned()],
            touch: true,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let rejected: Vec<&str> = session
        .rejections()
        .iter()
        .map(|rejection| rejection.id.as_str())
        .collect();
    let mut decided: BTreeMap<String, String> = BTreeMap::new();
    for mutant in session.catalog().mutants() {
        assert_eq!(mutant.candidate.rule.name, "inject-error", "{mutant:?}");
        let item = session
            .item_of(mutant.index)
            .expect("every fault site is inside an item")
            .to_owned();
        assert!(
            session.route(mutant).discharged().is_empty(),
            "a proof read off the run without the fault says nothing past the site the fault \
             changes, so nothing discharges a fault: {:?}",
            session.route(mutant)
        );
        let answer = if rejected.contains(&mutant.id.as_str()) {
            "not-put".to_owned()
        } else {
            let result = session
                .exec(&Request::new(mutant.id.to_string()), &Cancel::new())
                .expect("the fault runs");
            match result.outcome() {
                Outcome::Killed => "noticed".to_owned(),
                Outcome::Survived => "unnoticed".to_owned(),
                other @ (Outcome::NotRun
                | Outcome::StepLimitReached
                | Outcome::Waited
                | Outcome::Inconclusive
                | Outcome::Errored) => other.name().to_owned(),
            }
        };
        decided.insert(item, answer);
    }
    assert_eq!(
        decided,
        BTreeMap::from([
            ("load".to_owned(), "noticed".to_owned()),
            ("number".to_owned(), "noticed".to_owned()),
            ("measured".to_owned(), "unnoticed".to_owned()),
            ("ours".to_owned(), "not-put".to_owned()),
            ("maybe".to_owned(), "not-put".to_owned()),
        ]),
        "a read that fails is seen by the test that checks it and by nobody where the answer is \
         thrown away; an error type the engine cannot make, and an `Option`, are never guessed \
         at"
    );
    session.close().expect("close");
}
