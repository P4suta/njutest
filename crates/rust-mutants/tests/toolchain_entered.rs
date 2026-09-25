// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a mutant execution records the union of items its whole process entered, which is what a carried answer rests on (ADR 0041).

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::glob::Pattern;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Recording, Request, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::touch::{Completeness, ItemRef};
use rust_mutants::workspace::Workspace;

fn prepare(fixture: &Fixture) -> Session {
    prepare_narrowed(fixture, Vec::new())
}

fn prepare_narrowed(fixture: &Fixture, narrowing: Vec<Pattern>) -> Session {
    Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            tier: Tier::All,
            touch: true,
            narrowing,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

/// The portable name of the item called `name`, as the session's own item catalog numbers it.
fn named(session: &Session, name: &str) -> ItemRef {
    let items = &session.touched().items;
    let item = items
        .iter()
        .find(|one| one.name == name)
        .expect("the fixture's item");
    let first = items
        .iter()
        .filter(|one| one.path == item.path)
        .map(|one| one.index)
        .min()
        .expect("its file's first item");
    ItemRef {
        package: item.package.clone(),
        path: item.path.clone(),
        ordinal: item
            .index
            .checked_sub(first)
            .expect("an item after its file's first"),
    }
}

/// The items the process entered when `mutant` ran with recording asked for.
fn entered_under(session: &Session, rule: &str, inside: &str) -> rust_mutants::touch::Entered {
    let mutant = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| one.candidate.rule.name == rule && session.item_of(one.index) == Some(inside))
        .expect("the fixture's mutation");
    let result = session
        .exec(
            &Request::new(mutant.id.to_string()).recording(Recording::Items),
            &Cancel::new(),
        )
        .expect("exec");
    result
        .entered
        .expect("an execution asked to record what it entered records it")
}

#[test]
fn every_mutant_execution_names_the_items_it_entered() {
    let fixture = Fixture::copy("fixture-entered");
    let session = prepare(&fixture);
    let rare = named(&session, "rare");

    let forced = entered_under(&session, "condition-to-true", "pick");
    assert!(
        forced.items.contains(&rare),
        "a mutation that forces the branch the tests never take enters the item behind it, and \
         the union says so: {forced:?}"
    );
    assert_eq!(
        forced.completeness,
        Completeness::Whole,
        "a process that ran to its end accounts for everything it entered"
    );
    assert!(
        forced.records >= 1
            && usize::try_from(forced.records)
                .is_ok_and(|records| records <= forced.items.len() * 4),
        "and says what recording it cost, in records written, which stays a small multiple of \
         the items named: {} records for {} items",
        forced.records,
        forced.items.len()
    );

    let elsewhere = entered_under(&session, "return-default", "common");
    assert!(
        !elsewhere.items.contains(&rare),
        "and a mutation that leaves the branch alone never reaches it: {elsewhere:?}"
    );
    assert_eq!(
        elsewhere.completeness,
        Completeness::UpToFirstFailure,
        "and a kill's union is claimed only up to its failure, however the process ended, \
         because a process may be stopped there: {elsewhere:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_change_set_narrows_what_is_mutated_and_never_what_is_marked() {
    let fixture = Fixture::copy("fixture-outside");
    let whole = prepare(&fixture);
    let narrowed = prepare_narrowed(
        &fixture,
        vec![Pattern::compile("src/lib.rs").expect("a pattern")],
    );
    assert!(
        narrowed
            .catalog()
            .mutants()
            .iter()
            .all(|mutant| mutant.candidate.path == "src/lib.rs"),
        "a change set still decides what is mutated"
    );
    assert_eq!(
        narrowed.touched().items,
        whole.touched().items,
        "and every file the configuration selects carries its entry markers whatever the change \
         set, so what an execution entered is named the same way on a pull request as on the \
         whole run it is carried from"
    );
    let marked = std::fs::read_to_string(narrowed.snapshot_root().join("src/spin.rs"))
        .expect("the instrumented file");
    let pristine =
        std::fs::read_to_string(fixture.root().join("src/spin.rs")).expect("the pristine file");
    assert_ne!(
        marked, pristine,
        "and a file the change set left out is instrumented for entry all the same"
    );
    narrowed.close().expect("close");
    whole.close().expect("close");
}
