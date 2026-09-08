// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What preparing does about a target whose own tests do not pass with nothing active.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::EngineError;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Failing, PrepareOptions, Request, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::{OpenOptions, SessionError, Workspace};

fn open(fixture: &Fixture) -> Workspace {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            offline: true,
            locked: true,
            ..opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open")
}

fn prepare(fixture: &Fixture, failing: Failing) -> Result<Session, EngineError> {
    open(fixture).prepare(
        &PrepareOptions {
            tier: Tier::All,
            failing,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
}

#[test]
fn refusing_names_every_target_that_failed_rather_than_the_one_it_reached_first() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let error = prepare(&fixture, Failing::Refuse).expect_err("refused");
    let EngineError::Session(SessionError::VerifyFailed { targets, output }) = error else {
        panic!("a tree whose baseline fails is refused by RM5002: {error}");
    };
    assert_eq!(
        targets,
        vec![
            "fixture-verify-fails/lib/fixture_verify_fails".to_owned(),
            "fixture-verify-fails/test/beside".to_owned(),
        ],
        "verification does not stop at the first failure, because somebody is about to fix \
         what it names"
    );
    assert!(
        output.contains("doubling_two_is_five"),
        "and it quotes the first of them: {output}"
    );
}

#[test]
fn excluding_hands_back_the_table_of_what_every_target_came_to() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let session = prepare(&fixture, Failing::Exclude)
        .expect("excluding does not refuse, because the caller asked for the table");
    let verified = session.verified();
    assert_eq!(
        verified.failing(),
        vec![
            "fixture-verify-fails/lib/fixture_verify_fails",
            "fixture-verify-fails/test/beside",
        ],
        "every target of this fixture fails, and the caller is told which"
    );
    for target in verified.failing() {
        assert!(
            verified.targets.contains_key(target),
            "the table covers every target that ran, not only the ones that passed"
        );
        assert!(
            session.targets().iter().all(|kept| kept.id != target),
            "{target} was left out of the run rather than measured against"
        );
    }
    for target in verified.failing() {
        assert!(
            verified
                .touched
                .limitations
                .contains(&format!("baseline-not-passing:{target}")),
            "the record says why {target} is not in it: {:?}",
            verified.touched.limitations
        );
    }
}

#[test]
fn a_session_whose_every_target_was_excluded_refuses_to_run_rather_than_score_nothing() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let session = prepare(&fixture, Failing::Exclude).expect("prepare");
    let mutant = session.catalog().mutants()[0].display_id.clone();
    let error = session
        .exec(&Request::new(mutant), &Cancel::new())
        .expect_err("a run of no targets is not a run that found nothing");
    assert!(
        matches!(error, EngineError::Session(SessionError::NoTargets { .. })),
        "{error}"
    );
}

#[test]
fn a_tree_that_passes_reports_a_baseline_for_every_target_it_ran() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture, Failing::Refuse).expect("prepare");
    let verified = session.verified();
    assert!(
        verified.failing().is_empty(),
        "nothing of this fixture fails: {:?}",
        verified.failing()
    );
    assert!(
        !verified.targets.is_empty(),
        "the run asked every target and kept what each came to"
    );
    for (target, baseline) in &verified.targets {
        assert!(baseline.passed(), "{target} passed");
        assert!(
            baseline.output.is_empty(),
            "{target} passed, so nobody reads what it printed"
        );
        assert_eq!(
            session.tests_of(target).max(1),
            baseline.tests.max(1),
            "what one target's baseline ran is what asking the whole of it costs"
        );
    }
}

#[test]
fn a_target_whose_every_test_is_ignored_says_so_rather_than_saying_nothing() {
    let fixture = Fixture::copy("fixture-ignored");
    let session = prepare(&fixture, Failing::Refuse).expect("prepare");
    let baseline = session
        .verified()
        .targets
        .get("fixture-ignored/lib/fixture_ignored")
        .expect("the library target was verified");
    assert_eq!(
        baseline.outcome,
        rust_mutants::outcome::Outcome::Inconclusive,
        "a target that ran no test decided nothing"
    );
    assert_eq!(baseline.tests, 0, "it ran none");
    assert_eq!(
        baseline.ignored, 2,
        "and it says how many it was told to skip, which is what tells it from a harness \
         that printed no summary at all"
    );
}
