// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a whole run adds up to: the tally, the score, what stops a clean run, and the exit code that follows.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::time::Duration;

use njutest_devkit::result::{ResultState::Returned, result_state};
use rust_mutants::outcome::Outcome;
use rust_mutants::run::{CodegenIdentity, Finding, FindingKind, Judged, Run, Standing, Verified};

fn judged(index: u32, outcome: Outcome) -> Judged {
    Judged {
        index,
        id: format!("{index:064x}"),
        display_id: format!("{index:020x}"),
        outcome,
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        start_failure: None,
        protocol_failure: None,
        duration: Duration::from_millis(1),
        tests_run: Some(1),
        failed_tests: Vec::new(),
        signal: None,
        not_run_reason: None,
        route: None,
        retried: false,
        lingered: false,
        expected: false,
        measured: true,
        identical: CodegenIdentity::NotMeasured,
        source_run_id: None,
        step_notice: None,
    }
}

const fn of(judged: Vec<Judged>) -> Run {
    Run {
        judged,
        expectations: Vec::new(),
        skipped: 0,
        refused: 0,
        claims: Vec::new(),
        interrupted: false,
        shard: None,
        duration: Duration::from_secs(1),
        width: rust_mutants::run::Width {
            asked: rust_mutants::run::Jobs::Auto,
            used: 1,
        },
    }
}

#[test]
fn the_tally_is_the_outcomes_folded_and_nothing_else() {
    let run = of(vec![
        judged(0, Outcome::Killed),
        judged(1, Outcome::Killed),
        judged(2, Outcome::Survived),
        judged(3, Outcome::StepLimitReached),
        judged(4, Outcome::Inconclusive),
        judged(5, Outcome::Errored),
        judged(6, Outcome::NotRun),
        judged(7, Outcome::Waited),
    ]);
    let tally = run.tally();
    assert_eq!(
        result_state(&tally),
        Returned,
        "the small tally is representable"
    );
    let Ok(tally) = tally else { return };
    assert_eq!(tally.cataloged, 8);
    assert_eq!(tally.killed, 2);
    assert_eq!(tally.survived, 1);
    assert_eq!(tally.step_limit_reached, 1);
    assert_eq!(tally.waited, 1);
    assert_eq!(tally.inconclusive, 1);
    assert_eq!(tally.errored, 1);
    assert_eq!(tally.not_run, 1);
    assert_eq!(
        tally.executed, 7,
        "everything but what never ran was executed"
    );
    assert_eq!(
        tally.killed
            + tally.survived
            + tally.step_limit_reached
            + tally.waited
            + tally.inconclusive
            + tally.errored,
        tally.executed,
        "the executed mutants are exactly the ones with an outcome"
    );
}

#[test]
fn the_score_is_what_was_detected_over_what_was_decided() {
    let run = of(vec![
        judged(0, Outcome::Killed),
        judged(1, Outcome::StepLimitReached),
        judged(2, Outcome::Survived),
        judged(3, Outcome::Inconclusive),
        judged(4, Outcome::Waited),
    ]);
    let score = run.score();
    assert_eq!(
        result_state(&score),
        Returned,
        "the small tally is representable"
    );
    let Ok(score) = score else { return };
    assert!(score.is_some(), "two decided mutants");
    let Some(score) = score else { return };
    assert_eq!(score.detected, 1);
    assert_eq!(
        score.decided, 2,
        "an inconclusive mutant is not evidence either way, and neither are a finite step \
         limit or a clock bound this machine reached"
    );
    assert!((score.value - 0.5).abs() < 1e-12, "{score:?}");

    let inconclusive = of(vec![judged(0, Outcome::Inconclusive)]).score();
    assert_eq!(
        result_state(&inconclusive),
        Returned,
        "the small tally is representable"
    );
    let Ok(inconclusive) = inconclusive else {
        return;
    };
    assert!(
        inconclusive.is_none(),
        "a run that decided nothing has no score to report"
    );
    let empty = of(Vec::new()).score();
    assert_eq!(
        result_state(&empty),
        Returned,
        "the empty tally is representable"
    );
    let Ok(empty) = empty else { return };
    assert!(empty.is_none());
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
            locator: None,
            reason: "equivalent under the invariant".to_owned(),
            outcome: Outcome::Survived,
            mutant: Some(met.id.clone()),
            covered: 1,
            standing: Standing::Met,
        },
        Verified {
            id: "bbbb".to_owned(),
            locator: None,
            reason: "was equivalent last week".to_owned(),
            outcome: Outcome::Survived,
            mutant: Some(judged(2, Outcome::Killed).id),
            covered: 1,
            standing: Standing::Stale {
                actual: Outcome::Killed,
            },
        },
        Verified {
            id: "cccc".to_owned(),
            locator: None,
            reason: "for a mutant that is gone".to_owned(),
            outcome: Outcome::Survived,
            mutant: None,
            covered: 0,
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
        claims: Vec::new(),
        interrupted: false,
        shard: None,
        duration: Duration::from_secs(1),
        width: rust_mutants::run::Width {
            asked: rust_mutants::run::Jobs::Auto,
            used: 1,
        },
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
    let tally = run.tally();
    assert_eq!(
        result_state(&tally),
        Returned,
        "the small tally is representable"
    );
    let Ok(tally) = tally else { return };
    assert_eq!(tally.expected, 1);
    assert_eq!(run.exit_code(), 1);
}

#[test]
fn the_exit_code_says_what_the_run_established_and_nothing_more() {
    assert_eq!(
        of(vec![
            judged(0, Outcome::Killed),
            judged(1, Outcome::StepLimitReached)
        ])
        .exit_code(),
        2,
        "the finite step limit is an infrastructure finding, not a detection"
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
            .map(|finding| finding.detail.clone())
    };
    let detail0 = detail(0);
    assert!(detail0.is_some(), "a finding for 0");
    let Some(detail0) = detail0 else { return };
    assert!(
        detail0.contains("no test ran"),
        "a target that ran nothing is not a timeout: {detail0}"
    );
    let detail1 = detail(1);
    assert!(detail1.is_some(), "a finding for 1");
    let Some(detail1) = detail1 else { return };
    assert!(
        detail1.contains("timed out"),
        "a timeout that did not repeat says so: {detail1}"
    );
}

#[test]
fn auto_is_the_machine_capped_at_four_all_is_the_machine_and_a_count_wins() {
    use rust_mutants::run::{Jobs, JobsError};

    for cores in [1, 2, 4, 8, 64] {
        assert_eq!(
            Jobs::Auto.resolve_on(cores),
            cores.min(4),
            "{cores} cores: each test binary already runs its own tests on as many threads as the \
             machine has, so the cap is what keeps a duration a fact about the mutation rather \
             than the load on a machine a person is using"
        );
        assert_eq!(
            Jobs::All.resolve_on(cores),
            cores,
            "{cores} cores: a CI runner doing nothing else is asked for every one"
        );
        assert_eq!(Jobs::count(64).map(|jobs| jobs.resolve_on(cores)), Ok(64));
    }
    assert_eq!(
        Jobs::All.resolve_on(0),
        1,
        "a machine that says nothing still runs one"
    );
    for (written, read) in [
        ("auto", Ok(Jobs::Auto)),
        ("all", Ok(Jobs::All)),
        ("3", Jobs::count(3)),
        ("0", Err(JobsError::Zero)),
        (
            "many",
            Err(JobsError::Unknown {
                text: "many".to_owned(),
            }),
        ),
    ] {
        assert_eq!(Jobs::parse(written), read, "{written:?}");
    }
    assert_eq!(
        Jobs::parse("0").map_err(|error| error.to_string()),
        Err(
            "0 jobs would measure nothing; write `auto` for as many as the machine has, capped \
             at 4, or `all` for every one"
                .to_owned()
        ),
        "a zero that used to mean auto says which word means it now"
    );
}

#[test]
fn a_shard_nobody_could_have_meant_is_refused_by_the_text_it_was_given() {
    use rust_mutants::run::{Shard, ShardError};

    assert_eq!(
        Shard::parse("2/3").map(|shard| shard.to_string()),
        Ok("2/3".to_owned()),
        "the shape a person writes"
    );
    for text in ["2", "", "two/3", "2/three", "/3", "2/", "2/3/4"] {
        assert!(
            matches!(Shard::parse(text), Err(ShardError::Malformed { text: said }) if said == text),
            "a part of a run named by something that is not `K/N` is refused, and the refusal \
             says what it was given rather than a shape nobody typed: {text:?} came to {:?}",
            Shard::parse(text)
        );
    }
    for (text, index, of) in [("0/3", 0, 3), ("4/3", 4, 3), ("1/0", 1, 0)] {
        assert!(
            matches!(
                Shard::parse(text),
                Err(ShardError::OutOfRange { index: said, of: many }) if said == index && many == of
            ),
            "a part outside the run it is a part of is a different refusal, and it names both \
             numbers: {text:?} came to {:?}",
            Shard::parse(text)
        );
    }
}

#[test]
fn a_proof_removing_a_mutation_and_nothing_reaching_it_are_counted_and_named_apart() {
    use rust_mutants::run::NotRunReason;

    let unrun = |index: u32, why: NotRunReason| {
        let mut one = judged(index, Outcome::NotRun);
        one.not_run_reason = Some(why);
        one
    };
    let run = of(vec![
        unrun(1, NotRunReason::Unreached),
        unrun(2, NotRunReason::Discharged),
        judged(3, Outcome::Killed),
    ]);

    let tally = run.tally();
    assert_eq!(
        result_state(&tally),
        Returned,
        "the small tally is representable"
    );
    let Ok(tally) = tally else { return };
    assert_eq!(
        (tally.unreached, tally.discharged, tally.not_run),
        (1, 1, 2),
        "each reason is counted in its own column, or a reader is told a proof removed what \
         nothing reached: {tally:?}"
    );
    let found = run.findings();
    let kinds: Vec<FindingKind> = found.iter().map(|one| one.kind).collect();
    assert_eq!(
        kinds,
        [FindingKind::UnreachedMutant, FindingKind::DischargedMutant],
        "and the finding says the same, in the order the rows are in: a reader told the wrong \
         one checks the proof when they should write a test, or the other way about"
    );
    assert!(
        found[0].detail.contains("no measured test reaches")
            && found[0].detail.contains("never execute"),
        "and the sentence the reader acts on says that nothing ran the code: {:?}",
        found[0].detail
    );
    assert!(
        found[1].detail.contains("removed by a proof") && found[1].detail.contains("never observe"),
        "and that a proof removed every target that could have noticed: {:?}",
        found[1].detail
    );
    for (at, one) in found.iter().enumerate() {
        assert!(
            one.detail
                .contains(&format!("{:020x}", at.saturating_add(1))),
            "and each names the mutation it is about, because a reader with a list of findings \
             and no names has nothing to look up: {:?}",
            one.detail
        );
    }
}

#[test]
fn a_claim_is_written_back_by_the_way_it_named_its_mutant() {
    use rust_mutants::run::Expectation;
    use rust_mutants::session::Locator;

    let by_id = Expectation {
        id: Some("b8e3f78d".to_owned()),
        locator: None,
        reason: "the bound is equivalent".to_owned(),
        outcome: Outcome::Survived,
    };
    assert_eq!(
        by_id.name(),
        "b8e3f78d",
        "a claim by identity is written back as the identity a person typed"
    );

    let by_locator = Expectation {
        id: None,
        locator: Some(Locator {
            path: "src/lib.rs".to_owned(),
            item: "clamp".to_owned(),
            rule: "le-to-lt".to_owned(),
            original: "<=".to_owned(),
            line: None,
            count: None,
        }),
        ..by_id.clone()
    };
    assert_eq!(
        by_locator.name(),
        "src/lib.rs clamp le-to-lt \"<=\"",
        "and a claim by locator by all four of the things that name a mutation, because a \
         reader matching a finding to a line of the ledger has nothing else to match on"
    );

    let named_nothing = Expectation {
        id: None,
        locator: None,
        ..by_id
    };
    assert_eq!(
        named_nothing.name(),
        "",
        "and a claim that named nothing says nothing rather than something a reader would look for"
    );
}

#[test]
fn the_outcomes_a_clean_run_says_nothing_about_are_each_left_out_for_their_own_reason() {
    use rust_mutants::run::NotRunReason;

    let unrun = |index: u32, why: NotRunReason| {
        let mut one = judged(index, Outcome::NotRun);
        one.not_run_reason = Some(why);
        one
    };
    let mut accounted = judged(1, Outcome::Survived);
    accounted.expected = true;
    let rows = vec![
        accounted,
        judged(2, Outcome::Killed),
        judged(3, Outcome::StepLimitReached),
        unrun(4, NotRunReason::Unselected),
        unrun(5, NotRunReason::StoppedEarly),
        judged(6, Outcome::Survived),
    ];
    let run = of(rows);

    let kinds: Vec<FindingKind> = run.findings().iter().map(|one| one.kind).collect();
    assert_eq!(
        kinds,
        [
            FindingKind::StepLimitReachedMutant,
            FindingKind::SurvivingMutant
        ],
        "a finite step ceiling is an unanswered infrastructure finding, while the survivor \
         nobody accounted for is a test finding"
    );
    let tally = run.tally();
    assert_eq!(
        result_state(&tally),
        Returned,
        "the small tally is representable"
    );
    let Ok(tally) = tally else { return };
    assert_eq!(
        (
            tally.expected,
            tally.killed,
            tally.step_limit_reached,
            tally.survived
        ),
        (1, 1, 1, 2),
        "and the columns count them where a reader looks: {tally:?}"
    );
}

#[test]
fn a_mutation_this_machine_stopped_waiting_for_is_a_finding_that_says_so() {
    let run = of(vec![judged(0, Outcome::Waited)]);
    let findings = run.findings();
    assert_eq!(
        findings.iter().map(|one| one.kind).collect::<Vec<_>>(),
        [FindingKind::WaitedMutant],
        "a bound expiring establishes nothing about the mutation, so the run raises it as \
         a hole rather than passing over it: a reader who is shown nothing concludes the \
         mutation was answered"
    );
    let detail = &findings[0].detail;
    assert!(
        detail.contains("stopped waiting") && detail.contains("step allowance"),
        "and it names both ways out, because a person told only that a bound expired \
         reaches for the bound, which is the knob that made the answer depend on their \
         machine in the first place: {detail:?}"
    );
    assert!(
        FindingKind::WaitedMutant.is_infrastructure(),
        "it is a gap in what the run established rather than a fault in the code, and the \
         two are counted in different columns"
    );
}

#[test]
fn a_run_ends_on_the_gravest_thing_it_holds_and_an_interruption_outranks_all_of_it() {
    use rust_mutants::run::Exit;
    assert_eq!(Exit::of(false, []), Exit::Detected);
    assert_eq!(Exit::of(false, [FindingKind::SurvivingMutant]), Exit::Found);
    assert_eq!(
        Exit::of(
            false,
            [FindingKind::SurvivingMutant, FindingKind::WaitedMutant]
        ),
        Exit::Unestablished,
        "a run that could not measure something it ran says so before what it found"
    );
    assert_eq!(
        Exit::of(true, [FindingKind::WaitedMutant]),
        Exit::Interrupted
    );
    let codes: Vec<u8> = Exit::ALL.iter().map(|exit| exit.code()).collect();
    assert_eq!(codes, vec![0, 1, 2, 130, 143]);
}

#[test]
fn every_exit_a_caller_reads_back_is_one_of_the_table_and_no_other_code_is() {
    use rust_mutants::run::Exit;
    for exit in Exit::ALL {
        assert_eq!(
            Exit::read(i32::from(exit.code())),
            Some(exit),
            "a caller holding the code a run ended with reads back the exit it meant"
        );
    }
    for code in [-1, 3, 101, 129, 255] {
        assert_eq!(
            Exit::read(code),
            None,
            "and a code the table does not hold is no verdict at all, not the nearest one: {code}"
        );
    }
}
#[test]
fn an_errored_mutant_whose_process_never_started_says_why_rather_than_an_exit_nobody_produced() {
    let mut unstarted = judged(0, Outcome::Errored);
    unstarted.exit_code = rust_mutants::runner::EXIT_CODE_UNAVAILABLE;
    unstarted.start_failure = Some(rust_mutants::execute::StartFailure::Missing);
    let said: Vec<String> = of(vec![unstarted])
        .findings()
        .into_iter()
        .filter(|finding| finding.kind == FindingKind::ErroredMutant)
        .map(|finding| finding.detail)
        .collect();
    assert!(
        said.iter()
            .any(|detail| detail.contains("its test binary was not there")
                && !detail.contains("exit -1")),
        "a row about a process that never started names why, which `exit -1` never did: {said:?}"
    );
}

#[test]
fn an_errored_mutant_says_which_check_of_the_step_protocol_stopped_it_or_that_nothing_said() {
    use rust_mutants::execute::StepProtocolFailure;

    let detail = |failure: StepProtocolFailure| {
        let mut errored = judged(0, Outcome::Errored);
        errored.exit_code = 94;
        errored.protocol_failure = Some(failure);
        of(vec![errored])
            .findings()
            .into_iter()
            .find(|finding| finding.kind == FindingKind::ErroredMutant)
            .map(|finding| finding.detail)
    };
    let stated = detail(StepProtocolFailure::Stated {
        check: "lock".to_owned(),
        os: 33,
    });
    assert!(
        stated
            .as_deref()
            .is_some_and(|said| said.contains("`lock`") && said.contains("33")),
        "a reader is told the check that stopped the process and what the system answered, not \
         only a status: {stated:?}"
    );
    let silent = detail(StepProtocolFailure::Publication {});
    assert!(
        silent
            .as_deref()
            .is_some_and(|said| said.contains("stale build")),
        "every runtime this release generates says why it stops, so silence names a runtime from \
         another build: {silent:?}"
    );
}
