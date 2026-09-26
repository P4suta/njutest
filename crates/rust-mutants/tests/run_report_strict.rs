// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The current report reader distinguishes an explicitly absent fact from a field the document omitted.

#![expect(
    clippy::expect_used,
    reason = "a test reports malformed fixture setup by panicking"
)]

use rust_mutants::report::run::{DocumentError, RunDocument};

fn replace(document: &mut serde_json::Value, pointer: &str, value: serde_json::Value) {
    let slot = document.pointer_mut(pointer).expect("the fixture pointer");
    *slot = value;
}

#[test]
fn every_nullable_v1_report_field_is_still_a_required_key() {
    fn remove(document: &mut serde_json::Value, pointer: &str) {
        let (parent, field) = pointer.rsplit_once('/').expect("a JSON pointer to a field");
        let removed = document
            .pointer_mut(parent)
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|object| object.remove(field));
        assert!(removed.is_some(), "the fixture carries {pointer}");
    }

    let exact: serde_json::Value = njutest_devkit::strictjson::decode_str(include_str!(
        "../../../fuzz/seeds/run_report/one-run.json"
    ))
    .expect("the current report seed");
    serde_json::from_value::<RunDocument>(exact.clone()).expect("the exact v1 report");

    for pointer in [
        "/score",
        "/run/shard",
        "/selection/mutant_steps",
        "/mutants/0/step_notice",
        "/mutants/0/tests_run",
        "/mutants/0/signal",
        "/mutants/0/not_run_reason",
        "/mutants/0/route",
        "/mutants/0/route/fallback",
        "/mutants/0/source_run_id",
        "/findings/0/mutant",
    ] {
        let mut missing = exact.clone();
        remove(&mut missing, pointer);
        assert!(
            serde_json::from_value::<RunDocument>(missing).is_err(),
            "{pointer} must be explicitly present even when its value is null"
        );
    }

    let mut with_locator = exact;
    replace(
        &mut with_locator,
        "/expectations",
        serde_json::json!([{
            "id": "declared",
            "locator": {
                "path": "src/lib.rs",
                "item": "answer",
                "rule": "return-default",
                "original": "answer",
                "line": null,
                "count": null
            },
            "reason": "the invariant says this is unchanged",
            "outcome": "survived",
            "mutant": null,
            "covered": null,
            "standing": "unmatched",
            "actual": null,
            "why": "the locator names nothing",
            "where": { "cfg": null, "env": {} }
        }]),
    );
    serde_json::from_value::<RunDocument>(with_locator.clone())
        .expect("the exact nullable locator shape");
    for pointer in [
        "/expectations/0/locator",
        "/expectations/0/covered",
        "/expectations/0/locator/line",
        "/expectations/0/locator/count",
        "/expectations/0/where",
        "/expectations/0/where/cfg",
    ] {
        let mut missing = with_locator.clone();
        remove(&mut missing, pointer);
        assert!(
            serde_json::from_value::<RunDocument>(missing).is_err(),
            "{pointer} must be explicitly present even when its value is null"
        );
    }
}

#[test]
fn a_non_reusable_outcome_cannot_claim_cache_provenance() {
    let mut value: serde_json::Value = njutest_devkit::strictjson::decode_str(include_str!(
        "../../../fuzz/seeds/run_report/one-run.json"
    ))
    .expect("the current report seed");
    replace(
        &mut value,
        "/mutants/0/outcome",
        serde_json::json!("errored"),
    );
    replace(
        &mut value,
        "/mutants/0/source_run_id",
        serde_json::json!("earlier-run"),
    );

    let document: RunDocument = serde_json::from_value(value).expect("the current wire shape");
    assert!(matches!(
        document.validate(),
        Err(DocumentError::ReuseProvenance { .. })
    ));
}
