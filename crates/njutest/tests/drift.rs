// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says about a target whose baseline reach moved on a control, from the records alone.

use std::collections::BTreeSet;

use njutest::report::drift::{self, Drift, Moved, Unmeasured};
use njutest::report::{CatalogIndex, Decided, Discharged, MutantRecord, Position, Reuse, Routing};

const TARGET: &str = "pkg/lib/pkg";

const fn nothing() -> Moved {
    Moved {
        gained: BTreeSet::new(),
        lost: BTreeSet::new(),
    }
}

fn moved() -> Drift {
    Drift::Moved {
        target: TARGET.to_owned(),
        reached: Moved {
            gained: BTreeSet::from([4]),
            lost: BTreeSet::new(),
        },
        bodies: nothing(),
        infected: nothing(),
        entered: nothing(),
    }
}

fn held() -> Drift {
    Drift::Held {
        target: TARGET.to_owned(),
    }
}

/// One row decided as `outcome`, whose route discharged `TARGET` when `discharged` says so.
fn row(index: u32, outcome: Decided, discharged: bool) -> MutantRecord {
    MutantRecord {
        catalog_index: CatalogIndex::new(index),
        id: format!("{index:064}"),
        display_id: format!("{index:020}"),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 1,
            column: 1,
            character_column: 1,
        },
        rule: "gt-to-ge@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome,
        accepted: false,
        blind_in: Vec::new(),
        routing: Some(Routing {
            granularity: if discharged {
                rust_mutants::session::Granularity::Discharged
            } else {
                rust_mutants::session::Granularity::Block
            },
            reaching: if discharged {
                Vec::new()
            } else {
                vec![TARGET.to_owned()]
            },
            discharged: if discharged {
                vec![Discharged {
                    target: TARGET.to_owned(),
                    proof: rust_mutants::session::NEVER_INFECTED,
                }]
            } else {
                Vec::new()
            },
            fallback: None,
            answered: Vec::new(),
        }),
        reuse: Reuse(njutest::report::Established::Here),
    }
}

#[test]
fn a_move_one_control_saw_is_not_undone_by_another_that_saw_none() {
    let folded = drift::folded([TARGET], [moved(), held()]);
    assert_eq!(folded, [moved()], "one observation of a move suffices");
    let folded = drift::folded([TARGET], [held(), moved()]);
    assert_eq!(folded, [moved()], "whichever order the controls ran in");
}

#[test]
fn a_measured_target_no_control_compared_is_not_measured_and_says_why() {
    let other = Drift::NotMeasured {
        target: TARGET.to_owned(),
        why: Unmeasured::OtherTests,
    };
    assert_eq!(
        drift::folded([TARGET, "pkg/test/it"], [other.clone()]),
        [
            other,
            Drift::NotMeasured {
                target: "pkg/test/it".to_owned(),
                why: Unmeasured::NoControl,
            },
        ],
        "a control over other tests is a reason, and no control at all is another"
    );
    assert_eq!(
        drift::folded(
            [TARGET],
            [
                Drift::NotMeasured {
                    target: TARGET.to_owned(),
                    why: Unmeasured::Unrecorded,
                },
                held(),
            ]
        ),
        [held()],
        "a comparison made stands over one that could not be"
    );
    let Some(limitation) = drift::unmeasured(&drift::folded([TARGET, "pkg/test/it"], [held()]))
    else {
        panic!("a target nothing compared is stated");
    };
    assert_eq!(limitation.name, njutest::limitation::DRIFT_NOT_MEASURED);
    assert!(
        limitation.detail.ends_with("(pkg/test/it)") && limitation.detail.contains("1 target"),
        "{}",
        limitation.detail
    );
    assert_eq!(
        drift::unmeasured(&[held()]),
        None,
        "and nothing is stated when all held"
    );
}

#[test]
fn the_finding_counts_what_a_proof_decided_on_the_moved_record_and_nothing_a_kill_did() {
    let rows = [
        row(0, Decided::Survived, true),
        row(
            1,
            Decided::Killed {
                by: "pkg/test/it".to_owned(),
            },
            true,
        ),
        row(2, Decided::Unreached, false),
        row(3, Decided::Survived, false),
    ];
    let found = drift::found(&[moved(), held()], &rows);
    let [finding] = found.as_slice() else {
        panic!("one moved target, one finding: {found:?}");
    };
    assert_eq!(finding.kind, njutest::report::FindingKind::UnstableBaseline);
    assert!(
        finding
            .detail
            .contains("1 mutation a proof removed its run of, and 1 mutation no test reached"),
        "a kill is existential and rests on no discharge; a survivor a discharge decided and \
         an unreached claim both rest on the record that moved: {}",
        finding.detail
    );
}

#[test]
fn a_row_the_run_established_nothing_about_is_not_counted_as_resting_on_a_discharge() {
    let boundary = njutest::report::StepBoundary::new(10, 11).expect("a boundary");
    let rows = [
        row(0, Decided::Survived, true),
        row(
            1,
            Decided::Errored {
                on: TARGET.to_owned(),
            },
            true,
        ),
        row(
            2,
            Decided::Unconfirmed {
                on: TARGET.to_owned(),
            },
            true,
        ),
        row(
            3,
            Decided::Waited {
                on: TARGET.to_owned(),
            },
            true,
        ),
        row(
            4,
            Decided::StepLimitReached {
                on: TARGET.to_owned(),
                boundary,
            },
            true,
        ),
    ];
    let found = drift::found(&[moved()], &rows);
    let [finding] = found.as_slice() else {
        panic!("one moved target, one finding: {found:?}");
    };
    assert!(
        finding
            .detail
            .contains("1 mutation a proof removed its run of, and 0 mutations no test reached"),
        "only a survivor is a disposition a discharge decided; a row that errored, waited, \
         did not confirm, or crossed its step allowance is a hole whatever its route \
         discharged: {}",
        finding.detail
    );
}

#[test]
fn a_survivor_the_moved_target_was_routed_away_from_by_reach_rests_on_it() {
    let mut record = row(0, Decided::Survived, false);
    record.routing = Some(Routing {
        granularity: rust_mutants::session::Granularity::Block,
        reaching: vec!["pkg/test/other".to_owned()],
        discharged: Vec::new(),
        fallback: None,
        answered: Vec::new(),
    });
    let found = drift::found(&[moved()], &[record]);
    let [finding] = found.as_slice() else {
        panic!("{found:?}");
    };
    assert!(
        finding
            .detail
            .contains("1 mutation a proof removed its run of"),
        "a target the baseline's reach kept off a mutation's route was removed by the same \
         measurement that moved, so the survival rests on it as a discharge does: {}",
        finding.detail
    );
}

#[test]
fn a_survivor_this_run_decided_no_route_for_rests_on_the_moved_target() {
    let mut record = row(0, Decided::Survived, false);
    record.routing = None;
    let found = drift::found(&[moved()], &[record]);
    let [finding] = found.as_slice() else {
        panic!("{found:?}");
    };
    assert!(
        finding
            .detail
            .contains("1 mutation a proof removed its run of"),
        "a survival carried in from an interrupted run was routed on that run's baseline, which \
         this run cannot vouch for, so it is counted rather than presumed independent: {}",
        finding.detail
    );
}
