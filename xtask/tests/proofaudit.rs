// SPDX-FileCopyrightText: 2026 njutest contributors
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
const TARGET: &str = "pkg/test/lib";
const REPORT: &str = "njutest-assurance-report-v1.json";

fn base() -> serde_json::Value {
    serde_json::json!({
        "schema": "njutest-assurance-report-v1",
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
                "equivalent": 0,
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
                "reaching": ["t1"], "discharged": [], "considered": [], "reused": null
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
                    "considered": [], "reused": null
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
    let document = with(serde_json::json!({ "schema": "njutest-trace-v1" }));
    let directory = run_directory(&document);
    let error = gates::proofaudit(directory.path(), None).expect_err("nothing to re-decide");
    assert!(matches!(error, AuditError::Unrecognised { .. }), "{error}");
}

#[test]
fn the_summary_line_says_what_was_re_decided_and_what_it_found() {
    let rendered = audited_with_routes(&base()).to_string();
    assert!(
        rendered.ends_with(&format!(
            "proofaudit: {RUN}: 2 mutants and 1 target re-decided; 0 violations, 0 unaudited"
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
    assert!(rendered.ends_with("1 violation, 0 unaudited"), "{rendered}");
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
                    "reaching": [], "discharged": [], "considered": [TARGET], "reused": null
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
fn a_mutation_the_route_widened_to_everything_and_never_ran_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[serde_json::json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
            "route": {
                "mutant": SURVIVED, "granularity": "all", "fallback": "coverage-incomplete",
                "reaching": ["t1"], "discharged": [], "considered": [], "reused": null
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
                "mutant": SURVIVED, "granularity": "all", "fallback": "outside-blocks",
                "reaching": ["t1"], "discharged": [], "considered": [], "reused": "earlier"
            }
        })],
    );

    assert!(
        proven(&audit).is_empty(),
        "an answer read back from an earlier run is an answer this run did not have to \
         establish again: {audit}"
    );
}

#[test]
fn a_route_that_removed_every_execution_and_named_nobody_cannot_be_audited_at_all() {
    let audit = audited_with(
        &base(),
        &[serde_json::json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
            "route": {
                "mutant": SURVIVED, "granularity": "unreached", "fallback": null,
                "reaching": [], "discharged": [], "considered": [], "reused": null
            }
        })],
    );

    assert!(
        proven(&audit).contains(&SURVIVED.to_owned()),
        "nothing reaches a place only if somebody was in a position to notice and did \
         not, and a route that names nobody leaves an audit the word and no way to \
         check it: {audit}"
    );
}

#[test]
fn a_route_that_says_a_target_did_not_reach_a_mutation_it_also_kept_it_for_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[serde_json::json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
            "route": {
                "mutant": SURVIVED, "granularity": "block", "fallback": null,
                "reaching": [TARGET], "discharged": [], "considered": [TARGET], "reused": null
            }
        })],
    );

    assert!(
        proven(&audit).contains(&SURVIVED.to_owned()),
        "one route cannot answer one question two ways: {audit}"
    );
}

#[test]
fn a_route_held_to_a_target_the_run_does_not_report_is_held_to_nothing() {
    let audit = audited_with(
        &base(),
        &[serde_json::json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
            "route": {
                "mutant": SURVIVED, "granularity": "unreached", "fallback": null,
                "reaching": [], "discharged": [],
                "considered": ["a target no run of this workspace has"], "reused": null
            }
        })],
    );

    assert!(
        proven(&audit).contains(&SURVIVED.to_owned()),
        "a layer re-derived against targets that are not there is re-derived against \
         nothing: {audit}"
    );
}

#[test]
fn a_route_that_removed_a_target_with_a_proof_and_says_it_reached_nothing_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[serde_json::json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
            "route": {
                "mutant": SURVIVED, "granularity": "unreached", "fallback": null,
                "reaching": [],
                "discharged": [{ "target": TARGET, "proof": "never-infected" }],
                "considered": [TARGET], "reused": null
            }
        })],
    );

    assert!(
        proven(&audit).contains(&SURVIVED.to_owned()),
        "a target that reaches nothing needs no proof to remove it, and a proof that \
         removed it says it did reach: one route claiming both is claiming the layer \
         did work the reach layer had already done, and an audit that lets that pass \
         cannot tell which layer paid for what: {audit}"
    );
}

#[test]
fn a_kill_by_a_target_the_route_never_named_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[
            serde_json::json!({
                "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
                "route": {
                    "mutant": KILLED, "granularity": "block", "fallback": null,
                    "reaching": ["somewhere/else"], "discharged": [], "considered": [],
                    "reused": null
                }
            }),
            serde_json::json!({
                "seq": 2, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1, "type": "mutant-exec",
                "mutant": {
                    "mutant": KILLED, "target": TARGET, "args": [], "outcome": "killed",
                    "duration_ms": 5
                }
            }),
        ],
    );

    assert!(
        proven(&audit).contains(&KILLED.to_owned()),
        "the reach layer removed this target without running it, and the recording then \
         shows it killing the mutation: a layer that drops a target which finds a defect \
         is unsound whether it dropped it for a proof or for a measurement: {audit}"
    );
}

#[test]
fn a_route_that_kept_nothing_and_ran_something_is_one_violation_and_not_two() {
    let audit = audited_with(
        &base(),
        &[
            serde_json::json!({
                "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
                "route": {
                    "mutant": KILLED, "granularity": "unreached", "fallback": null,
                    "reaching": [], "discharged": [], "considered": [TARGET], "reused": null
                }
            }),
            serde_json::json!({
                "seq": 2, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1, "type": "mutant-exec",
                "mutant": {
                    "mutant": KILLED, "target": TARGET, "args": [], "outcome": "killed",
                    "duration_ms": 5
                }
            }),
        ],
    );

    assert_eq!(
        proven(&audit),
        vec![KILLED.to_owned()],
        "a route that kept nothing is the reach layer's own claim and is answered there; \
         saying the same fact twice in two sentences makes a reader look for two \
         defects: {audit}"
    );
    assert_eq!(audit.violations(), 1, "{audit}");
}

/// A report whose every column disagrees with the records it summarises.
fn disagreeing() -> serde_json::Value {
    with(serde_json::json!({
        "accounting": {
            "targets": { "selected": 9, "passed": 9, "failed": 9, "skipped": 9, "missing": 9 },
            "mutants": {
                "cataloged": 9, "rejected": 9, "executed": 9, "killed": 9, "survived": 9,
                "timed_out": 9, "unreached": 9, "equivalent": 9, "accepted": 9,
                "reused_killed": 9, "reused_survived": 9
            },
            "soundness": { "unsafe_items": 0, "packages_with_unsafe": 0, "executed": false }
        },
        "findings": []
    }))
}

/// A report that leaves columns out, so each of them is unaudited rather than re-decided.
fn omitting() -> serde_json::Value {
    let mut document = base();
    for (group, column) in [
        ("mutants", "killed"),
        ("mutants", "cataloged"),
        ("mutants", "executed"),
        ("mutants", "accepted"),
        ("targets", "selected"),
        ("targets", "passed"),
    ] {
        without(&mut document, group, column);
    }
    document
}

/// One route event, so a recording can be built out of the decisions it holds.
fn routed(mutant: &str, granularity: &str, route: &serde_json::Value) -> serde_json::Value {
    let mut record = serde_json::json!({
        "mutant": mutant, "granularity": granularity, "fallback": null,
        "reaching": [], "discharged": [], "considered": [], "reused": null
    });
    if let (Some(into), Some(from)) = (record.as_object_mut(), route.as_object()) {
        for (key, value) in from {
            let _replaced = into.insert(key.clone(), value.clone());
        }
    }
    serde_json::json!({
        "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0,
        "type": "route", "route": record
    })
}

/// One execution event.
fn executed(mutant: &str, target: &str, outcome: &str) -> serde_json::Value {
    at(2, mutant, target, outcome)
}

/// One execution event, numbered, so a recording can hold more than one.
fn at(seq: u64, mutant: &str, target: &str, outcome: &str) -> serde_json::Value {
    serde_json::json!({
        "seq": seq, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1, "type": "mutant-exec",
        "mutant": { "mutant": mutant, "target": target, "args": [], "outcome": outcome,
                    "duration_ms": 5 }
    })
}

/// A second target this run recorded, so a route can keep one and be killed by the other.
const OTHER: &str = "pkg/test/beside";

/// Every audit this test file can provoke, so a property over remarks is a property over the sentences the audit can write.
fn provoked() -> Vec<(&'static str, Audit)> {
    let mut all = documents();
    all.extend(recordings());
    all
}

/// Every audit a report alone can provoke.
fn documents() -> Vec<(&'static str, Audit)> {
    let unnamed = with(serde_json::json!({
        "mutants": [{
            "id": "c".repeat(64), "display_id": "cccccccccccccccccccc", "path": "src/lib.rs",
            "position": { "line": 1, "column": 1, "character_column": 1 },
            "rule": "r@1", "outcome": "killed", "killed_by": null, "reused": false,
            "source_run_id": null
        }]
    }));
    vec![
        ("columns that disagree", audited_with_routes(&disagreeing())),
        (
            "columns that are not there",
            audited_with_routes(&omitting()),
        ),
        ("a report that holds together", audited_with_routes(&base())),
        (
            "findings that name no survivor",
            audited_with_routes(&with(serde_json::json!({ "findings": [] }))),
        ),
        ("a kill that names no target", audited(&unnamed)),
        (
            "an assurance that carries a finding",
            audited(&with(serde_json::json!({ "verdict": "ASSURED" }))),
        ),
    ]
}

/// Every audit a report and a recording together can provoke.
fn recordings() -> Vec<(&'static str, Audit)> {
    let mut all = routing();
    all.extend(concluding());
    all
}

/// Every audit a routing recording can provoke.
fn routing() -> Vec<(&'static str, Audit)> {
    vec![
        (
            "a route that reaches nothing and runs something",
            audited_with(
                &base(),
                &[
                    routed(
                        KILLED,
                        "unreached",
                        &serde_json::json!({ "considered": [TARGET] }),
                    ),
                    executed(KILLED, TARGET, "killed"),
                ],
            ),
        ),
        (
            "a route that names nobody",
            audited_with(
                &base(),
                &[routed(SURVIVED, "unreached", &serde_json::json!({}))],
            ),
        ),
        (
            "a route widened to everything that ran nothing",
            audited_with(
                &base(),
                &[routed(
                    SURVIVED,
                    "all",
                    &serde_json::json!({ "fallback": "not-measured", "reaching": [TARGET] }),
                )],
            ),
        ),
        (
            "a kill by a target a proof removed",
            audited_with(
                &base(),
                &[
                    routed(
                        KILLED,
                        "block",
                        &serde_json::json!({
                            "reaching": ["elsewhere"],
                            "discharged": [{ "target": TARGET, "proof": "never-infected" }]
                        }),
                    ),
                    executed(KILLED, TARGET, "killed"),
                ],
            ),
        ),
        (
            "a kill by a target the route never named",
            audited_with(
                &base(),
                &[
                    routed(
                        KILLED,
                        "block",
                        &serde_json::json!({ "reaching": ["elsewhere"] }),
                    ),
                    executed(KILLED, TARGET, "killed"),
                ],
            ),
        ),
    ]
}

/// Every audit a report's own conclusions can provoke.
fn concluding() -> Vec<(&'static str, Audit)> {
    vec![
        (
            "a route held to a target that is not there",
            audited_with(
                &base(),
                &[routed(
                    SURVIVED,
                    "unreached",
                    &serde_json::json!({ "considered": ["a target no run of this has"] }),
                )],
            ),
        ),
        ("no recording at all", audited(&base())),
        (
            "a survivor no finding names",
            audited(&with(serde_json::json!({ "findings": [] }))),
        ),
        (
            "a disposition read back from nowhere",
            audited(&with(serde_json::json!({
                "mutants": [{
                    "id": "d".repeat(64), "display_id": "dddddddddddddddddddd",
                    "path": "src/lib.rs",
                    "position": { "line": 1, "column": 1, "character_column": 1 },
                    "rule": "r@1", "outcome": "killed", "killed_by": TARGET,
                    "reused": true, "source_run_id": null
                }]
            }))),
        ),
        (
            "a disposition read back from this run",
            audited(&with(serde_json::json!({
                "mutants": [{
                    "id": "e".repeat(64), "display_id": "eeeeeeeeeeeeeeeeeeee",
                    "path": "src/lib.rs",
                    "position": { "line": 1, "column": 1, "character_column": 1 },
                    "rule": "r@1", "outcome": "killed", "killed_by": TARGET,
                    "reused": true, "source_run_id": RUN
                }]
            }))),
        ),
        (
            "an assurance over a target that failed",
            audited(&with(serde_json::json!({
                "verdict": "ASSURED", "findings": [],
                "accounting": { "targets": { "failed": 1 } }
            }))),
        ),
        (
            "an assurance over nothing observed",
            audited(&with(serde_json::json!({
                "verdict": "ASSURED", "findings": [],
                "accounting": { "targets": { "passed": 0 } }
            }))),
        ),
        (
            "a defect that names nothing wrong",
            audited(&with(serde_json::json!({ "verdict": "DEFECT" }))),
        ),
    ]
}

#[test]
fn every_remark_a_recording_earns_says_something_a_reader_can_act_on() {
    let mut seen = 0usize;
    for (what, audit) in provoked() {
        for remark in &audit.remarks {
            seen = seen.saturating_add(1);
            let said = remark.detail.trim();
            assert!(
                !remark.subject.trim().is_empty(),
                "{what}: a remark that names nothing is one a reader cannot look up: \
                 {remark:?}"
            );
            assert!(
                said.len() > 20,
                "{what}: and one that says nothing is one they cannot act on. A sentence \
                 is the whole of what an audit produces, so an empty one is the audit \
                 failing quietly: {remark:?}"
            );
            assert!(
                said != remark.subject.trim(),
                "{what}: saying the subject back is not saying anything: {remark:?}"
            );
            assert!(
                !said.contains("  "),
                "{what}: two spaces are where a sentence was built around something that \
                 turned out to be empty, and the reader gets the frame without the fact: \
                 {remark:?}"
            );
            assert!(
                !said.ends_with(':') && !said.ends_with(';'),
                "{what}: a sentence that ends where its reason should start is a sentence \
                 that promised one: {remark:?}"
            );
        }
    }
    assert!(
        seen >= 30,
        "the recordings between them have to reach the sentences this audit can write, \
         and {seen} is too few to have done that"
    );
}

#[test]
fn every_column_the_report_carries_is_re_decided_and_not_a_subset_of_them() {
    let audit = audited_with_routes(&disagreeing());
    let named: Vec<&str> = audit
        .remarks
        .iter()
        .map(|remark| remark.subject.as_str())
        .collect();

    for column in [
        "accounting.targets.selected",
        "accounting.targets.passed",
        "accounting.targets.failed",
        "accounting.targets.skipped",
        "accounting.targets.missing",
        "accounting.mutants.cataloged",
        "accounting.mutants.executed",
        "accounting.mutants.killed",
        "accounting.mutants.survived",
        "accounting.mutants.unreached",
        "accounting.mutants.rejected",
        "accounting.mutants.timed_out",
        "accounting.mutants.equivalent",
        "accounting.mutants.reused_killed",
        "accounting.mutants.reused_survived",
    ] {
        assert!(
            named.contains(&column),
            "{column} disagrees with the records it summarises and the audit says nothing \
             about it; a column nothing re-decides is a number a report may say anything \
             it likes in: {named:?}"
        );
    }
}

#[test]
fn a_route_the_audit_passes_over_does_not_stop_it_looking_at_the_rest() {
    let audit = audited_with(
        &base(),
        &[
            serde_json::json!({
                "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
                "route": {
                    "mutant": "an earlier answer", "granularity": "all",
                    "fallback": "not-measured", "reaching": [TARGET], "discharged": [],
                    "considered": [], "reused": "an earlier run"
                }
            }),
            serde_json::json!({
                "seq": 2, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1, "type": "route",
                "route": {
                    "mutant": SURVIVED, "granularity": "unreached", "fallback": null,
                    "reaching": [], "discharged": [], "considered": [], "reused": null
                }
            }),
        ],
    );

    assert!(
        proven(&audit).contains(&SURVIVED.to_owned()),
        "the route before it was read back from an earlier run and is passed over; a \
         loop that stopped there instead of skipping it would report nothing about \
         everything after it: {audit}"
    );
}

#[test]
fn one_fact_said_twice_is_one_line() {
    let twice = serde_json::json!({
        "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
        "route": {
            "mutant": SURVIVED, "granularity": "unreached", "fallback": null,
            "reaching": [], "discharged": [], "considered": [], "reused": null
        }
    });
    let mut second = twice.clone();
    if let Some(seq) = second.get_mut("seq") {
        *seq = serde_json::json!(2);
    }
    let audit = audited_with(&base(), &[twice, second]);

    assert_eq!(
        audit.violations(),
        1,
        "one route recorded twice is one fact, and saying it twice makes a reader look \
         for two defects: {audit}"
    );
}

#[test]
fn a_timeout_is_a_target_noticing_and_a_proof_may_not_have_removed_it() {
    let audit = audited_with(
        &base(),
        &[
            routed(
                KILLED,
                "block",
                &serde_json::json!({
                    "reaching": ["elsewhere"],
                    "discharged": [{ "target": TARGET, "proof": "branch-never-taken" }]
                }),
            ),
            executed(KILLED, TARGET, "timed_out"),
        ],
    );

    assert!(
        proven(&audit).contains(&KILLED.to_owned()),
        "a confirmed timeout is a behaviour change the tests noticed, so a proof that \
         removed the target it happened on removed one that found a defect: {audit}"
    );
}

#[test]
fn a_route_that_kept_the_killer_beside_others_removed_nothing_that_found_a_defect() {
    let audit = audited_with(
        &base(),
        &[
            routed(
                KILLED,
                "block",
                &serde_json::json!({ "reaching": [TARGET, "and one more"] }),
            ),
            executed(KILLED, TARGET, "killed"),
        ],
    );

    assert!(
        proven(&audit).is_empty(),
        "the killer is one of the targets this route kept, so the reach layer removed \
         nothing that found anything. Asking whether *every* kept target was the killer \
         would call a route that kept two and was killed by one of them unsound: {audit}"
    );
}

#[test]
fn a_route_that_kept_others_and_was_killed_by_none_of_them_is_a_violation() {
    let audit = audited_with(
        &base(),
        &[
            routed(
                KILLED,
                "block",
                &serde_json::json!({ "reaching": ["elsewhere", "further away"] }),
            ),
            executed(KILLED, TARGET, "killed"),
        ],
    );

    assert!(
        proven(&audit).contains(&KILLED.to_owned()),
        "the killer is among none of the targets this route kept, however many it kept: \
         {audit}"
    );
}

#[test]
fn a_defect_that_names_one_fault_among_findings_that_are_not_faults_is_supported() {
    let named = with(serde_json::json!({
        "verdict": "DEFECT",
        "findings": [
            { "kind": "surviving-mutant", "subject": SURVIVED, "detail": "d", "position": null },
            { "kind": "failing-test", "subject": TARGET, "detail": "it failed", "position": null }
        ]
    }));
    let audit = audited(&named);

    assert!(
        !violations(&named).contains(&"verdict".to_owned()),
        "one fault among the findings is what a DEFECT rests on; asking that *every* \
         finding be a fault would refuse a report that names a fault and a gap: {audit}"
    );
}

#[test]
fn what_a_run_looked_at_decides_which_assurance_it_may_reach() {
    for (kind, assurance) in [
        ("full", "ASSURED"),
        ("changed", "CHANGE_ASSURED"),
        ("scoped", "SCOPE_ASSURED"),
    ] {
        let document = with(serde_json::json!({
            "run_kind": kind, "verdict": assurance, "findings": []
        }));
        let reached = audited(&document);
        assert!(
            !reached
                .remarks
                .iter()
                .any(|remark| remark.subject == "verdict"),
            "a {kind} run may conclude {assurance}, and an audit that cannot say which \
             assurance a run kind reaches has not checked it either: {reached}"
        );
        let wider = with(serde_json::json!({
            "run_kind": kind, "verdict": "ASSURED", "findings": []
        }));
        if assurance != "ASSURED" {
            assert!(
                violations(&wider).contains(&"verdict".to_owned()),
                "and a {kind} run may not conclude ASSURED, which is a claim about code \
                 it did not look at"
            );
        }
    }
}

#[test]
fn a_column_the_audit_could_not_check_is_counted_as_one_it_could_not_check() {
    let mut document = base();
    without(&mut document, "mutants", "killed");
    let audit = audited(&document);

    assert_eq!(
        audit.unaudited(),
        4,
        "one column that is not there leaves the column itself, the two equations it is \
         a side of, and the routing this recording does not carry. A count of what could \
         not be checked is what tells a reader how much of the report the audit is \
         silent about, and one that is always zero says it checked everything: {audit}"
    );
    assert!(
        audit.to_string().contains("4 unaudited"),
        "and the summary says it: {audit}"
    );
}

#[test]
fn a_field_the_recording_does_not_carry_is_absent_rather_than_fatal() {
    let mut document = base();
    if let Some(object) = document.as_object_mut() {
        let _removed = object.remove("run_id");
    }
    let audit = audited(&document);

    assert!(
        audit.run_id.is_empty(),
        "a document missing a field it should have is a document the audit reads what it \
         can of; reaching into it and unwrapping would end the audit at the first thing \
         that was not there: {audit}"
    );
}

#[test]
fn an_execution_the_audit_passes_over_does_not_stop_it_looking_at_the_rest() {
    let audit = audited_with(
        &base(),
        &[
            routed(
                KILLED,
                "block",
                &serde_json::json!({
                    "reaching": ["elsewhere"],
                    "discharged": [{ "target": TARGET, "proof": "never-infected" }]
                }),
            ),
            at(2, KILLED, "something else", "survived"),
            at(3, KILLED, TARGET, "killed"),
        ],
    );

    assert!(
        proven(&audit).contains(&KILLED.to_owned()),
        "the execution before it said nothing and is passed over; a loop that stopped \
         there would report nothing about everything after it: {audit}"
    );
}

#[test]
fn an_execution_by_a_target_no_proof_removed_does_not_stop_the_search_for_one_it_did() {
    let audit = audited_with(
        &base(),
        &[
            routed(
                KILLED,
                "block",
                &serde_json::json!({
                    "reaching": [OTHER],
                    "discharged": [{ "target": TARGET, "proof": "branch-never-taken" }]
                }),
            ),
            at(2, KILLED, OTHER, "killed"),
            at(3, KILLED, TARGET, "killed"),
        ],
    );

    assert!(
        proven(&audit).contains(&KILLED.to_owned()),
        "the first killer is one the route kept and no proof removed, so it is passed \
         over; the second is one a proof removed and then killed with: {audit}"
    );
}

#[test]
fn an_execution_by_a_target_the_route_kept_does_not_stop_the_search_for_one_it_dropped() {
    let audit = audited_with(
        &base(),
        &[
            routed(KILLED, "block", &serde_json::json!({ "reaching": [OTHER] })),
            at(2, KILLED, OTHER, "killed"),
            at(3, KILLED, TARGET, "killed"),
        ],
    );

    assert!(
        proven(&audit).contains(&KILLED.to_owned()),
        "the first killer is one the route kept and is passed over; the second is one \
         the reach layer removed: {audit}"
    );
}

#[test]
fn an_execution_a_proof_answers_for_does_not_stop_the_search_for_one_nothing_answers_for() {
    let audit = audited_with(
        &base(),
        &[
            routed(
                KILLED,
                "block",
                &serde_json::json!({
                    "reaching": [OTHER],
                    "discharged": [{ "target": TARGET, "proof": "never-infected" }]
                }),
            ),
            at(2, KILLED, TARGET, "killed"),
            at(3, KILLED, "a third target", "killed"),
        ],
    );

    let said: Vec<&str> = audit
        .remarks
        .iter()
        .filter(|remark| remark.standing == Standing::Violated && remark.layer == Layer::Proofs)
        .map(|remark| remark.detail.as_str())
        .collect();
    assert!(
        said.iter().any(|detail| detail.contains("a third target")),
        "the first killer is one a proof removed, which the proof layer answers for, so \
         the reach check passes over it; the second is one nothing named at all, and a \
         loop that stopped at the first would never reach it: {said:?}"
    );
}

#[test]
fn two_kills_that_name_no_target_are_both_reported() {
    let mut document = base();
    if let Some(mutants) = document
        .get_mut("mutants")
        .and_then(serde_json::Value::as_array_mut)
    {
        for mutant in mutants.iter_mut() {
            mutant["outcome"] = serde_json::json!("killed");
            mutant["killed_by"] = serde_json::Value::Null;
        }
    }
    let audit = audited(&document);
    let named: std::collections::BTreeSet<String> = audit
        .remarks
        .iter()
        .filter(|remark| remark.standing == Standing::Violated && remark.layer == Layer::Killers)
        .map(|remark| remark.subject.clone())
        .collect();

    assert!(
        named.len() >= 2,
        "a reader works through the list an audit hands them; one that stops at the \
         first kill it cannot check sends them back for the rest: {audit}"
    );
}

#[test]
fn a_survivor_a_finding_names_does_not_stop_the_search_for_one_it_does_not() {
    let mut document = with(serde_json::json!({
        "accounting": { "mutants": { "killed": 0, "survived": 2, "executed": 2 } }
    }));
    if let Some(mutants) = document
        .get_mut("mutants")
        .and_then(serde_json::Value::as_array_mut)
    {
        for mutant in mutants.iter_mut() {
            mutant["outcome"] = serde_json::json!("survived");
            mutant["killed_by"] = serde_json::Value::Null;
        }
    }
    let named = document
        .get("mutants")
        .and_then(|held| held.get(0))
        .and_then(|held| held.get("display_id"))
        .cloned()
        .expect("a mutant to name");
    if let Some(findings) = document.get_mut("findings") {
        *findings = serde_json::json!([
            { "kind": "surviving-mutant", "subject": named, "detail": "d", "position": null }
        ]);
    }
    let audit = audited(&document);

    let unnamed = audit
        .remarks
        .iter()
        .filter(|remark| remark.standing == Standing::Violated && remark.layer == Layer::Findings)
        .count();
    assert_eq!(
        unnamed, 1,
        "one survivor has a finding and is passed over, and the other has none; a loop \
         that stopped at the first one it could account for would say the report is \
         complete: {audit}"
    );
}

#[test]
fn a_whole_recording_rejects_an_unmatched_finding_for_a_unique_prefix() {
    let document = with(serde_json::json!({
        "findings": [{
            "kind": "unmatched-acceptance",
            "subject": "aaaa",
            "detail": "no single mutant",
            "position": null
        }]
    }));
    let audit = audited(&document);

    assert!(
        audit.remarks.iter().any(|remark| {
            remark.layer == Layer::Acceptances
                && remark.standing == Standing::Violated
                && remark.subject == "aaaa"
        }),
        "the complete catalog uniquely resolves the prefix: {audit}"
    );
}

#[test]
fn invalid_absent_and_ambiguous_acceptances_support_the_unmatched_finding() {
    for subject in ["A", "cccc", "aaaa"] {
        let mut document = with(serde_json::json!({
            "findings": [{
                "kind": "unmatched-acceptance",
                "subject": subject,
                "detail": "no single mutant",
                "position": null
            }]
        }));
        if subject == "aaaa" {
            *document
                .pointer_mut("/mutants/1/id")
                .expect("the second mutant identity") =
                serde_json::json!(format!("aaaa{}", "b".repeat(60)));
        }
        let audit = audited(&document);

        assert!(
            audit
                .remarks
                .iter()
                .all(|remark| remark.layer != Layer::Acceptances),
            "{subject:?} does not resolve to exactly one catalog entry: {audit}"
        );
    }
}

#[test]
fn a_shard_marks_acceptance_resolution_unaudited_until_merge() {
    let document = with(serde_json::json!({
        "scope": { "shard": "1/2" },
        "findings": [{
            "kind": "unmatched-acceptance",
            "subject": "aaaa",
            "detail": "no single mutant",
            "position": null
        }]
    }));
    let audit = audited(&document);

    assert!(
        audit.remarks.iter().any(|remark| {
            remark.layer == Layer::Acceptances
                && remark.standing == Standing::Unaudited
                && remark.subject == "aaaa"
        }),
        "a shard has only its own mutant records: {audit}"
    );
}

/// What the audit said about the verdict of `document`, in its own words.
fn about_the_verdict(document: &serde_json::Value) -> Vec<String> {
    audited(document)
        .remarks
        .iter()
        .filter(|remark| remark.subject == "verdict")
        .map(|remark| remark.detail.clone())
        .collect()
}

#[test]
fn an_assurance_over_targets_that_did_not_run_is_a_violation_however_few_they_are() {
    for name in ["failed", "missing"] {
        let document = with(serde_json::json!({
            "verdict": "ASSURED", "findings": [],
            "accounting": { "targets": { name: 1 } }
        }));
        let said = about_the_verdict(&document);
        assert!(
            said.iter().any(|detail| detail.contains(name)),
            "one {name} target is enough to stop an assurance, and a check that waited \
             for two would let the first through. What it said instead: {said:?}"
        );
    }
}

#[test]
fn an_assurance_over_nothing_observed_or_nothing_asked_is_a_violation() {
    for (group, name) in [("targets", "passed"), ("mutants", "executed")] {
        let document = with(serde_json::json!({
            "verdict": "ASSURED", "findings": [],
            "accounting": { group: { name: 0 } }
        }));
        let said = about_the_verdict(&document);
        assert!(
            said.iter()
                .any(|detail| detail.contains(name) && detail.contains(group)),
            "a run that counts no {name} {group} assures nothing, whatever else it says. \
             What it said instead: {said:?}"
        );
    }
}

#[test]
fn a_row_the_recording_names_is_the_row_the_audit_looks_it_up_by() {
    let mut document = base();
    let named = document
        .get("targets")
        .and_then(|held| held.get(0))
        .and_then(|held| held.get("id"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .expect("a target id");
    if let Some(mutants) = document
        .get_mut("mutants")
        .and_then(serde_json::Value::as_array_mut)
    {
        for mutant in mutants.iter_mut() {
            if mutant.get("outcome").and_then(serde_json::Value::as_str) == Some("killed")
                && let Some(by) = mutant.get_mut("killed_by")
            {
                *by = serde_json::Value::String(named.clone());
            }
        }
    }

    assert_eq!(
        violations(&document),
        Vec::<String>::new(),
        "a kill may name the target by its identity as well as by its name, and an audit \
         that read no identity out of the row would say the target is not there"
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
fn the_reach_layer_is_re_derived_from_the_recording_rather_than_left_unaudited() {
    let audit = audited_with_routes(&base());

    assert!(
        !audit.remarks.iter().any(|remark| {
            remark.layer == Layer::Proofs
                && remark.standing == Standing::Unaudited
                && remark.subject == "reach"
        }),
        "a route names the targets it removed every execution from, so the layer is \
         checked against them rather than declared out of reach: {audit}"
    );
    assert_eq!(
        audit.violations(),
        0,
        "and a recording that holds together holds together: {audit}"
    );
}

#[test]
fn a_route_that_says_an_answer_was_both_read_back_and_refused_is_a_violation() {
    let consulting = |reused: serde_json::Value, refused: serde_json::Value| {
        audited_with(
            &base(),
            &[serde_json::json!({
                "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0, "type": "route",
                "route": {
                    "mutant": SURVIVED, "granularity": "block", "fallback": null,
                    "reaching": [TARGET], "discharged": [], "considered": [],
                    "reused": reused, "refused": refused
                }
            })],
        )
    };
    let run = serde_json::json!("20260906T000000Z-000001");
    let reason = serde_json::json!("key-changed");
    let null = serde_json::Value::Null;

    let both = consulting(run.clone(), reason.clone());
    assert!(
        proven(&both).contains(&SURVIVED.to_owned()),
        "a believed record is an execution that did not happen, so the recording has to \
         say which way reuse went for each mutation; one that says the answer was taken \
         and that it was refused says neither: {both}"
    );

    let taken = consulting(run, null.clone());
    let refused = consulting(null.clone(), reason);
    let asked = consulting(null.clone(), null);
    assert!(
        proven(&taken).is_empty() && proven(&refused).is_empty() && proven(&asked).is_empty(),
        "while a route that names one of the two, or neither because there was no store \
         to ask, is a route that says what happened: {taken}, {refused}, {asked}"
    );
}
