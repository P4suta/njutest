// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a whole run adds up to: the tally, the score, what stops a clean run, and the exit code that follows.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::time::Duration;

use rust_mutants::outcome::Outcome;
use rust_mutants::run::{Finding, FindingKind, Judged, Run, Standing, Verified};

fn judged(index: u32, outcome: Outcome) -> Judged {
    Judged {
        index,
        id: format!("{index:064x}"),
        display_id: format!("{index:020x}"),
        outcome,
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration: Duration::from_millis(1),
        tests_run: Some(1),
        failed_tests: Vec::new(),
        signal: None,
        not_run_reason: None,
        route: None,
        retried: false,
        expected: false,
        measured: true,
        identical: None,
        source_run_id: None,
    }
}

const fn of(judged: Vec<Judged>) -> Run {
    Run {
        judged,
        expectations: Vec::new(),
        skipped: 0,
        refused: 0,
        interrupted: false,
        shard: None,
        duration: Duration::from_secs(1),
    }
}

#[test]
fn the_tally_is_the_outcomes_folded_and_nothing_else() {
    let run = of(vec![
        judged(0, Outcome::Killed),
        judged(1, Outcome::Killed),
        judged(2, Outcome::Survived),
        judged(3, Outcome::TimedOut),
        judged(4, Outcome::Inconclusive),
        judged(5, Outcome::Errored),
        judged(6, Outcome::NotRun),
    ]);
    let tally = run.tally();
    assert_eq!(tally.cataloged, 7);
    assert_eq!(tally.killed, 2);
    assert_eq!(tally.survived, 1);
    assert_eq!(tally.timed_out, 1);
    assert_eq!(tally.inconclusive, 1);
    assert_eq!(tally.errored, 1);
    assert_eq!(tally.not_run, 1);
    assert_eq!(
        tally.executed, 6,
        "everything but what never ran was executed"
    );
    assert_eq!(
        tally.killed + tally.survived + tally.timed_out + tally.inconclusive + tally.errored,
        tally.executed,
        "the executed mutants are exactly the ones with an outcome"
    );
}

#[test]
fn the_score_is_what_was_detected_over_what_was_decided() {
    let run = of(vec![
        judged(0, Outcome::Killed),
        judged(1, Outcome::TimedOut),
        judged(2, Outcome::Survived),
        judged(3, Outcome::Inconclusive),
    ]);
    let score = run.score().expect("three decided mutants");
    assert_eq!(score.detected, 2);
    assert_eq!(
        score.decided, 3,
        "an inconclusive mutant is not evidence either way"
    );
    assert!((score.value - 2.0 / 3.0).abs() < 1e-12, "{score:?}");

    assert!(
        of(vec![judged(0, Outcome::Inconclusive)]).score().is_none(),
        "a run that decided nothing has no score to report"
    );
    assert!(of(Vec::new()).score().is_none());
}

#[test]
fn every_mutant_the_tests_did_not_notice_becomes_a_finding_that_names_it() {
    let run = of(vec![
        judged(0, Outcome::Killed),
        judged(1, Outcome::Survived),
        judged(2, Outcome::Inconclusive),
        judged(3, Outcome::Errored),
    ]);
    let findings = run.findings();
    let kinds: Vec<FindingKind> = findings.iter().map(|finding| finding.kind).collect();
    assert_eq!(
        kinds,
        [
            FindingKind::SurvivingMutant,
            FindingKind::InconclusiveMutant,
            FindingKind::ErroredMutant
        ]
    );
    assert!(findings.iter().all(|finding| !finding.detail.is_empty()));
    assert_eq!(
        findings[0].mutant.as_deref(),
        Some(judged(1, Outcome::Survived).id.as_str())
    );
    assert!(of(vec![judged(0, Outcome::Killed)]).findings().is_empty());
}

#[test]
fn a_mutant_a_reviewer_expected_to_survive_is_not_a_finding_and_a_stale_claim_is() {
    let mut met = judged(1, Outcome::Survived);
    met.expected = true;
    let expectations = vec![
        Verified {
            id: "aaaa".to_owned(),
            reason: "equivalent under the invariant".to_owned(),
            outcome: Outcome::Survived,
            mutant: Some(met.id.clone()),
            standing: Standing::Met,
        },
        Verified {
            id: "bbbb".to_owned(),
            reason: "was equivalent last week".to_owned(),
            outcome: Outcome::Survived,
            mutant: Some(judged(2, Outcome::Killed).id),
            standing: Standing::Stale {
                actual: Outcome::Killed,
            },
        },
        Verified {
            id: "cccc".to_owned(),
            reason: "for a mutant that is gone".to_owned(),
            outcome: Outcome::Survived,
            mutant: None,
            standing: Standing::Unmatched {
                why: "no mutant answers to \"cccc\"".to_owned(),
            },
        },
    ];
    let run = Run {
        judged: vec![met, judged(2, Outcome::Killed)],
        expectations,
        skipped: 0,
        refused: 0,
        interrupted: false,
        shard: None,
        duration: Duration::from_secs(1),
    };
    let kinds: Vec<FindingKind> = run.findings().iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        [
            FindingKind::StaleExpectation,
            FindingKind::UnmatchedExpectation
        ],
        "an expected survivor is accounted for, not reported as a hole"
    );
    assert_eq!(run.tally().expected, 1);
    assert_eq!(run.exit_code(), 1);
}

#[test]
fn the_exit_code_says_what_the_run_established_and_nothing_more() {
    assert_eq!(
        of(vec![
            judged(0, Outcome::Killed),
            judged(1, Outcome::TimedOut)
        ])
        .exit_code(),
        0,
        "every mutant was noticed"
    );
    assert_eq!(of(Vec::new()).exit_code(), 0, "nothing to notice");
    assert_eq!(
        of(vec![judged(0, Outcome::Survived)]).exit_code(),
        1,
        "a survivor is a gap in the tests, not a failure of the run"
    );
    assert_eq!(
        of(vec![judged(0, Outcome::Inconclusive)]).exit_code(),
        1,
        "a run that could not decide has not established detection"
    );
    assert_eq!(
        of(vec![judged(0, Outcome::Errored)]).exit_code(),
        2,
        "the harness itself failed, which is about the run and not the tests"
    );
    assert_eq!(
        of(vec![judged(0, Outcome::NotRun)]).exit_code(),
        2,
        "a mutant nothing ran and nothing cancelled is an invariant broken"
    );
    let mut interrupted = of(vec![judged(0, Outcome::NotRun)]);
    interrupted.interrupted = true;
    assert_eq!(interrupted.exit_code(), 130);
    assert!(
        interrupted
            .findings()
            .iter()
            .all(|finding| finding.kind != FindingKind::NotRunMutant),
        "what the interruption stopped is the interruption, not a hole"
    );
}

#[test]
fn every_finding_kind_has_a_wire_name_that_reads_back() {
    for kind in FindingKind::ALL {
        assert!(!kind.name().is_empty());
        assert_eq!(FindingKind::parse(kind.name()), Some(kind));
    }
    assert_eq!(FindingKind::parse("something-else"), None);
    let finding = Finding {
        kind: FindingKind::SurvivingMutant,
        mutant: None,
        detail: "d".to_owned(),
    };
    assert_eq!(finding.kind.name(), "surviving-mutant");
}

#[test]
fn an_inconclusive_mutant_says_which_of_the_two_things_left_it_undecided() {
    let silent = {
        let mut one = judged(0, Outcome::Inconclusive);
        one.tests_run = Some(0);
        one
    };
    let timed_out = {
        let mut one = judged(1, Outcome::Inconclusive);
        one.retried = true;
        one.tests_run = None;
        one
    };
    let run = of(vec![silent, timed_out]);
    let findings = run.findings();
    let detail = |index: u32| {
        findings
            .iter()
            .find(|finding| finding.mutant.as_deref() == Some(&format!("{index:064x}")))
            .map_or_else(
                || panic!("a finding for {index}"),
                |finding| finding.detail.clone(),
            )
    };
    assert!(
        detail(0).contains("no test ran"),
        "a target that ran nothing is not a timeout: {}",
        detail(0)
    );
    assert!(
        detail(1).contains("timed out"),
        "a timeout that did not repeat says so: {}",
        detail(1)
    );
}

#[test]
fn jobs_defaults_to_the_machine_capped_at_four_and_a_number_wins() {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    assert_eq!(rust_mutants::run::jobs(0), cores.min(4));
    assert_eq!(rust_mutants::run::jobs(1), 1);
    assert_eq!(
        rust_mutants::run::jobs(64),
        64,
        "each test binary already runs its own tests on as many threads as the machine has, \
         so the cap is what keeps a duration a fact about the mutation rather than the load; \
         a person who says otherwise has said so"
    );
}
