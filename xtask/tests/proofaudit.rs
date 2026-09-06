// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gate that re-decides a recorded run without asking the runner whether it agrees with itself.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::Path;

use xtask::gates;
use xtask::proofaudit::{Audit, AuditError, EXIT_UNREADABLE, Layer, Standing};

const RUN: &str = "20260906T101500Z-9f1c2d";
const EARLIER: &str = "20260905T090000Z-1a2b3c";
const KILLED: &str = "aaaaaaaaaaaaaaaaaaaa";
const SURVIVED: &str = "bbbbbbbbbbbbbbbbbbbb";
const TARGET: &str = "pkg/test/lib adds_two_numbers";
const REPORT: &str = "mjutest-assurance-report-v1.json";

fn base() -> serde_json::Value {
    serde_json::json!({
        "schema": "mjutest-assurance-report-v1",
        "schema_version": 1,
        "run_id": RUN,
        "run_kind": "scoped",
        "contract": "standard-v1",
        "verdict": "INSUFFICIENT",
        "accounting": {
            "targets": { "selected": 1, "passed": 1, "failed": 0, "skipped": 0, "missing": 0 },
            "mutants": {
                "cataloged": 2,
                "rejected": 0,
                "executed": 2,
                "killed": 1,
                "survived": 1,
                "timed_out": 0,
                "unreached": 0,
                "accepted": 0,
                "reused_killed": 0,
                "reused_survived": 0
            },
            "soundness": { "unsafe_items": 0, "packages_with_unsafe": 0, "executed": false }
        },
        "targets": [
            {
                "id": "3f2a1b0c9d8e7f60",
                "name": TARGET,
                "package": "pkg",
                "status": "passed",
                "duration_ms": 5,
                "message": null
            }
        ],
        "mutants": [
            {
                "id": "a".repeat(64),
                "display_id": KILLED,
                "path": "src/lib.rs",
                "position": { "line": 7, "column": 9, "character_column": 9 },
                "rule": "negate-condition@1",
                "outcome": "killed",
                "killed_by": TARGET,
                "reused": false,
                "source_run_id": null
            },
            {
                "id": "b".repeat(64),
                "display_id": SURVIVED,
                "path": "src/lib.rs",
                "position": { "line": 11, "column": 5, "character_column": 5 },
                "rule": "return-ok-default@1",
                "outcome": "survived",
                "killed_by": null,
                "reused": false,
                "source_run_id": null
            }
        ],
        "findings": [
            {
                "kind": "surviving-mutant",
                "subject": SURVIVED,
                "detail": "no test noticed return-ok-default@1 at src/lib.rs:11",
                "position": null
            }
        ],
        "limitations": []
    })
}

fn assured() -> serde_json::Value {
    let mut document = base();
    merge(
        &mut document,
        serde_json::json!({
            "run_kind": "full",
            "verdict": "ASSURED",
            "findings": [],
            "accounting": {
                "mutants": { "cataloged": 1, "executed": 1, "killed": 1, "survived": 0 }
            }
        }),
    );
    let mutants = document
        .get_mut("mutants")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the mutant records");
    mutants.truncate(1);
    document
}

fn merge(document: &mut serde_json::Value, overrides: serde_json::Value) {
    match (document, overrides) {
        (serde_json::Value::Object(into), serde_json::Value::Object(from)) => {
            for (key, value) in from {
                merge(into.entry(key).or_insert(serde_json::Value::Null), value);
            }
        }
        (serde_json::Value::Array(into), serde_json::Value::Array(from)) if !from.is_empty() => {
            for (at, value) in from.into_iter().enumerate() {
                match into.get_mut(at) {
                    Some(existing) => merge(existing, value),
                    None => into.push(value),
                }
            }
        }
        (into, from) => *into = from,
    }
}

fn with(overrides: serde_json::Value) -> serde_json::Value {
    let mut document = base();
    merge(&mut document, overrides);
    document
}

fn without(document: &mut serde_json::Value, group: &str, column: &str) {
    let columns = document
        .get_mut("accounting")
        .and_then(|accounting| accounting.get_mut(group))
        .and_then(serde_json::Value::as_object_mut)
        .expect("the accounting group");
    let _removed = columns.remove(column);
}

fn run_directory(document: &serde_json::Value) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(directory.path().join(REPORT), document.to_string()).expect("the recording");
    directory
}

/// A route and an execution for each mutant of [`base`], with no proof removing anything.
fn routes() -> Vec<serde_json::Value> {
    let mut lines = Vec::new();
    for (seq, (mutant, outcome)) in [(KILLED, "killed"), (SURVIVED, "survived")]
        .into_iter()
        .enumerate()
    {
        lines.push(serde_json::json!({
            "seq": seq.saturating_mul(2).saturating_add(1), "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0,
            "type": "route",
            "route": {
                "mutant": mutant, "granularity": "block", "fallback": null,
                "reaching": ["t1"], "discharged": [], "file_candidates": 1, "reused": null
            }
        }));
        lines.push(serde_json::json!({
            "seq": seq.saturating_mul(2).saturating_add(2), "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1,
            "type": "mutant-exec",
            "mutant": {
                "mutant": mutant, "target": "t1", "args": [], "outcome": outcome,
                "duration_ms": 5
            }
        }));
    }
    lines
}

fn audited(document: &serde_json::Value) -> Audit {
    let directory = run_directory(document);
    gates::proofaudit(directory.path(), None).expect("a recording this audit can read")
}

/// One recording of what a run routed and what it ran, as the trace holds it.
fn recorded(lines: &[serde_json::Value]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let mut stream = String::new();
    for line in lines {
        stream.push_str(&line.to_string());
        stream.push('\n');
    }
    std::fs::write(directory.path().join("trace.jsonl"), stream).expect("the recording");
    directory
}

fn audited_with_routes(document: &serde_json::Value) -> Audit {
    audited_with(document, &routes())
}

fn audited_with(document: &serde_json::Value, lines: &[serde_json::Value]) -> Audit {
    let run = run_directory(document);
    let trace = recorded(lines);
    gates::proofaudit(run.path(), Some(trace.path())).expect("a recording this audit can read")
}

fn subjects(document: &serde_json::Value, standing: Standing) -> Vec<String> {
    audited(document)
        .remarks
        .into_iter()
        .filter(|remark| remark.standing == standing)
        .map(|remark| remark.subject)
        .collect()
}

fn violations(document: &serde_json::Value) -> Vec<String> {
    subjects(document, Standing::Violated)
}

fn unaudited(document: &serde_json::Value) -> Vec<String> {
    subjects(document, Standing::Unaudited)
}

fn exit_code(directory: &Path) -> i32 {
    std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("proofaudit")
        .arg(directory)
        .output()
        .expect("the gate runs")
        .status
        .code()
        .expect("the gate exits of its own accord")
}

#[test]
fn a_recording_that_agrees_with_itself_has_nothing_to_report() {
    let audit = audited_with_routes(&base());
    assert_eq!(audit.violations(), 0, "{audit}");
    assert_eq!(audit.exit_code(), 0);
}

#[test]
fn an_assurance_the_run_is_wide_enough_to_reach_has_nothing_to_report() {
    let audit = audited_with_routes(&assured());
    assert_eq!(audit.violations(), 0, "{audit}");
}

#[test]
fn a_mutant_column_the_records_contradict_is_a_violation() {
    assert_eq!(
        violations(&with(
            serde_json::json!({ "accounting": { "mutants": { "killed": 5 } } })
        )),
        ["accounting.mutants.executed", "accounting.mutants.killed"],
        "the column and the records it summarises are two recordings of one fact"
    );
}

#[test]
fn a_target_column_the_records_contradict_is_a_violation() {
    assert_eq!(
        violations(&with(
            serde_json::json!({ "accounting": { "targets": { "passed": 3 } } })
        )),
        ["accounting.targets", "accounting.targets.passed"]
    );
}

#[test]
fn columns_that_do_not_add_up_to_the_catalog_are_a_violation() {
    assert!(
        violations(&with(
            serde_json::json!({ "accounting": { "mutants": { "cataloged": 3 } } })
        ))
        .contains(&"accounting.mutants".to_owned()),
        "every cataloged mutant was refused by the compiler, reached by nothing, or executed"
    );
}

#[test]
fn a_kill_by_a_target_a_proof_discharged_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[
            serde_json::json!({
                "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
                "route": {
                    "mutant": KILLED, "granularity": "block", "fallback": null,
                    "reaching": ["t1"],
                    "discharged": [{ "target": "t2", "proof": "never-infected" }],
                    "file_candidates": 2, "reused": null
                }
            }),
            serde_json::json!({
                "seq": 2, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1, "type": "mutant-exec",
                "mutant": {
                    "mutant": KILLED, "target": "t2", "args": [], "outcome": "killed",
                    "duration_ms": 5
                }
            }),
        ],
    );

    let named: Vec<String> = audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Proofs && remark.standing == Standing::Violated)
        .map(|remark| remark.subject.clone())
        .collect();
    assert_eq!(
        named,
        [KILLED],
        "a layer that would drop a target which then killed the mutant is unsound, and the \
         recording is where that is caught: {audit}"
    );
}

#[test]
fn a_recording_of_no_routes_leaves_the_proofs_unaudited() {
    let audit = audited(&base());

    assert!(
        audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Proofs && remark.standing == Standing::Unaudited),
        "fail-closed is never turning what cannot be checked into what is fine: {audit}"
    );
}

#[test]
fn an_acceptance_of_a_mutation_nothing_reached_is_not_a_violation() {
    let document = with(serde_json::json!({
        "accounting": {
            "mutants": { "executed": 1, "survived": 0, "unreached": 1, "accepted": 1 }
        },
        "mutants": [{}, { "outcome": "unreached" }],
        "findings": []
    }));

    assert!(
        !violations(&document).contains(&"accounting.mutants.accepted".to_owned()),
        "a mutation nothing reached raises the same finding as one every reaching test passed, \
         so it is a mutation a reviewer can answer for: {:?}",
        violations(&document)
    );
}

#[test]
fn more_acceptances_than_there_are_mutations_to_accept_is_a_violation() {
    let document = with(serde_json::json!({
        "accounting": {
            "mutants": { "executed": 1, "survived": 0, "unreached": 1, "accepted": 2 }
        },
        "mutants": [{}, { "outcome": "unreached" }],
        "findings": []
    }));

    assert!(
        violations(&document).contains(&"accounting.mutants.accepted".to_owned()),
        "an acceptance answers for a mutation this run recorded, and there are not that many"
    );
}

#[test]
fn a_column_the_recording_omits_is_unaudited_rather_than_a_pass_or_a_failure() {
    let mut document = base();
    without(&mut document, "mutants", "killed");
    assert_eq!(violations(&document), Vec::<String>::new());
    assert!(
        unaudited(&document).contains(&"accounting.mutants.killed".to_owned()),
        "fail-closed is never turning what cannot be checked into what is fine, and never into what is broken"
    );
}

#[test]
fn an_assurance_that_carries_a_finding_is_a_violation() {
    let mut document = assured();
    merge(
        &mut document,
        serde_json::json!({ "findings": [{ "kind": "surviving-mutant", "subject": KILLED, "detail": "d", "position": null }] }),
    );
    assert!(violations(&document).contains(&"verdict".to_owned()));
}

#[test]
fn an_assurance_wider_than_the_scope_the_run_looked_at_is_a_violation() {
    let mut document = assured();
    merge(&mut document, serde_json::json!({ "run_kind": "scoped" }));
    assert!(
        violations(&document).contains(&"verdict".to_owned()),
        "a scoped run assures only what it looked at"
    );
}

#[test]
fn an_assurance_over_a_target_that_failed_is_a_violation() {
    let mut document = assured();
    merge(
        &mut document,
        serde_json::json!({
            "accounting": { "targets": { "passed": 0, "failed": 1 } },
            "targets": [{
                "id": "3f2a1b0c9d8e7f60",
                "name": TARGET,
                "package": "pkg",
                "status": "failed",
                "duration_ms": 5,
                "message": null
            }]
        }),
    );
    assert!(violations(&document).contains(&"verdict".to_owned()));
}

#[test]
fn a_defect_that_names_nothing_a_reader_can_act_on_is_a_violation() {
    let mut document = with(serde_json::json!({ "verdict": "DEFECT" }));
    assert!(
        violations(&document).contains(&"verdict".to_owned()),
        "a surviving mutant is a gap in the tests, not a fault in the code"
    );
    merge(
        &mut document,
        serde_json::json!({ "findings": [{ "kind": "failing-test", "subject": TARGET, "detail": "d", "position": null }] }),
    );
    assert!(!violations(&document).contains(&"verdict".to_owned()));
}

#[test]
fn a_kill_that_names_no_target_at_all_is_a_violation() {
    assert_eq!(
        violations(&with(
            serde_json::json!({ "mutants": [{ "killed_by": null }] })
        )),
        [KILLED],
        "a kill nobody can name is not a kill a reader can check"
    );
}

#[test]
fn a_kill_attributed_to_a_target_the_recording_does_not_carry_is_a_violation() {
    assert_eq!(
        violations(&with(
            serde_json::json!({ "mutants": [{ "killed_by": "pkg/test/lib somebody_else" }] })
        )),
        [KILLED]
    );
}

#[test]
fn a_kill_attributed_to_a_target_that_never_passed_is_a_violation() {
    let audit = audited(&with(serde_json::json!({
        "accounting": { "targets": { "passed": 0, "skipped": 1 } },
        "targets": [{ "status": "skipped" }]
    })));
    let killers: Vec<&str> = audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Killers)
        .map(|remark| remark.subject.as_str())
        .collect();
    assert_eq!(
        killers,
        [KILLED],
        "a target the run never saw pass on the original tree cannot tell the two programs apart"
    );
}

#[test]
fn a_survivor_no_finding_names_is_a_violation() {
    assert_eq!(
        violations(&with(serde_json::json!({ "findings": [] }))),
        [SURVIVED],
        "a mutation nothing noticed is a gap in the tests, and a report that hides it claims more than it holds"
    );
}

#[test]
fn a_surviving_mutant_finding_that_names_no_survivor_is_a_violation() {
    assert_eq!(
        violations(&with(serde_json::json!({
            "findings": [{ "kind": "surviving-mutant", "subject": "cccccccccccccccccccc", "detail": "d", "position": null }]
        }))),
        ["bbbbbbbbbbbbbbbbbbbb", "cccccccccccccccccccc"]
    );
}

#[test]
fn a_survivor_no_finding_names_is_unaudited_where_the_recording_counts_an_acceptance() {
    let document = with(serde_json::json!({
        "findings": [],
        "accounting": { "mutants": { "accepted": 1 } }
    }));
    assert_eq!(violations(&document), Vec::<String>::new());
    assert!(
        unaudited(&document).contains(&SURVIVED.to_owned()),
        "the recording counts acceptances without naming them, so which survivor was accepted cannot be re-decided"
    );
}

#[test]
fn a_reused_disposition_that_names_no_source_run_is_a_violation() {
    assert_eq!(
        violations(&with(serde_json::json!({
            "mutants": [{ "reused": true }],
            "accounting": { "mutants": { "reused_killed": 1 } }
        }))),
        [KILLED],
        "a verdict read back from a run a reader cannot name is a verdict taken on trust"
    );
}

#[test]
fn a_reused_disposition_that_names_this_run_itself_is_a_violation() {
    assert_eq!(
        violations(&with(serde_json::json!({
            "mutants": [{ "reused": true, "source_run_id": RUN }],
            "accounting": { "mutants": { "reused_killed": 1 } }
        }))),
        [KILLED],
        "a run cannot have read its own answer back"
    );
}

#[test]
fn a_source_run_on_a_disposition_this_run_established_is_a_violation() {
    assert_eq!(
        violations(&with(serde_json::json!({
            "mutants": [{ "reused": false, "source_run_id": EARLIER }]
        }))),
        [KILLED]
    );
}

#[test]
fn what_a_reused_disposition_rests_on_is_unaudited() {
    let document = with(serde_json::json!({
        "mutants": [{ "reused": true, "source_run_id": EARLIER }],
        "accounting": { "mutants": { "reused_killed": 1 } }
    }));
    assert_eq!(violations(&document), Vec::<String>::new());
    assert!(
        unaudited(&document).contains(&"provenance".to_owned()),
        "whether the recorded killer is still routed under the same behaviour key is not in the report"
    );
}

#[test]
fn a_run_directory_with_no_report_in_it_cannot_be_audited() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let error = gates::proofaudit(directory.path(), None).expect_err("nothing to re-decide");
    assert!(matches!(error, AuditError::Unreadable { .. }), "{error}");
    assert!(error.to_string().contains(REPORT), "{error}");
}

#[test]
fn a_report_that_is_not_json_cannot_be_audited() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(directory.path().join(REPORT), "not json").expect("the recording");
    let error = gates::proofaudit(directory.path(), None).expect_err("nothing to re-decide");
    assert!(matches!(error, AuditError::Unparsable { .. }), "{error}");
}

#[test]
fn a_document_of_another_schema_cannot_be_audited() {
    let document = with(serde_json::json!({ "schema": "mjutest-trace-v1" }));
    let directory = run_directory(&document);
    let error = gates::proofaudit(directory.path(), None).expect_err("nothing to re-decide");
    assert!(matches!(error, AuditError::Unrecognised { .. }), "{error}");
}

#[test]
fn the_summary_line_says_what_was_re_decided_and_what_it_found() {
    let rendered = audited_with_routes(&base()).to_string();
    assert!(
        rendered.ends_with(&format!(
            "proofaudit: {RUN}: 2 mutants and 1 target re-decided; 0 violations, 1 unaudited"
        )),
        "{rendered}"
    );
}

#[test]
fn every_violation_is_a_line_of_its_own_before_the_summary() {
    let rendered = audited_with_routes(&with(serde_json::json!({ "findings": [] }))).to_string();
    let mut lines = rendered.lines();
    let first = lines.next().unwrap_or_default();
    assert!(first.starts_with("violation: findings: "), "{rendered}");
    assert!(first.contains(SURVIVED), "{rendered}");
    assert!(rendered.ends_with("1 violation, 1 unaudited"), "{rendered}");
}

#[test]
fn a_clean_recording_exits_zero_and_one_with_a_violation_in_it() {
    assert_eq!(exit_code(run_directory(&base()).path()), 0);
    assert_eq!(
        exit_code(run_directory(&with(serde_json::json!({ "findings": [] }))).path()),
        1
    );
}

#[test]
fn a_run_directory_that_could_not_be_read_exits_apart_from_both() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    assert_eq!(exit_code(directory.path()), i32::from(EXIT_UNREADABLE));
    assert_eq!(
        EXIT_UNREADABLE, 2,
        "\"I could not look\" is neither \"I looked and found nothing\" nor \"I looked and found something\""
    );
}

#[test]
fn a_mutation_the_route_calls_unreached_and_the_recording_runs_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[
            serde_json::json!({
                "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
                "route": {
                    "mutant": KILLED, "granularity": "unreached", "fallback": null,
                    "reaching": [], "discharged": [], "file_candidates": 0, "reused": null
                }
            }),
            serde_json::json!({
                "seq": 2, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1, "type": "mutant-exec",
                "mutant": {
                    "mutant": KILLED, "target": "t1", "args": [], "outcome": "killed",
                    "duration_ms": 5
                }
            }),
        ],
    );

    assert!(
        proven(&audit).contains(&KILLED.to_owned()),
        "no measured test reaches this is a claim about the code, and the run that made it \
         then ran a test against it: {audit}"
    );
}

#[test]
fn a_mutation_the_route_sends_to_the_suite_and_the_recording_never_runs_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[serde_json::json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
            "route": {
                "mutant": SURVIVED, "granularity": "suite", "fallback": "coverage-incomplete",
                "reaching": [], "discharged": [], "file_candidates": 0, "reused": null
            }
        })],
    );

    assert!(
        proven(&audit).contains(&SURVIVED.to_owned()),
        "a premise that fails has to end in more work rather than in less, and this one \
         ended in none: {audit}"
    );
}

#[test]
fn a_route_the_run_read_back_from_an_earlier_one_is_not_held_to_running_anything() {
    let audit = audited_with(
        &base(),
        &[serde_json::json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
            "route": {
                "mutant": SURVIVED, "granularity": "suite", "fallback": "outside-blocks",
                "reaching": [], "discharged": [], "file_candidates": 0, "reused": "earlier"
            }
        })],
    );

    assert!(
        proven(&audit).is_empty(),
        "an answer read back from an earlier run is an answer this run did not have to \
         establish again: {audit}"
    );
}

/// Every mutant the proof layers say a recording does not support.
fn proven(audit: &Audit) -> Vec<String> {
    audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Proofs && remark.standing == Standing::Violated)
        .map(|remark| remark.subject.clone())
        .collect()
}

#[test]
fn the_regions_a_route_was_decided_from_are_not_in_the_recording_and_the_audit_says_so() {
    let audit = audited_with_routes(&base());

    assert!(
        audit.remarks.iter().any(|remark| {
            remark.layer == Layer::Proofs
                && remark.standing == Standing::Unaudited
                && remark.subject == "reach"
        }),
        "the audit re-derives what it can and names what it cannot: which regions a \
         target executed is in the coverage profiles, and a completed run does not keep \
         them: {audit}"
    );
    assert_eq!(
        audit.violations(),
        0,
        "and not being able to check something is not finding something wrong: {audit}"
    );
}
