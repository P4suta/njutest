// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each disposition of the shared judging comes to for a fault, and which of them leave a run short of assured.

use njutest::assure::faults::decided;
use njutest::assure::mutation::{Disposition, Unconfirmed};
use njutest::report::FindingKind;
use njutest::report::faults::{FaultDecision, FaultRecord, found};

#[test]
fn every_disposition_comes_to_one_fault_decision_and_the_impossible_ones_fail_closed() {
    let on = || "pkg/test/it".to_owned();
    let cases = [
        (Disposition::Killed { by: on() }, "noticed"),
        (Disposition::Unreached, "unreached"),
        (Disposition::Waited { on: on() }, "waited"),
        (
            Disposition::StepLimitReached {
                on: on(),
                boundary: njutest::report::StepBoundary::new(10, 11).expect("a boundary"),
            },
            "waited",
        ),
        (
            Disposition::Unconfirmed {
                on: on(),
                why: Unconfirmed::ControlFailed {
                    detail: "the original failed too".to_owned(),
                },
            },
            "undecided",
        ),
        (
            Disposition::Errored {
                on: on(),
                detail: "the harness would not start".to_owned(),
            },
            "undecided",
        ),
        (
            Disposition::Rejected {
                diagnostic: "error[E0277]: the trait bound".to_owned(),
            },
            "not-put",
        ),
        (
            Disposition::Survived {
                route: rust_mutants::session::Route::All {
                    reaching: vec!["pkg/test/it".to_owned()],
                    fallback: rust_mutants::session::Fallback::NotMeasured,
                },
            },
            "unnoticed",
        ),
        (
            Disposition::Survived {
                route: rust_mutants::session::Route::Discharged {
                    discharged: Vec::new(),
                },
            },
            "undecided",
        ),
        (
            Disposition::Equivalent {
                route: rust_mutants::session::Route::All {
                    reaching: vec!["pkg/test/it".to_owned()],
                    fallback: rust_mutants::session::Fallback::NotMeasured,
                },
            },
            "undecided",
        ),
    ];
    for (disposition, expected) in cases {
        assert_eq!(decided(&disposition).name(), expected, "{disposition:?}");
    }
}

#[test]
fn a_fault_the_run_could_not_decide_is_a_finding_so_the_run_is_not_assured() {
    let record = |decision| FaultRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "c".repeat(64),
        display_id: "c".repeat(20),
        path: "src/lib.rs".to_owned(),
        item: "load".to_owned(),
        position: None,
        decision,
    };
    let records: Vec<FaultRecord> = FaultDecision::every().into_iter().map(record).collect();
    let kinds: Vec<FindingKind> = found(&records).iter().map(|finding| finding.kind).collect();
    assert_eq!(
        kinds,
        vec![
            FindingKind::UnnoticedFault,
            FindingKind::NotMeasured,
            FindingKind::NotMeasured
        ],
        "one finding for the failure nothing noticed, then one for each the run could not \
         decide, in the order the records hold them"
    );
}
