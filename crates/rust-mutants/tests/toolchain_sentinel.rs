// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every routing layer routes the mutant planted for it, and a planted expectation that does not hold is reported blind.

use rust_mutants::runner::Cancel;
use rust_mutants::sentinel::{Expectation, Expected, Planted, Planting};
use rust_mutants::session::{PrepareOptions, Proof};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

/// The options a caller that measures by the guards alone prepares its own tree with.
fn touching() -> PrepareOptions {
    PrepareOptions {
        coverage: false,
        ..PrepareOptions::default()
    }
}

#[test]
fn every_layer_routes_the_mutant_planted_for_it_and_leaves_the_one_beside_it() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let root = temp.path().join("planted");
    let sighted = rust_mutants::sentinel::sighted(
        &root,
        opening(&njutest_devkit::paths::cargo_binary(), temp.path()),
        &touching(),
        &Cancel::new(),
    )
    .expect("the planted crate prepares");

    let said: Vec<String> = sighted
        .sightings
        .iter()
        .map(|one| {
            format!(
                "{} {} expected {} routed {}",
                one.expectation.planted,
                one.expectation.mutant,
                one.expectation.expected,
                one.routed()
            )
        })
        .collect();
    assert!(
        sighted.blind().is_none(),
        "a layer that does not route what was planted for it removes nothing a run may believe: {said:#?}"
    );
    let layers: Vec<Planted> = sighted
        .sightings
        .iter()
        .map(|one| one.expectation.planted)
        .collect();
    for planted in Planted::every() {
        assert_eq!(
            layers.iter().filter(|one| **one == planted).count(),
            2,
            "{planted} is asked about the mutant it must remove and the one it must leave: {said:#?}"
        );
    }
    assert!(
        sighted.kept.is_empty(),
        "a workspace not opened to keep its directories keeps none: {:?}",
        sighted.kept
    );
}

#[test]
fn an_expectation_the_session_does_not_bear_out_is_blind_and_says_what_it_saw() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let root = temp.path().join("planted");
    rust_mutants::sentinel::materialise(&root).expect("the planted crate is written");
    let cancel = Cancel::new();
    let session = Workspace::open(
        &root,
        opening(&njutest_devkit::paths::cargo_binary(), temp.path()),
        &cancel,
    )
    .expect("the workspace opens")
    .prepare(&rust_mutants::sentinel::routing(&touching()), &cancel)
    .expect("the session prepares");

    let unreached_as_uninfected = rust_mutants::sentinel::sight(
        &session,
        Expectation {
            planted: Planted::Proof(Proof::NeverInfected),
            mutant: Planting::new("one", "return-default"),
            expected: Expected::Discharged(Proof::NeverInfected),
        },
    );
    assert!(
        !unreached_as_uninfected.sighted(),
        "a mutant nothing reaches was never infected by anything either, and a sentinel that \
         accepted that would pass a layer which discharges what it never measured"
    );
    assert_eq!(unreached_as_uninfected.routed(), "unreached");

    let removed_as_kept = rust_mutants::sentinel::sight(
        &session,
        Expectation {
            planted: Planted::Proof(Proof::BranchNeverTaken),
            mutant: Planting::new("clamp", "le-to-lt"),
            expected: Expected::Kept,
        },
    );
    assert!(
        !removed_as_kept.sighted(),
        "a mutant a proof removed is not one the tests are asked about"
    );
    assert_eq!(
        removed_as_kept.routed(),
        "discharged: sentinel/test/planted by branch-never-taken"
    );

    let absent = rust_mutants::sentinel::sight(
        &session,
        Expectation {
            planted: Planted::Reach,
            mutant: Planting::new("one", "no-such-rule"),
            expected: Expected::Unreached,
        },
    );
    assert!(
        !absent.sighted(),
        "a planted mutant the catalog does not hold checks nothing, so it cannot vouch for a layer"
    );
    assert!(
        absent.routed().starts_with("not routed: "),
        "{}",
        absent.routed()
    );
    session.close().expect("the session closes");
}
