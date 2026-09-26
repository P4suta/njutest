// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run did, counted rather than timed.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use rust_mutants::report::run::RunDocument;
use rust_mutants::work::{Removal, Work};

/// A report of `targets` targets and the rows given, with everything else a run writes filled in.
fn document(targets: &[&str], rows: &[serde_json::Value]) -> RunDocument {
    let value = serde_json::json!({
        "document_type": "rust-mutants/run-report",
        "schema_version": 3,
        "tool_version": "0.1.0",
        "run": {
            "id": "20260907T000000000Z",
            "started_at": "2026-09-07T00:00:00Z",
            "finished_at": "2026-09-07T00:00:01Z",
            "duration_ms": 1000,
            "interrupted": false,
            "exit_code": 0,
            "shard": null,
            "jobs": {"asked": "auto", "used": 1}
        },
        "workspace": {
            "root_name": "demo",
            "toolchain": "cargo 1.98.0 / rustc 1.98.0",
            "workspace_digest": "a".repeat(64),
            "catalog_digest": "b".repeat(64),
            "platform": {"os": "linux", "arch": "x86_64", "target": "x86_64-unknown-linux-gnu"}
        },
        "selection": {
            "tier": "balanced", "operators": [], "include": [], "exclude": [], "packages": [],
            "build": [], "mutant_steps": null
        },
        "targets": targets
            .iter()
            .map(|id| serde_json::json!({
                "id": id, "kind": "test", "harness": true, "tests": 1, "limitations": []
            }))
            .collect::<Vec<_>>(),
        "accounting": {
            "cataloged": rows.len(), "refused": 0, "skipped": 0, "executed": 0,
            "killed": 0, "survived": 0, "step_limit_reached": 0, "waited": 0, "inconclusive": 0, "errored": 0,
            "not_run": 0, "unreached": 0, "discharged": 0, "declined": 0, "expected": 0
        },
        "score": null,
        "established_tests": 0,
        "mutants": rows,
        "rejections": [],
        "skips": [],
        "expectations": [],
        "findings": [],
        "facts": []
    });
    serde_json::from_value(value).expect("the report reads back")
}

/// One row, with everything a work ledger does not read filled in.
fn row(index: u32, extra: &serde_json::Value) -> serde_json::Value {
    let mut value = serde_json::json!({
        "index": index,
        "id": format!("{index:064x}"),
        "display_id": format!("{index:020x}"),
        "path": "src/lib.rs",
        "package": "demo",
        "family": "comparison",
        "rule": "gt-to-ge",
        "item": "answer",
        "rule_version": 1,
        "line": 11,
        "column": 8,
        "start_byte": 10,
        "end_byte": 11,
        "source_digest": "c".repeat(64),
        "original": ">",
        "replacement": ">=",
        "outcome": "killed",
        "step_notice": null,
        "target": "demo/test/one",
        "exit_code": 0,
        "duration_ms": 1,
        "tests_run": 1,
        "killed_by": [],
        "signal": null,
        "retried": false,
        "lingered": false,
        "not_run_reason": null,
        "declined": [],
        "route": null,
        "identical": "not-measured",
        "expected": false,
        "unreached": false,
        "source_run_id": null
    });
    let (Some(object), Some(more)) = (value.as_object_mut(), extra.as_object()) else {
        panic!("both are objects");
    };
    for (name, one) in more {
        let replaced_fixture_field = object.insert(name.clone(), one.clone());
        assert!(
            replaced_fixture_field.is_some(),
            "the fixture override must name an existing report field: {name}"
        );
    }
    if let Some(route) = value
        .get_mut("route")
        .and_then(serde_json::Value::as_object_mut)
    {
        route.entry("fallback").or_insert(serde_json::Value::Null);
        route
            .entry("discharged")
            .or_insert_with(|| serde_json::json!([]));
        route
            .entry("executed")
            .or_insert_with(|| serde_json::json!([]));
        route
            .entry("tests")
            .or_insert_with(|| serde_json::json!({}));
    }
    value
}

fn pairs(work: &Work, reason: &str) -> u64 {
    work.removed
        .iter()
        .find(|removed| removed.reason == reason)
        .map_or(0, |removed| removed.pairs)
}

#[test]
fn every_pair_a_whole_run_would_start_is_started_or_removed_by_something_named() {
    let document = document(
        &["demo/test/one", "demo/test/two", "demo/lib/demo"],
        &[
            row(
                0,
                &serde_json::json!({
                    "route": {
                        "granularity": "block",
                        "reaching": ["demo/test/one", "demo/test/two"],
                        "executed": ["demo/test/one"]
                    }
                }),
            ),
            row(
                1,
                &serde_json::json!({
                    "outcome": "not_run",
                    "not_run_reason": "unreached",
                    "unreached": true,
                    "route": {"granularity": "unreached", "reaching": []}
                }),
            ),
        ],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(work.targets, 3);
    assert_eq!(work.cataloged, 2);
    assert_eq!(work.whole, 6, "two mutants against three targets");
    assert_eq!(work.started, 1, "one process was started");
    assert!(
        work.balances().expect("exact balance arithmetic"),
        "a pair nothing accounts for is work nobody can explain: {work:?}"
    );
    assert_eq!(
        work.started + work.removed.iter().map(|one| one.pairs).sum::<u64>(),
        work.whole
    );
}

#[test]
fn a_mutation_no_measured_target_reaches_removes_every_pair_it_had() {
    let document = document(
        &["demo/test/one", "demo/test/two"],
        &[row(
            0,
            &serde_json::json!({
                "outcome": "not_run",
                "not_run_reason": "unreached",
                "unreached": true,
                "route": {"granularity": "unreached", "reaching": []}
            }),
        )],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(work.started, 0);
    assert_eq!(pairs(&work, "unreached"), 2);
    assert!(work.balances().expect("exact balance arithmetic"));
}

#[test]
fn a_proof_that_discharges_a_target_removes_that_pair_under_its_own_name() {
    let document = document(
        &["demo/test/one", "demo/test/two"],
        &[row(
            0,
            &serde_json::json!({
                "outcome": "not_run",
                "not_run_reason": "discharged",
                "route": {
                    "granularity": "discharged",
                    "reaching": [],
                    "discharged": [
                        {"target": "demo/test/one", "proof": "branch-never-taken"},
                        {"target": "demo/test/two", "proof": "never-infected"}
                    ]
                }
            }),
        )],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(work.started, 0);
    assert_eq!(pairs(&work, "branch-never-taken"), 1);
    assert_eq!(pairs(&work, "never-infected"), 1);
    assert!(work.balances().expect("exact balance arithmetic"));
    for removed in &work.removed {
        assert_eq!(removed.removal, Removal::Proof, "{removed:?}");
    }
}

#[test]
fn a_target_that_answered_removes_the_targets_that_would_have_been_asked_after_it() {
    let document = document(
        &["demo/test/one", "demo/test/two", "demo/test/three"],
        &[row(
            0,
            &serde_json::json!({
                "route": {
                    "granularity": "all",
                    "reaching": ["demo/test/one", "demo/test/two", "demo/test/three"],
                    "executed": ["demo/test/one"]
                }
            }),
        )],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(work.started, 1);
    assert_eq!(
        pairs(&work, "answered"),
        2,
        "a mutant one test noticed is noticed; asking the rest establishes nothing"
    );
    assert!(work.balances().expect("exact balance arithmetic"));
}

#[test]
fn an_outcome_an_earlier_run_established_removes_every_pair_of_it() {
    let document = document(
        &["demo/test/one", "demo/test/two"],
        &[row(
            0,
            &serde_json::json!({
                "source_run_id": "20260901T000000000Z",
                "route": {
                    "granularity": "all",
                    "reaching": ["demo/test/one", "demo/test/two"],
                    "executed": []
                }
            }),
        )],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(work.started, 0);
    assert_eq!(pairs(&work, "reused"), 2);
    assert_eq!(work.removed[0].removal, Removal::Memory);
    assert!(work.balances().expect("exact balance arithmetic"));
}

#[test]
fn a_confirming_retry_is_one_pair_started_twice() {
    let document = document(
        &["demo/test/one"],
        &[row(
            0,
            &serde_json::json!({
                "outcome": "waited",
                "retried": true,
                "lingered": false,
                "route": {
                    "granularity": "all",
                    "reaching": ["demo/test/one"],
                    "executed": ["demo/test/one"]
                }
            }),
        )],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(
        work.started, 2,
        "a timeout recorded only after its confirming retry cost two processes"
    );
    assert!(
        work.started > work.whole,
        "a retry is work the whole did not budget for, and the ledger says so rather than hiding it"
    );
}

#[test]
fn a_filter_and_an_early_stop_are_removals_a_reader_can_tell_from_a_proof() {
    let document = document(
        &["demo/test/one"],
        &[
            row(
                0,
                &serde_json::json!({"outcome": "not_run", "not_run_reason": "unselected"}),
            ),
            row(
                1,
                &serde_json::json!({"outcome": "not_run", "not_run_reason": "stopped-early"}),
            ),
        ],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(pairs(&work, "unselected"), 1);
    assert_eq!(pairs(&work, "stopped-early"), 1);
    for removed in &work.removed {
        assert_eq!(
            removed.removal,
            Removal::Selection,
            "a run that was asked for less did less; that is not a proof: {removed:?}"
        );
    }
    assert!(work.balances().expect("exact balance arithmetic"));
}

#[test]
fn the_share_a_run_did_not_do_is_what_it_removed_over_the_whole() {
    let document = document(
        &[
            "demo/test/one",
            "demo/test/two",
            "demo/test/three",
            "demo/test/four",
        ],
        &[row(
            0,
            &serde_json::json!({
                "route": {
                    "granularity": "block",
                    "reaching": ["demo/test/one"],
                    "executed": ["demo/test/one"]
                }
            }),
        )],
    );
    let work = Work::of(&document).expect("valid work ledger");
    assert_eq!(work.whole, 4);
    assert_eq!(work.started, 1);
    assert!(
        (work.saved().expect("exact skipped count") - 0.75).abs() < 1e-12,
        "{work:?}"
    );
    assert!(
        document_of_nothing()
            .saved()
            .expect("exact empty skipped count")
            .abs()
            < 1e-12,
        "a run with nothing to do saved nothing rather than everything"
    );
}

fn document_of_nothing() -> Work {
    Work::of(&document(&[], &[])).expect("valid empty work ledger")
}
