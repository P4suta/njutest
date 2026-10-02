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
        "/targets/0/sealed/remedy",
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
fn a_place_counted_as_evaluated_before_the_program_runs_is_a_candidate_left_out_for_it() {
    let seed: serde_json::Value = njutest_devkit::strictjson::decode_str(include_str!(
        "../../../fuzz/seeds/run_report/one-run.json"
    ))
    .expect("the current report seed");
    let with = |reason: &str, counted: Option<u32>| {
        let mut value = seed.clone();
        replace(
            &mut value,
            "/rejections",
            serde_json::json!([{
                "index": 13, "id": "c".repeat(64), "display_id": "c".repeat(20),
                "path": "src/lib.rs", "rule": "add-to-sub", "code": "E0015",
                "diagnostic": "error[E0015]: cannot call non-const function `double` in constants",
                "isolated": true, "reason": reason
            }]),
        );
        let mut skips = value
            .pointer("/skips")
            .and_then(serde_json::Value::as_array)
            .expect("the seed's skips")
            .clone();
        if let Some(count) = counted {
            skips.push(serde_json::json!({
                "reason": "evaluated-before-run", "path": "src/lib.rs", "count": count,
                "explanation": "the function is evaluated before the program runs"
            }));
        }
        let skipped = match counted {
            Some(count) => count.checked_add(23).expect("a small count"),
            None => 23,
        };
        replace(&mut value, "/skips", serde_json::Value::Array(skips));
        replace(
            &mut value,
            "/accounting/skipped",
            serde_json::json!(skipped),
        );
        let refused = u32::from(reason == "compiler-refused");
        replace(
            &mut value,
            "/accounting/refused",
            serde_json::json!(refused),
        );
        serde_json::from_value::<RunDocument>(value)
            .expect("the current wire shape")
            .validate()
    };
    assert!(
        with("evaluated-before-run", Some(1)).is_ok(),
        "a candidate of a function the compiler evaluates is a rejection for its identity and a \
         skipped place for the count, and neither is a refusal"
    );
    assert!(
        with("compiler-refused", None).is_ok(),
        "a refusal is counted as refused and is no skipped place"
    );
    for (counted, left) in [(None, 1), (Some(2), 1)] {
        assert!(
            matches!(
                with("evaluated-before-run", counted),
                Err(DocumentError::PassedOver { left: l, .. }) if l == left
            ),
            "a report whose skips and rejections disagree about the places a function evaluated \
             before the program runs holds is refused, whichever of the two is wrong: {counted:?}"
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
