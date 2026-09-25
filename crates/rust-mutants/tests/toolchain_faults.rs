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

#[test]
fn a_survivor_is_told_apart_only_with_the_fault_at_its_own_site_beside_it() {
    let fixture = Fixture::copy("fixture-faulted");
    let session = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            operators: vec!["question-to-unwrap".to_owned(), "inject-error".to_owned()],
            touch: true,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let in_measured = |rule: &str| {
        session
            .catalog()
            .mutants()
            .iter()
            .find(|mutant| {
                mutant.candidate.rule.name == rule
                    && session.item_of(mutant.index) == Some("measured")
            })
            .unwrap_or_else(|| panic!("{rule} has a site in `measured`"))
            .id
            .to_string()
    };
    let (unwrapped, fault) = (
        in_measured("question-to-unwrap"),
        in_measured("inject-error"),
    );
    let beside = session
        .fault_beside(session.resolve(&unwrapped).expect("the survivor resolves"))
        .map(|one| one.id.to_string());
    assert_eq!(
        beside.as_deref(),
        Some(fault.as_str()),
        "the fault at the call `.unwrap()` keeps is the one carried into its alternative"
    );
    let outcome = |request: Request| {
        session
            .exec(&request, &Cancel::new())
            .expect("the execution runs")
            .outcome()
    };
    assert_eq!(
        outcome(Request::new(unwrapped.clone())),
        Outcome::Survived,
        "while the read succeeds, `?` and `.unwrap()` do the same thing"
    );
    assert_eq!(
        outcome(Request::new(fault.clone())),
        Outcome::Survived,
        "the caller throws the answer away, so nothing notices the read failing"
    );
    assert_eq!(
        outcome(Request::new(unwrapped).with_fault(fault.clone())),
        Outcome::Killed,
        "with the read failing, `.unwrap()` panics where `?` returned the error, and the test \
         that never checked the answer fails"
    );
    let refused = session.exec(
        &Request::new(fault.clone()).with_fault(fault),
        &Cancel::new(),
    );
    assert!(
        refused.is_err(),
        "a fault is put beside a mutation, never beside a fault or in place of one: {refused:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_fault_the_instrumentation_did_not_carry_into_a_branch_is_refused_beside_it() {
    let fixture = Fixture::copy("fixture-faulted");
    let session = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            operators: vec!["question-to-unwrap".to_owned(), "inject-error".to_owned()],
            touch: true,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let in_item = |rule: &str, item: &str| {
        session
            .catalog()
            .mutants()
            .iter()
            .find(|one| one.candidate.rule.name == rule && session.item_of(one.index) == Some(item))
            .unwrap_or_else(|| panic!("{rule} has a site in `{item}`"))
            .id
            .to_string()
    };
    let unwrapped = in_item("question-to-unwrap", "measured");
    let elsewhere = in_item("inject-error", "load");
    let uncarried = session.exec(
        &Request::new(unwrapped).with_fault(elsewhere),
        &Cancel::new(),
    );
    assert!(
        uncarried.is_err(),
        "a fault the instrumentation did not carry into this branch would activate nothing, so \
         asking for it is refused rather than run as the mutation alone: {uncarried:?}"
    );
    session.close().expect("close");
}
