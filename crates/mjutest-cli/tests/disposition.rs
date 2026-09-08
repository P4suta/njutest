// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each disposition makes a reader act on, and what an acceptance can answer for.

use std::collections::{BTreeMap, BTreeSet};

use mjutest_cli::assure::mutation::{Disposition, Judged, Mutation, Unconfirmed};
use mjutest_cli::assure::route::{BRANCH_NEVER_TAKEN, Discharge, NEVER_INFECTED, Reaches, Route};
use mjutest_cli::report::FindingKind;

fn discharge(target: &str, proof: &'static str) -> Discharge {
    Discharge {
        target: target.to_owned(),
        proof,
    }
}

const MUTANT: &str = "aaaaaaaaaaaaaaaaaaaa";

fn judged(disposition: Disposition) -> Judged {
    Judged {
        id: "a".repeat(64),
        display_id: MUTANT.to_owned(),
        path: "src/lib.rs".to_owned(),
        rule: "negate-condition@1".to_owned(),
        position: None,
        disposition,
        source_run_id: None,
    }
}

fn phase(disposition: Disposition) -> Mutation {
    Mutation {
        judged: vec![judged(disposition)],
        skips: BTreeMap::new(),
    }
}

fn accepted() -> BTreeSet<String> {
    BTreeSet::from(["a".repeat(64)])
}

#[test]
fn a_mutation_that_ran_out_of_time_is_a_gap_the_run_reports() {
    let phase = phase(Disposition::TimedOut {
        on: "pkg/test/lib does_not_finish".to_owned(),
    });

    let findings = phase.findings(&BTreeSet::new());

    assert_eq!(
        findings.len(),
        1,
        "a timeout is not a proof about the mutant, so it is not something a run passes over"
    );
    let raised = findings.first().expect("the finding a timeout raises");
    assert_eq!(raised.kind, FindingKind::Timeout);
    assert!(
        !raised.kind.is_defect(),
        "an expired budget says nothing about the code under test"
    );
}

#[test]
fn an_acceptance_does_not_answer_for_a_pair_that_did_not_agree() {
    let phase = phase(Disposition::Unconfirmed {
        on: "pkg/test/lib adds_two_numbers".to_owned(),
        why: Unconfirmed::DidNotReproduce,
    });

    let findings = phase.findings(&accepted());

    assert_eq!(
        findings.len(),
        1,
        "an acceptance answers for a mutation nothing noticed, and an inconclusive outcome is \
         not one"
    );
}

#[test]
fn an_acceptance_does_not_answer_for_a_harness_that_could_not_run() {
    let phase = phase(Disposition::Errored {
        on: "pkg/test/lib adds_two_numbers".to_owned(),
        detail: "the binary is not there".to_owned(),
    });

    let findings = phase.findings(&accepted());

    assert_eq!(
        findings.len(),
        1,
        "a mutation nobody measured is not a mutation a reviewer can answer for"
    );
}

#[test]
fn an_acceptance_answers_for_a_mutation_every_reaching_test_passed() {
    let phase = phase(Disposition::Unreached);

    let findings = phase.findings(&accepted());

    assert!(
        findings.is_empty(),
        "a mutation nothing noticed is exactly what an acceptance is for: {findings:?}"
    );
}

#[test]
fn an_acceptance_does_not_answer_for_a_mutation_the_clock_cut_short() {
    let phase = phase(Disposition::TimedOut {
        on: "pkg/test/lib does_not_finish".to_owned(),
    });

    let findings = phase.findings(&accepted());

    assert_eq!(
        findings.len(),
        1,
        "an acceptance is a reviewer saying a mutation nothing noticed is one nothing \
         needs to notice, and a run that gave up on the clock did not establish that \
         nothing noticed it: it established nothing at all: {findings:?}"
    );
}

#[test]
fn a_survivor_no_test_could_have_noticed_says_so_and_names_the_proofs() {
    let phase = phase(Disposition::Survived {
        route: Route::Discharged {
            discharged: vec![
                discharge("pkg/lib/pkg", NEVER_INFECTED),
                discharge("pkg/test/it", BRANCH_NEVER_TAKEN),
            ],
        },
    });

    let findings = phase.findings(&BTreeSet::new());
    let detail = &findings.first().expect("one finding").detail;

    assert!(
        detail.contains("no test could have noticed"),
        "every target that reaches this was removed by a proof, so nothing looked and \
         shrugged: a reader told that no test noticed it would go looking for the test \
         that should have: {detail}"
    );
    assert!(
        detail.contains(BRANCH_NEVER_TAKEN) && detail.contains(NEVER_INFECTED),
        "and the proofs are named, because a reader who cannot tell a discharge from an \
         oversight can act on neither: {detail}"
    );
    assert!(
        detail.contains("2 targets"),
        "and how many were removed, which is what says how much of the suite this rests \
         on: {detail}"
    );
}

#[test]
fn a_survivor_tests_did_run_says_how_many_looked() {
    let one = phase(Disposition::Survived {
        route: Route::Block {
            reaching: vec![Reaches {
                target: "pkg/lib/pkg".to_owned(),
                tests: mjutest_cli::assure::route::Asked::Every,
            }],
            discharged: Vec::new(),
            fallback: None,
        },
    });

    let findings = one.findings(&BTreeSet::new());
    let detail = &findings.first().expect("one finding").detail;

    assert!(
        detail.contains("no test noticed") && detail.contains("1 target ran it"),
        "a target ran the mutation and passed anyway, which is a gap in what that target \
         asserts and not a proof about the mutation: {detail}"
    );
}
