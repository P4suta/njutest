// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a review sees when a change changes what a run claims.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use xtask::reportdiff::{DiffError, compare};

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

fn set(document: &mut serde_json::Value, path: &str, value: serde_json::Value) {
    *document.pointer_mut(path).expect("golden report path") = value;
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
fn a_shift_between_who_decided_names_the_column_that_moved() {
    let before = report(&serde_json::json!({
        "accounting": { "mutants": { "cataloged": 12, "observers": {
            "types": 3, "tests": 7, "proved": 0, "unnoticed": 2, "unreached": 0, "undecided": 0
        } } }
    }));
    let after = report(&serde_json::json!({
        "accounting": { "mutants": { "cataloged": 12, "observers": {
            "types": 5, "tests": 7, "proved": 0, "unnoticed": 0, "unreached": 0, "undecided": 0
        } } }
    }));
    assert_eq!(
        diff(&before, &after),
        [
            "accounting.mutants.observers.types\t3\t5",
            "accounting.mutants.observers.unnoticed\t2\t0",
        ],
        "a change that moves two mutations from nobody noticing to the type system \
         catching them is the whole point of the review, and a diff that says only \
         that an object called observers is different has told a reviewer nothing"
    );
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

/// One run report, as a run report is shaped.
fn run_report(mutants: &serde_json::Value, findings: &serde_json::Value, ms: u64) -> String {
    serde_json::json!({
        "document_type": "rust-mutants/run-report",
        "run": { "id": "20260907T000000000Z", "duration_ms": ms, "exit_code": 1 },
        "accounting": { "cataloged": 2, "executed": 2, "killed": 1, "survived": 1 },
        "score": { "detected": 1, "decided": 2, "value": 0.5 },
        "mutants": mutants,
        "findings": findings,
    })
    .to_string()
}

#[test]
fn two_run_reports_that_differ_only_in_duration_are_the_same_run() {
    let rows = serde_json::json!([
        { "display_id": "aaaa", "outcome": "killed", "duration_ms": 10 },
        { "display_id": "bbbb", "outcome": "survived", "duration_ms": 20 },
    ]);
    let findings =
        serde_json::json!([{ "kind": "surviving-mutant", "mutant": "bbbb", "detail": "x" }]);
    assert_eq!(
        diff(
            &run_report(&rows, &findings, 812),
            &run_report(&rows, &findings, 4_211)
        ),
        Vec::<String>::new(),
        "a run that took longer is the same run, and a diff that says so hides the ones that \
         are not"
    );
}

#[test]
fn a_mutant_that_changed_its_outcome_is_named_with_both_answers() {
    let before = serde_json::json!([{ "display_id": "aaaa", "outcome": "killed" }]);
    let after = serde_json::json!([{ "display_id": "aaaa", "outcome": "survived" }]);
    let empty = serde_json::json!([]);
    let changes = diff(
        &run_report(&before, &empty, 10),
        &run_report(&after, &empty, 10),
    );
    assert!(
        changes.iter().any(|change| change.contains("aaaa")
            && change.contains("killed")
            && change.contains("survived")),
        "{changes:?}"
    );
}

#[test]
fn a_count_and_a_score_that_moved_are_both_shown() {
    let empty = serde_json::json!([]);
    let one = run_report(&empty, &empty, 10);
    let moved = one
        .replace("\"killed\":1", "\"killed\":2")
        .replace("0.5", "1.0");
    let changes = diff(&one, &moved);
    assert!(
        changes.iter().any(|change| change.contains("killed")),
        "{changes:?}"
    );
    assert!(
        changes.iter().any(|change| change.contains("score")),
        "{changes:?}"
    );
}

#[test]
fn a_canonical_assurance_report_that_changes_a_limitation_is_not_called_the_same() {
    let before = include_str!("../../crates/njutest/tests/testdata/report.golden.json");
    let mut after: serde_json::Value =
        xtask::strictjson::decode_str(before).expect("golden report");
    set(
        &mut after,
        "/report/builds/0/parts/0/limitations/0/detail",
        serde_json::json!("the limitation now says something else"),
    );
    let changes = diff(before, &after.to_string());
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(
        changes.first().is_some_and(
            |change| change.starts_with("report.builds[0].parts[0].limitations[0].detail\t")
        ),
        "{changes:?}"
    );
}

#[test]
fn two_report_kinds_are_never_called_the_same() {
    let assurance = include_str!("../../crates/njutest/tests/testdata/report.golden.json");
    let empty = serde_json::json!([]);
    let run = run_report(&empty, &empty, 10);
    assert_eq!(
        diff(assurance, &run),
        ["document_type\tcomplete\trust-mutants/run-report"]
    );
}

#[test]
fn an_unknown_report_kind_is_refused() {
    let alien = r#"{"document_type":"unknown-report"}"#;
    let error = compare(("alien.json", alien), ("same.json", alien)).expect_err("refused");
    assert!(error.to_string().contains("alien.json"), "{error}");
}

#[test]
fn canonical_run_identity_and_timing_are_not_claim_differences() {
    let before = include_str!("../../crates/njutest/tests/testdata/report.golden.json");
    let mut after: serde_json::Value =
        xtask::strictjson::decode_str(before).expect("golden report");
    set(
        &mut after,
        "/report/run_id",
        serde_json::json!("another-run"),
    );
    set(
        &mut after,
        "/report/builds/0/parts/0/timing/duration_ms",
        serde_json::json!(99_999),
    );
    set(
        &mut after,
        "/report/builds/0/parts/0/targets/0/duration_ms",
        serde_json::json!(9_999),
    );
    assert!(diff(before, &after.to_string()).is_empty());
}

#[test]
fn canonical_source_identities_are_provenance_differences() {
    let before = include_str!("../../crates/njutest/tests/testdata/report.golden.json");
    let mut after: serde_json::Value =
        xtask::strictjson::decode_str(before).expect("golden report");
    set(
        &mut after,
        "/report/builds/0/parts/0/run_id",
        serde_json::json!("another-source-run"),
    );
    set(
        &mut after,
        "/report/provenance/cached",
        serde_json::json!(true),
    );
    set(
        &mut after,
        "/report/provenance/source_run_id",
        serde_json::json!("another-cached-run"),
    );
    set(
        &mut after,
        "/report/builds/0/parts/0/mutants/0/reuse/source_run_id",
        serde_json::json!("another-reused-run"),
    );
    let changes = diff(before, &after.to_string());
    assert!(
        changes
            .iter()
            .any(|change| change.starts_with("report.builds[0].parts[0].run_id\t")),
        "{changes:?}"
    );
    assert!(
        changes
            .iter()
            .any(|change| change.starts_with("report.provenance.source_run_id\t")),
        "{changes:?}"
    );
    assert!(
        changes.iter().any(|change| change
            .starts_with("report.builds[0].parts[0].mutants[0].reuse.source_run_id\t")),
        "{changes:?}"
    );
}

#[test]
fn a_run_mutant_without_an_outcome_is_refused() {
    let rows = serde_json::json!([{ "display_id": "aaaa" }]);
    let empty = serde_json::json!([]);
    let malformed = run_report(&rows, &empty, 10);
    let error = compare(("malformed.json", &malformed), ("same.json", &malformed))
        .expect_err("missing outcome refused");
    assert!(matches!(error, DiffError::InvalidRunMutant { .. }));
    assert!(error.to_string().contains("malformed.json"), "{error}");
}

#[test]
fn duplicate_run_mutant_names_are_refused() {
    let rows = serde_json::json!([
        { "display_id": "aaaa", "outcome": "killed" },
        { "display_id": "aaaa", "outcome": "survived" }
    ]);
    let empty = serde_json::json!([]);
    let malformed = run_report(&rows, &empty, 10);
    let error = compare(("malformed.json", &malformed), ("same.json", &malformed))
        .expect_err("duplicate display ID refused");
    assert!(matches!(error, DiffError::InvalidRunMutant { .. }));
    assert!(error.to_string().contains("malformed.json"), "{error}");
}
