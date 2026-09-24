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
            &json!({ "state": "single-threaded", "because": [{ "kind": "starts" }] }),
            &libtest("1", Some(0))
        ),
        "a proven binary names no reason it is not"
    );
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
