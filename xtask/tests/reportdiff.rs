// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a review sees when a change changes what a run claims.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use xtask::reportdiff::compare;

fn report(body: &serde_json::Value) -> String {
    body.to_string()
}

fn diff(before: &str, after: &str) -> Vec<String> {
    compare(("a.json", before), ("b.json", after))
        .expect("both documents are JSON")
        .into_iter()
        .map(|change| change.to_string())
        .collect()
}

#[test]
fn two_reports_that_claim_the_same_thing_differ_in_nothing() {
    let one = report(&serde_json::json!({
        "verdict": "ASSURED",
        "accounting": { "targets": { "selected": 3, "passed": 3 } },
        "targets": [{ "name": "a", "status": "passed" }],
        "findings": [],
        "limitations": [{ "name": "doctests-not-routed" }],
    }));
    assert_eq!(diff(&one, &one), Vec::<String>::new());
}

#[test]
fn a_verdict_that_moved_is_the_first_thing_a_reviewer_sees() {
    let before = report(&serde_json::json!({ "verdict": "ASSURED" }));
    let after = report(&serde_json::json!({ "verdict": "DEFECT" }));
    assert_eq!(diff(&before, &after), ["verdict\tASSURED\tDEFECT"]);
}

#[test]
fn a_count_that_moved_names_the_count() {
    let before = report(&serde_json::json!({
        "accounting": { "mutants": { "killed": 10, "survived": 2 } }
    }));
    let after = report(&serde_json::json!({
        "accounting": { "mutants": { "killed": 11, "survived": 1 } }
    }));
    assert_eq!(
        diff(&before, &after),
        [
            "accounting.mutants.killed\t10\t11",
            "accounting.mutants.survived\t2\t1"
        ]
    );
}

#[test]
fn a_finding_that_appeared_and_one_that_went_away_are_both_shown() {
    let before = report(&serde_json::json!({ "findings": [{ "subject": "old" }] }));
    let after = report(&serde_json::json!({ "findings": [{ "subject": "new" }] }));
    assert_eq!(
        diff(&before, &after),
        ["findings.new\t—\tpresent", "findings.old\tpresent\t—"]
    );
}

#[test]
fn a_target_that_changed_its_mind_is_named_with_both_answers() {
    let before = report(&serde_json::json!({
        "targets": [{ "name": "core::adds", "status": "passed" }]
    }));
    let after = report(&serde_json::json!({
        "targets": [{ "name": "core::adds", "status": "failed" }]
    }));
    assert_eq!(
        diff(&before, &after),
        ["targets[core::adds]\tpassed\tfailed"]
    );
}

#[test]
fn how_long_it_took_is_not_a_difference_worth_showing() {
    let before = report(&serde_json::json!({
        "verdict": "ASSURED",
        "timing": { "duration_ms": 1000 },
        "targets": [{ "name": "a", "status": "passed", "duration_ms": 5 }],
    }));
    let after = report(&serde_json::json!({
        "verdict": "ASSURED",
        "timing": { "duration_ms": 9999 },
        "targets": [{ "name": "a", "status": "passed", "duration_ms": 700 }],
    }));
    assert_eq!(
        diff(&before, &after),
        Vec::<String>::new(),
        "two machines differ in duration for reasons that are not the change"
    );
}

#[test]
fn a_document_that_is_not_a_report_is_an_error_that_names_it() {
    let error = compare(("a.json", "{}"), ("b.json", "not json")).expect_err("refused");
    assert!(error.to_string().contains("b.json"), "{error}");
}
