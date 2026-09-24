// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The concurrency audit re-derives every reason an engine recording witnesses and holds the reported standing to it exactly.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use serde_json::json;
use xtask::concurrency::{UnwitnessedError, Witnessed, agrees, derived};

fn libtest(threads: &str, loose: Option<u64>) -> Witnessed {
    Witnessed {
        loose,
        kind: Some(("lib".to_owned(), true)),
        args: Some(vec![format!("--test-threads={threads}")]),
    }
}

fn holds(standing: &serde_json::Value, witnessed: &Witnessed) -> bool {
    agrees(standing, &derived(witnessed).expect("witnessed")).is_ok()
}

#[test]
fn a_standing_is_held_to_every_reason_the_recording_witnesses() {
    let single = json!({ "state": "single-threaded" });
    let loose = json!({ "state": "concurrent", "because": [{ "kind": "loose-reach" }] });
    let parallel = json!({ "state": "concurrent", "because": [{ "kind": "parallel-tests" }] });
    let spawns = json!({ "state": "concurrent", "because": [
        { "kind": "starts", "package": "p", "path": "src/lib.rs", "line": 1, "what": "spawn" }
    ] });
    let untouched = json!({ "state": "not-proven", "why": [{ "kind": "no-touch" }] });
    let native = json!({ "state": "not-proven", "why": [{ "kind": "native-code", "package": "p", "by": "links" }] });
    assert!(holds(&single, &libtest("1", Some(0))));
    assert!(holds(&loose, &libtest("1", Some(3))));
    assert!(holds(&parallel, &libtest("4", Some(0))));
    assert!(holds(&spawns, &libtest("1", Some(0))));
    assert!(holds(&untouched, &libtest("1", None)));
    for (lie, witnessed, why) in [
        (
            &single,
            libtest("1", Some(3)),
            "single-threaded over loose reach",
        ),
        (
            &single,
            libtest("4", Some(0)),
            "single-threaded under a parallel harness",
        ),
        (
            &single,
            libtest("1", None),
            "single-threaded with no reach recorded",
        ),
        (
            &loose,
            libtest("1", Some(0)),
            "loose reach the baseline never had",
        ),
        (
            &spawns,
            libtest("1", Some(3)),
            "concurrent, leaving out the loose reach",
        ),
        (
            &spawns,
            libtest("4", Some(0)),
            "concurrent, leaving out the parallel harness",
        ),
        (
            &native,
            libtest("1", Some(3)),
            "not proven where loose reach makes it concurrent",
        ),
        (
            &native,
            libtest("1", None),
            "not proven, leaving out that nothing was recorded",
        ),
        (
            &untouched,
            libtest("1", Some(0)),
            "no touch where the baseline recorded reach",
        ),
    ] {
        assert!(!holds(lie, &witnessed), "the audit accepted {why}");
    }
    let doctest = Witnessed {
        loose: Some(0),
        kind: Some(("doc".to_owned(), true)),
        args: Some(Vec::new()),
    };
    assert!(
        !holds(&single, &doctest),
        "the audit accepted a proven doctest binary"
    );
    assert!(holds(
        &json!({ "state": "not-proven", "why": [{ "kind": "doctest" }] }),
        &doctest
    ));
    assert!(
        !holds(&json!({ "state": "threaded" }), &libtest("1", Some(0))),
        "a state no run gives is a violation, never passed over"
    );
}

#[test]
fn a_reason_or_a_thread_count_no_run_gives_is_refused() {
    let single = json!({ "state": "single-threaded" });
    assert!(
        !holds(
            &json!({ "state": "concurrent", "because": [{ "kind": "banana" }] }),
            &libtest("1", Some(0))
        ),
        "a reason no run gives is no reason"
    );
    for skipped in [
        vec!["--skip".to_owned(), "--test-threads=1".to_owned()],
        vec!["--".to_owned(), "--test-threads=1".to_owned()],
    ] {
        let witnessed = Witnessed {
            loose: Some(0),
            kind: Some(("lib".to_owned(), true)),
            args: Some(skipped),
        };
        assert!(
            !holds(&single, &witnessed),
            "a `--test-threads=1` libtest reads as something else is no single thread"
        );
    }
}

#[test]
fn a_binary_the_recording_says_too_little_about_is_not_derived() {
    assert_eq!(
        derived(&Witnessed {
            loose: Some(0),
            kind: None,
            args: Some(Vec::new())
        }),
        Err(UnwitnessedError::TargetUnbuilt)
    );
    assert_eq!(
        derived(&Witnessed {
            loose: Some(0),
            kind: Some(("lib".to_owned(), true)),
            args: None
        }),
        Err(UnwitnessedError::ArgumentsUnrecorded),
        "a libtest binary whose baseline arguments were not recorded has no known thread count"
    );
}

mod explored {
    use std::collections::BTreeSet;

    use serde_json::json;
    use xtask::concurrency::{Explored, Run, agrees_explored, replayed};
    use xtask::knobs::Ended;

    fn run(delayed: Option<u64>, ended: Ended, failed: &[&str]) -> Run {
        Run {
            delayed,
            ended,
            failed: failed.iter().map(|one| (*one).to_owned()).collect(),
        }
    }

    fn broke_at(site: u64) -> Vec<Run> {
        let mut runs = vec![run(Some(site), Ended::Failed, &["t"])];
        for _ in 0..5 {
            runs.push(run(Some(site), Ended::Failed, &["t"]));
            runs.push(run(None, Ended::Passed, &[]));
        }
        runs
    }

    fn holds(report: &serde_json::Value, runs: &[Run]) -> bool {
        replayed(runs).is_ok_and(|derived| agrees_explored(report, &derived).is_ok())
    }

    #[test]
    fn a_broken_schedule_is_five_rounds_that_repeat_the_failure_and_pass_without_the_delay() {
        let mut runs = vec![run(Some(2), Ended::Passed, &[])];
        runs.extend(broke_at(7));
        assert_eq!(
            replayed(&runs).expect("a sequence the run gives"),
            Explored::Broke {
                site: 7,
                failed: BTreeSet::from(["t".to_owned()]),
                rounds: 5
            }
        );
        let broke = json!({ "state": "broke", "site": 7, "path": "src/lib.rs", "line": 3, "failed": ["t"], "rounds": 5 });
        assert!(holds(&broke, &runs));
        let flaky = [
            run(Some(7), Ended::Failed, &["t"]),
            run(Some(7), Ended::Failed, &["t"]),
            run(None, Ended::Passed, &[]),
            run(Some(7), Ended::Failed, &["t", "other"]),
        ];
        assert!(
            !holds(&broke, &flaky),
            "a round that failed other tests confirms nothing"
        );
        let dirty = [
            run(Some(7), Ended::Failed, &["t"]),
            run(Some(7), Ended::Failed, &["t"]),
            run(None, Ended::Failed, &["t"]),
        ];
        assert!(
            !holds(&broke, &dirty),
            "a test that fails without the delay is broken everywhere"
        );
    }

    #[test]
    fn every_state_a_report_gives_is_the_one_the_recorded_sequence_comes_to() {
        let passed = [
            run(Some(1), Ended::Passed, &[]),
            run(Some(2), Ended::Passed, &[]),
        ];
        let sampled = json!({ "state": "sampled", "asked": 2, "delayed": [1, 2] });
        assert!(holds(&sampled, &passed));
        let waited = [
            run(Some(1), Ended::Passed, &[]),
            run(Some(2), Ended::Waited, &[]),
        ];
        assert!(
            !holds(&sampled, &waited),
            "a delay that settled nothing is no passing sample"
        );
        let undecided =
            json!({ "state": "undecided", "asked": 2, "delayed": [1, 2], "undecided": [2] });
        assert!(holds(&undecided, &waited));
        let unexplored = json!({ "state": "unexplored", "why": "not-asked" });
        assert!(holds(&unexplored, &[]));
        assert!(
            !holds(&unexplored, &broke_at(4)),
            "a binary the engine broke is not one the report may call unexplored"
        );
        assert!(
            !holds(
                &json!({ "state": "sampled", "asked": 1, "delayed": [1] }),
                &passed
            ),
            "a report may not drop a site the engine delayed"
        );
        let mut broke = broke_at(4);
        broke.push(run(Some(5), Ended::Passed, &[]));
        assert!(
            replayed(&broke).is_err(),
            "nothing runs after a schedule broke"
        );
        assert!(
            replayed(&[run(None, Ended::Passed, &[])]).is_err(),
            "an undelayed control before any delayed one is none the procedure starts"
        );
        assert!(!holds(&json!({ "state": "explored" }), &passed));
    }
}
