// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each disposition makes a reader act on, and what an acceptance can answer for.

#![expect(
    clippy::indexing_slicing,
    reason = "a phase built from one disposition raises one finding, and no finding where this reads one is the failure it is here to report"
)]

use std::collections::{BTreeMap, BTreeSet};

use mjutest_cli::assure::mutation::{Disposition, Judged, Mutation, Unconfirmed, tail};
use mjutest_cli::assure::route::{BRANCH_NEVER_TAKEN, Discharge, NEVER_INFECTED, Reaches, Route};
use mjutest_cli::report::{FindingKind, Position};

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

#[test]
fn what_a_capture_is_quoted_by_is_its_last_word_and_never_nothing() {
    let harness = b"running 3 tests\ntest one ... ok\ntest two ... FAILED\n\
                    failures:\n    two\ntest result: FAILED. 1 passed; 1 failed\n";

    assert_eq!(
        tail(harness),
        "test result: FAILED. 1 passed; 1 failed",
        "the last line is where a harness says what it found, and the lines above it are \
         how it got there: quoting the first would put `running 3 tests` in front of \
         somebody asking what went wrong"
    );
    assert_eq!(
        tail(b"the answer\n\n   \n"),
        "the answer",
        "and blank lines after it are not what it said"
    );
    assert_eq!(
        tail(b""),
        "(it said nothing)",
        "a capture with nothing in it is quoted as having said nothing: an empty \
         quotation reads as a line somebody forgot to copy"
    );
    assert_eq!(
        tail(&b"x".repeat(500)),
        "x".repeat(200),
        "and one enormous line does not become the whole of a report somebody has to read"
    );
}

#[test]
fn each_way_a_pair_can_fail_to_agree_says_which_one_happened() {
    let control = phase(Disposition::Unconfirmed {
        on: "pkg/test/lib adds_two_numbers".to_owned(),
        why: Unconfirmed::ControlFailed {
            detail: "assertion failed: left == right".to_owned(),
        },
    });
    let again = phase(Disposition::Unconfirmed {
        on: "pkg/test/lib adds_two_numbers".to_owned(),
        why: Unconfirmed::DidNotReproduce,
    });

    let control = control.findings(&BTreeSet::new())[0].detail.clone();
    let again = again.findings(&BTreeSet::new())[0].detail.clone();

    assert!(
        control.contains("the same test failed on the original code")
            && control.contains("assertion failed: left == right"),
        "the test was already failing, so what it said is the whole of what a reader can \
         act on: the mutation is not the subject: {control}"
    );
    assert!(
        again.contains("did not happen the second time"),
        "and a kill that did not reproduce is the opposite finding — the test does \
         notice it, sometimes — so the two sentences send a reader to two different \
         places: {again}"
    );
    assert_ne!(
        control, again,
        "which is why they are not one sentence with a name attached"
    );
}

#[test]
fn a_finding_is_raised_where_the_mutation_it_names_is() {
    let mut one = judged(Disposition::Unreached);
    one.position = Some(Position {
        line: 12,
        column: 5,
        character_column: 5,
    });
    let phase = Mutation {
        judged: vec![one],
        skips: BTreeMap::new(),
    };

    let findings = phase.findings(&BTreeSet::new());

    assert_eq!(
        findings[0].position.map(|at| at.line),
        Some(12),
        "an editor shows a finding where its mutation is, and a finding that carries no \
         position is one a person has to go looking for with a display id and a grep"
    );
}

#[test]
fn a_survivor_removed_by_one_proof_twice_over_names_that_proof_once() {
    let phase = phase(Disposition::Survived {
        route: Route::Discharged {
            discharged: vec![
                discharge("pkg/test/two", NEVER_INFECTED),
                discharge("pkg/lib/pkg", NEVER_INFECTED),
                discharge("pkg/test/it", BRANCH_NEVER_TAKEN),
            ],
        },
    });

    let detail = phase.findings(&BTreeSet::new())[0].detail.clone();

    assert!(
        detail.ends_with(&format!("by {BRANCH_NEVER_TAKEN} and {NEVER_INFECTED}")),
        "three targets were removed by two proofs, and the sentence names each proof \
         once and in one order. `contains` would pass on a sentence that went on to \
         name never-infected twice more, so the whole of what it says has to be the \
         whole of what is checked: {detail}"
    );
}

#[test]
fn a_survivor_some_tests_ran_and_others_were_removed_from_says_the_tests_ran() {
    let phase = phase(Disposition::Survived {
        route: Route::Block {
            reaching: vec![Reaches {
                target: "pkg/lib/pkg".to_owned(),
                tests: mjutest_cli::assure::route::Asked::Every,
            }],
            discharged: vec![discharge("pkg/test/it", NEVER_INFECTED)],
            fallback: None,
        },
    });

    let detail = phase.findings(&BTreeSet::new())[0].detail.clone();

    assert!(
        detail.contains("no test noticed") && !detail.contains("could have"),
        "a proof removed one target and another ran the mutation anyway and passed. What \
         a reader has to do is strengthen that target, and telling them no test could \
         have noticed would send them to audit the proof instead: {detail}"
    );
}

fn of(display_id: &str, disposition: Disposition, reused: bool) -> Judged {
    Judged {
        id: display_id.repeat(4),
        display_id: display_id.to_owned(),
        path: "src/lib.rs".to_owned(),
        rule: "add-to-sub@1".to_owned(),
        position: None,
        disposition,
        source_run_id: reused.then(|| "20260905T081500Z-000000".to_owned()),
    }
}

/// One of every disposition, some read back from an earlier run.
fn all_of_them() -> Mutation {
    let route = || Route::Discharged {
        discharged: vec![discharge("pkg/lib/pkg", NEVER_INFECTED)],
    };
    Mutation {
        judged: vec![
            of(
                "aaaa",
                Disposition::Rejected {
                    diagnostic: "no".to_owned(),
                },
                false,
            ),
            of(
                "bbbb",
                Disposition::Killed {
                    by: "one".to_owned(),
                },
                false,
            ),
            of(
                "cccc",
                Disposition::Killed {
                    by: "one".to_owned(),
                },
                true,
            ),
            of(
                "dddd",
                Disposition::TimedOut {
                    on: "one".to_owned(),
                },
                false,
            ),
            of("eeee", Disposition::Survived { route: route() }, false),
            of("ffff", Disposition::Survived { route: route() }, true),
            of("gggg", Disposition::Survived { route: route() }, false),
            of("hhhh", Disposition::Unreached, false),
            of("iiii", Disposition::Unreached, false),
            of("jjjj", Disposition::Equivalent { route: route() }, false),
            of(
                "kkkk",
                Disposition::Unconfirmed {
                    on: "one".to_owned(),
                    why: Unconfirmed::DidNotReproduce,
                },
                false,
            ),
            of(
                "llll",
                Disposition::Errored {
                    on: "one".to_owned(),
                    detail: "no binary".to_owned(),
                },
                false,
            ),
        ],
        skips: BTreeMap::new(),
    }
}

#[test]
fn every_disposition_is_counted_once_in_the_columns_it_belongs_to() {
    let accepted = BTreeSet::from(["gggg".repeat(4), "iiii".repeat(4), "jjjj".repeat(4)]);

    let counts = all_of_them().accounting(&accepted);

    assert_eq!(counts.cataloged, 12, "one row for every mutation judged");
    assert_eq!(counts.rejected, 1);
    assert_eq!(
        counts.executed, 8,
        "a mutation is executed when something ran it: the kills, the timeout, the \
         survivals, the pair that did not agree and the harness that failed. What is not \
         executed is what was refused, what nothing reached, and what the compiler \
         rendered identically"
    );
    assert_eq!(counts.killed, 2);
    assert_eq!(counts.timed_out, 1);
    assert_eq!(counts.survived, 3);
    assert_eq!(counts.unreached, 2);
    assert_eq!(counts.equivalent, 1);
    assert_eq!(
        counts.reused_killed, 1,
        "and a run says how much of what it reports it established itself"
    );
    assert_eq!(counts.reused_survived, 1);
    assert_eq!(
        counts.accepted, 3,
        "an acceptance answers for a survivor, for a mutation nothing reached, and for \
         one proved equivalent, and for nothing else: counting a timeout or an error \
         among them would let a reviewer sign off on an outcome nobody established"
    );
    assert_eq!(
        counts.cataloged,
        counts.rejected + counts.executed + counts.unreached + counts.equivalent,
        "and every mutation is in exactly one of the four, which is the identity a \
         reader adds up to check the rest"
    );
}

#[test]
fn which_test_decided_a_mutation_is_named_by_the_dispositions_that_had_one() {
    let route = Route::Discharged {
        discharged: vec![discharge("pkg/lib/pkg", NEVER_INFECTED)],
    };

    for (what, disposition) in [
        (
            "a kill",
            Disposition::Killed {
                by: "pkg/test/it one".to_owned(),
            },
        ),
        (
            "a timeout",
            Disposition::TimedOut {
                on: "pkg/test/it one".to_owned(),
            },
        ),
        (
            "a pair that did not agree",
            Disposition::Unconfirmed {
                on: "pkg/test/it one".to_owned(),
                why: Unconfirmed::DidNotReproduce,
            },
        ),
        (
            "a harness that could not run",
            Disposition::Errored {
                on: "pkg/test/it one".to_owned(),
                detail: "no binary".to_owned(),
            },
        ),
    ] {
        assert_eq!(
            disposition.decided_by(),
            Some("pkg/test/it one"),
            "{what} happened against one target, and which one is the thing a reader \
             cannot work out from anything else in the row"
        );
    }

    for (what, disposition) in [
        (
            "a survivor",
            Disposition::Survived {
                route: route.clone(),
            },
        ),
        ("one nothing reached", Disposition::Unreached),
        ("one proved equivalent", Disposition::Equivalent { route }),
        (
            "one the compiler refused",
            Disposition::Rejected {
                diagnostic: "no".to_owned(),
            },
        ),
    ] {
        assert_eq!(
            disposition.decided_by(),
            None,
            "{what} was decided by no test, and naming one would put a target in a row \
             as having said something it never said"
        );
    }
}

/// Three mutations of one disposition, so a column counted over them is that column alone.
fn three(of_a_kind: [(&str, Disposition, bool); 3]) -> Mutation {
    Mutation {
        judged: of_a_kind
            .into_iter()
            .map(|(name, disposition, reused)| of(name, disposition, reused))
            .collect(),
        skips: BTreeMap::new(),
    }
}

#[test]
fn each_column_counts_its_own_kind_and_not_whatever_makes_the_total_come_out() {
    let route = || Route::Discharged {
        discharged: vec![discharge("pkg/lib/pkg", NEVER_INFECTED)],
    };
    let killed = || Disposition::Killed {
        by: "one".to_owned(),
    };
    let none = BTreeSet::new();
    let first = |name: &str| BTreeSet::from([name.repeat(4)]);

    assert_eq!(
        three([
            ("aaaa", killed(), true),
            ("bbbb", killed(), false),
            ("cccc", killed(), false),
        ])
        .accounting(&none)
        .reused_killed,
        1,
        "a run says how many of its kills it read back rather than establishing, so the \
         ones it counts are the ones naming the run they came from: counting the others \
         gives the same total on a run where the two happen to be even, and a different \
         answer on every run where they are not"
    );
    assert_eq!(
        three([
            ("mmmm", Disposition::Survived { route: route() }, true),
            ("nnnn", Disposition::Survived { route: route() }, false),
            ("oooo", Disposition::Survived { route: route() }, false),
        ])
        .accounting(&none)
        .reused_survived,
        1,
        "and how many survivals"
    );
    assert_eq!(
        three([
            ("dddd", Disposition::Survived { route: route() }, false),
            ("eeee", Disposition::Survived { route: route() }, false),
            ("ffff", Disposition::Survived { route: route() }, false),
        ])
        .accounting(&first("dddd"))
        .accepted,
        1,
        "and how many of its survivors a reviewer answered for, which is the ones the \
         ledger names rather than the ones it does not"
    );
    assert_eq!(
        three([
            ("gggg", Disposition::Unreached, false),
            ("hhhh", Disposition::Unreached, false),
            ("iiii", Disposition::Unreached, false),
        ])
        .accounting(&first("gggg"))
        .accepted,
        1,
        "and the same of the mutations nothing reached"
    );
    assert_eq!(
        three([
            ("jjjj", Disposition::Equivalent { route: route() }, false),
            ("kkkk", Disposition::Equivalent { route: route() }, false),
            ("llll", Disposition::Equivalent { route: route() }, false),
        ])
        .accounting(&first("jjjj"))
        .accepted,
        1,
        "and of the ones the compiler rendered identically"
    );
}
