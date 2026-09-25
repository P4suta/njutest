// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gate that re-decides a recorded run without asking the runner whether it agrees with itself.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::Path;

use xtask::gates;
use xtask::proofaudit::sentinel::{
    self, ASKED, KILLED, RUN, SURVIVED, TARGET, base, merge, never_noticed, routes, was_put,
    went_past, with,
};
use xtask::proofaudit::{
    Audit, AuditError, Coverage, EXIT_UNREADABLE, Layer, REPORT_FILE, Standing,
};

const EARLIER: &str = "20260905T090000Z-1a2b3c";

fn run_directory(document: &serde_json::Value) -> tempfile::TempDir {
    sentinel::run_directory(document).expect("a run directory")
}

fn recorded(lines: &[serde_json::Value]) -> tempfile::TempDir {
    sentinel::recorded(lines).expect("a recording")
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

fn without(document: &mut serde_json::Value, group: &str, column: &str) {
    let columns = document
        .get_mut("accounting")
        .and_then(|accounting| accounting.get_mut(group))
        .and_then(serde_json::Value::as_object_mut)
        .expect("the accounting group");
    assert!(
        columns.remove(column).is_some(),
        "the fixture accounting has {group}.{column}"
    );
}

fn audited(document: &serde_json::Value) -> Audit {
    let directory = run_directory(document);
    let concluded = sentinel::concluding(document, &[]);
    if concluded.is_empty() {
        return gates::proofaudit(directory.path(), None).expect("a recording this audit can read");
    }
    let trace = recorded(&concluded);
    gates::proofaudit(directory.path(), Some(trace.path()))
        .expect("a recording this audit can read")
}

fn off_schema(document: &serde_json::Value) -> bool {
    let directory = run_directory(document);
    matches!(
        gates::proofaudit(directory.path(), None),
        Err(AuditError::OffSchema { .. })
    )
}

fn audited_with_routes(document: &serde_json::Value) -> Audit {
    audited_with(document, &routes())
}

fn audited_with(document: &serde_json::Value, lines: &[serde_json::Value]) -> Audit {
    let run = run_directory(document);
    let trace = recorded(&sentinel::concluding(document, lines));
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
    assert_eq!(audit.exit_code(), 0, "{audit}");
}

#[test]
fn every_layer_says_how_far_it_got_even_with_nothing_to_look_at() {
    let said = audited(&base()).to_string();
    for layer in Layer::ALL {
        let heads = said
            .lines()
            .filter(|line| line.starts_with(&format!("layer: {}: ", layer.label())))
            .count();
        assert_eq!(
            heads,
            1,
            "{} says how far it got once:\n{said}",
            layer.label()
        );
    }
}

#[test]
fn an_affirmative_model_outcome_without_its_evidence_is_refused_before_any_layer_reads_it() {
    let mutant = "b".repeat(64);
    let document = with(serde_json::json!({
        "contract": "verified-v1",
        "accounting": {
            "mutants": { "survived": 0, "model_proved": 1 }
        },
        "findings": [],
        "models": [{
            "mutant": mutant,
            "answer": {
                "decision": "proved",
                "evidence": {}
            }
        }],
        "mutants": [
            {},
            { "decision": { "outcome": "model-proved" } }
        ]
    }));
    assert!(
        off_schema(&document),
        "an affirmative model answer without its evidence is not a report the schema allows"
    );
}

#[test]
fn verified_v1_requires_exactly_one_model_record_for_every_test_survivor() {
    let survivor = "b".repeat(64);

    let missing = with(serde_json::json!({ "contract": "verified-v1" }));
    let audit = audited(&missing);
    assert!(
        audit.remarks.iter().any(|remark| {
            remark.layer == Layer::Model
                && remark.standing == Standing::Violated
                && remark
                    .detail
                    .contains("no exactly corresponding model record")
        }),
        "{audit}"
    );

    let duplicate = with(serde_json::json!({
        "contract": "verified-v1",
        "models": [
            {
                "mutant": survivor,
                "answer": { "decision": "ineligible", "reason": "effect" }
            },
            {
                "mutant": survivor,
                "answer": { "decision": "ineligible", "reason": "effect" }
            }
        ]
    }));
    let audit = audited(&duplicate);
    assert!(
        audit.remarks.iter().any(|remark| {
            remark.layer == Layer::Model
                && remark.standing == Standing::Violated
                && remark.detail.contains("unique, non-empty")
        }),
        "{audit}"
    );
}

#[test]
fn verified_v1_refuses_extra_and_open_shaped_model_records_before_any_layer_reads_them() {
    let killed = "a".repeat(64);
    let survivor = "b".repeat(64);
    let document = with(serde_json::json!({
        "contract": "verified-v1",
        "models": [
            {
                "mutant": survivor,
                "answer": { "decision": "ineligible", "reason": "effect" }
            },
            {
                "mutant": killed,
                "answer": {
                    "decision": "ineligible",
                    "reason": "effect",
                    "untrusted": true
                }
            }
        ]
    }));
    assert!(
        off_schema(&document),
        "an open-shaped model record is not a report the schema allows"
    );
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
        "mutants": [{}, { "decision": { "outcome": "unreached" }, "accepted": true }],
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
        "mutants": [{}, { "decision": { "outcome": "unreached" }, "accepted": true }],
        "findings": []
    }));

    assert!(
        violations(&document).contains(&"accounting.mutants.accepted".to_owned()),
        "an acceptance answers for a mutation this run recorded, and there are not that many"
    );
}

#[test]
fn a_column_the_recording_omits_is_refused_before_any_layer_reads_it() {
    let mut document = sentinel::complete_report(&base()).expect("the specimen completes");
    let columns = document
        .pointer_mut("/report/builds/0/parts/0/accounting/mutants")
        .and_then(serde_json::Value::as_object_mut)
        .expect("the mutant accounting");
    assert!(
        columns.remove("killed").is_some(),
        "the specimen has the column"
    );
    assert!(
        off_schema(&document),
        "a report missing a column the schema requires is refused, never read with the column \
         taken as zero or as unknown"
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
fn a_kill_that_names_no_target_at_all_is_refused_before_any_layer_reads_it() {
    assert!(
        off_schema(&with(
            serde_json::json!({ "mutants": [{ "decision": { "killed_by": null } }] })
        )),
        "a kill nobody can name is not a report the published schema allows"
    );
}

#[test]
fn a_kill_attributed_to_a_target_the_recording_does_not_carry_is_a_violation() {
    assert_eq!(
        violations(&with(
            serde_json::json!({ "mutants": [{ "decision": { "killed_by": "pkg/test/lib somebody_else" } }] })
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
fn an_aggregate_acceptance_cannot_hide_a_row_the_report_did_not_accept() {
    let document = with(serde_json::json!({
        "findings": [],
        "accounting": { "mutants": { "accepted": 1 } }
    }));
    assert!(
        violations(&document).contains(&"accounting.mutants.accepted".to_owned())
            && violations(&document).contains(&SURVIVED.to_owned()),
        "an aggregate count cannot answer a different row or replace the required row-local fact"
    );
}

#[test]
fn a_reused_disposition_that_names_no_source_run_is_refused_before_any_layer_reads_it() {
    assert!(
        off_schema(&with(serde_json::json!({
            "mutants": [{ "reuse": { "reused": true } }],
            "accounting": { "mutants": { "reused_killed": 1 } }
        }))),
        "a verdict read back from a run a reader cannot name is not a report the schema allows"
    );
}

#[test]
fn a_reused_disposition_that_names_this_run_itself_is_a_violation() {
    assert_eq!(
        violations(&with(serde_json::json!({
            "mutants": [{ "reuse": { "reused": true, "source_run_id": RUN } }],
            "accounting": { "mutants": { "reused_killed": 1 } }
        }))),
        [KILLED],
        "a run cannot have read its own answer back"
    );
}

#[test]
fn a_source_run_on_a_disposition_this_run_established_is_refused_before_any_layer_reads_it() {
    assert!(off_schema(&with(serde_json::json!({
        "mutants": [{ "reuse": { "reused": false, "source_run_id": EARLIER } }]
    }))));
}

#[test]
fn what_a_reused_disposition_rests_on_is_unaudited() {
    let document = with(serde_json::json!({
        "mutants": [{ "reuse": { "reused": true, "source_run_id": EARLIER } }],
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
    assert!(error.to_string().contains(REPORT_FILE), "{error}");
}

#[test]
fn an_explicit_recording_that_is_missing_cannot_be_audited() {
    let run = run_directory(&base());
    let trace = tempfile::tempdir().expect("a temporary directory");
    let error = gates::proofaudit(run.path(), Some(trace.path()))
        .expect_err("an explicitly requested recording must exist");
    assert!(matches!(error, AuditError::Unreadable { .. }), "{error}");
    assert!(error.to_string().contains("trace.jsonl"), "{error}");
}

#[test]
fn a_corrupt_line_rejects_the_entire_explicit_recording() {
    let run = run_directory(&base());
    let trace = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(trace.path().join("trace.jsonl"), "not json\n").expect("the corrupt recording");
    let error = gates::proofaudit(run.path(), Some(trace.path()))
        .expect_err("corrupt evidence must not become an empty recording");
    assert!(
        matches!(error, AuditError::MalformedRecording { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("line 1"), "{error}");
}

#[test]
fn a_report_that_is_not_json_cannot_be_audited() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(directory.path().join(REPORT_FILE), "not json").expect("the recording");
    let error = gates::proofaudit(directory.path(), None).expect_err("nothing to re-decide");
    assert!(matches!(error, AuditError::Unparsable { .. }), "{error}");
}

#[test]
fn duplicate_report_keys_are_malformed_before_any_redecision() {
    let root = serde_json::to_string(&base())
        .expect("report fixture")
        .replacen(
            "\"schema\":\"njutest-assurance-report-v1\"",
            "\"schema\":\"forged\",\"schema\":\"njutest-assurance-report-v1\"",
            1,
        );

    let nested = serde_json::to_string(&base())
        .expect("report fixture")
        .replacen(
            "\"outcome\":\"killed\"",
            "\"outcome\":\"survived\",\"outcome\":\"killed\"",
            1,
        );

    let with_model = with(serde_json::json!({
        "models": [{
            "mutant": "b".repeat(64),
            "answer": {
                "decision": "proved",
                "evidence": {"verifier": {"tool": "0.68.0"}}
            }
        }]
    }));
    let model = serde_json::to_string(&with_model)
        .expect("model report fixture")
        .replacen(
            "\"tool\":\"0.68.0\"",
            "\"tool\":\"forged\",\"tool\":\"0.68.0\"",
            1,
        );

    for document in [root, nested, model] {
        let nothing = xtask::proofaudit::Recorded {
            runner: None,
            engines: &[],
            outputs: &[],
        };
        let error = xtask::proofaudit::audit_with("duplicate.json", &document, nothing, None)
            .expect_err("duplicate keys never reach proof redecision");
        assert!(matches!(error, AuditError::Unparsable { .. }), "{error}");
        assert!(
            error.to_string().contains("duplicate JSON object key"),
            "{error}"
        );
    }
}

#[test]
fn a_document_of_another_schema_cannot_be_audited() {
    let document = with(serde_json::json!({ "schema": "njutest-trace-v1" }));
    let directory = run_directory(&document);
    let error = gates::proofaudit(directory.path(), None).expect_err("nothing to re-decide");
    assert!(matches!(error, AuditError::OffSchema { .. }), "{error}");
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
    assert_eq!(
        exit_code(run_directory(&base()).path()),
        i32::from(xtask::proofaudit::EXIT_UNAUDITED),
        "read without its recording, a clean run leaves layers unaudited"
    );
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
            serde_json::json!({
                "seq": 3, "timestamp": "2026-09-06T00:00:02Z", "elapsed_ms": 2, "type": "mutant-exec",
                "mutant": {
                    "mutant": SURVIVED, "target": TARGET, "args": [], "outcome": "survived",
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
                "step_limit_reached": 9, "waited": 9, "unreached": 9,
                "equivalent": 9, "accepted": 9,
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
            assert!(
                into.insert(key.clone(), value.clone()).is_some(),
                "the route override names a declared field: {key}"
            );
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
                    "duration_ms": 5,
                    "step_boundary": if outcome == "step_limit_reached" {
                        serde_json::json!({ "limit": 10, "observed": 11 })
                    } else {
                        serde_json::Value::Null
                    } }
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
            "rule": "r@1",
            "decision": { "outcome": "killed", "killed_by": "pkg/test/nowhere", "step_boundary": null },
            "reuse": { "reused": false, "source_run_id": null }
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
            "a disposition read back from this run",
            audited(&with(serde_json::json!({
                "mutants": [{
                    "id": "e".repeat(64), "display_id": "eeeeeeeeeeeeeeeeeeee",
                    "path": "src/lib.rs",
                    "position": { "line": 1, "column": 1, "character_column": 1 },
                    "rule": "r@1",
                    "decision": { "outcome": "killed", "killed_by": TARGET, "step_boundary": null },
                    "reuse": { "reused": true, "source_run_id": RUN }
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
        "accounting.mutants.step_limit_reached",
        "accounting.mutants.waited",
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
fn an_execution_boundary_is_not_a_target_noticing_and_cannot_invalidate_a_proof() {
    for outcome in ["waited", "step_limit_reached"] {
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
                executed(KILLED, TARGET, outcome),
            ],
        );

        assert!(
            !proven(&audit).contains(&KILLED.to_owned()),
            "{outcome} is a non-verdict, so it cannot be converted into a detection merely \
             because a proof removed that target: {audit}"
        );
    }
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
            "run_kind": kind,
            "verdict": assurance,
            "findings": [],
            "accounting": { "mutants": { "accepted": 1 } },
            "mutants": [{}, { "accepted": true }]
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
fn what_the_audit_could_not_check_is_counted_as_what_it_could_not_check() {
    let audit = audited(&base());
    assert_eq!(
        audit.unaudited(),
        4,
        "a run that kept no routing, no execution and no engine recording leaves the proofs, the \
         targets put to mutations, the outcomes held to what ran, and the drift unchecked; a count \
         of what could not be checked is what tells a reader how much of the report the audit is \
         silent about, and one that is always zero says it checked everything: {audit}"
    );
    assert!(
        audit.to_string().contains("4 unaudited"),
        "the summary says so as a number a script can compare: {audit}"
    );
}

#[test]
fn a_field_the_report_does_not_carry_is_a_typed_refusal_rather_than_a_default() {
    let mut document = base();
    if let Some(object) = document.as_object_mut() {
        assert!(
            object.remove("run_id").is_some(),
            "the fixture has a run identifier to remove"
        );
    }
    assert!(
        off_schema(&document),
        "a report missing a field the schema requires is refused, naming where, neither read with \
         the field defaulted nor ending the audit in a panic"
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
    let document = with(serde_json::json!({
        "mutants": [
            { "decision": { "outcome": "killed", "killed_by": "pkg/test/nowhere" } },
            { "decision": { "outcome": "killed", "killed_by": "pkg/test/nowhere" } }
        ]
    }));
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
    let document = with(serde_json::json!({
        "accounting": { "mutants": { "killed": 0, "survived": 2, "executed": 2 } },
        "mutants": [
            { "decision": { "outcome": "survived", "killed_by": null } },
            { "decision": { "outcome": "survived", "killed_by": null } }
        ],
        "findings": [
            { "kind": "surviving-mutant", "subject": KILLED, "detail": "d", "position": null }
        ]
    }));
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

#[test]
fn a_hollow_target_the_report_does_not_name_is_a_violation() {
    let audit = audited_with(&base(), &never_noticed());
    let violated: Vec<String> = audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Hollow && remark.standing == Standing::Violated)
        .map(|remark| remark.subject.clone())
        .collect();
    assert_eq!(
        violated,
        vec!["blunt".to_owned()],
        "the recording says `blunt` was put to two mutations and answered `survived` \
         to both, so the report owes a hollow-target finding about it. A report that \
         is silent there is one this audit exists to refuse: {:?}",
        audit.remarks
    );
}

#[test]
fn a_target_that_noticed_something_is_not_owed_a_hollow_finding() {
    let audit = audited_with(&base(), &routes());
    assert!(
        !audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Hollow && remark.standing == Standing::Violated),
        "`t1` noticed one of the two, so nothing is owed about it: {:?}",
        audit.remarks
    );
}

#[test]
fn a_run_that_kept_no_recording_says_the_hollow_layer_was_not_audited() {
    let audit = audited(&base());
    assert!(
        audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Hollow && remark.standing == Standing::Unaudited),
        "without a recording there is nothing to re-derive from, and saying so is not \
         the same as agreeing: {:?}",
        audit.remarks
    );
}

/// The report with one `wire-unnoticed` finding about `subject`.
fn calling_it_a_gap(subject: &str) -> serde_json::Value {
    let mut document = base();
    document
        .get_mut("findings")
        .and_then(serde_json::Value::as_array_mut)
        .expect("the findings")
        .push(serde_json::json!({
            "kind": "wire-unnoticed",
            "subject": subject,
            "detail": "nothing noticed when the run was told to answer 500",
            "position": null
        }));
    document
}

/// What the wire layer said about a run, by standing.
fn wire_remarks(audit: &Audit, standing: Standing) -> Vec<String> {
    audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Wire && remark.standing == standing)
        .map(|remark| remark.subject.clone())
        .collect()
}

#[test]
fn a_run_that_recorded_a_seam_and_put_nothing_leaves_the_wire_layer_unaudited() {
    let audit = audited_with(&base(), &[went_past()]);
    assert!(
        !wire_remarks(&audit, Standing::Unaudited).is_empty(),
        "six questions were licensed and none was put, so what the suite would have \
         done with them cannot be re-derived, and saying so is not the same as \
         agreeing: {:?}",
        audit.remarks
    );
    assert!(
        wire_remarks(&audit, Standing::Violated).is_empty(),
        "and not putting a question is not a contradiction: {:?}",
        audit.remarks
    );
}

#[test]
fn a_question_nothing_noticed_that_the_report_does_not_name_is_a_violation() {
    let audit = audited_with(&base(), &[went_past(), was_put(ASKED, "unnoticed")]);
    assert!(
        wire_remarks(&audit, Standing::Violated).contains(&ASKED.to_owned()),
        "the recording has the suite carrying on through a 500 on the api seam, so \
         the report owes a wire-unnoticed finding about it: {:?}",
        audit.remarks
    );
}

#[test]
fn a_question_the_tests_noticed_that_the_report_calls_a_gap_is_a_violation() {
    let audit = audited_with(
        &calling_it_a_gap(ASKED),
        &[went_past(), was_put(ASKED, "tests")],
    );
    assert!(
        wire_remarks(&audit, Standing::Violated).contains(&ASKED.to_owned()),
        "a report that calls a question nobody could close a gap, where the recording \
         has a test closing it, is the one answer this product must never give: {:?}",
        audit.remarks
    );
}

/// The exchange above, as the audit reads one.
fn as_read() -> xtask::wire::Exchange {
    xtask::wire::Exchange {
        capability: "api".to_owned(),
        seq: 0,
        wire: "http".to_owned(),
        method: Some("GET".to_owned()),
        path: Some("/orders".to_owned()),
        status: Some(200),
    }
}

/// Every question the exchange licenses, put and decided, with `unnoticed` for `gap`.
fn all_put(gap: &str) -> Vec<serde_json::Value> {
    let mut lines = vec![went_past()];
    for (id, rule) in
        xtask::wire::licensed(&as_read()).expect("the fixture fields fit the identity recipe")
    {
        let decision = if rule == gap { "unnoticed" } else { "tests" };
        lines.push(serde_json::json!({
            "type": "wire-exec",
            "wire": {
                "fault": id,
                "capability": "api",
                "seq": 0,
                "rule": rule,
                "answer": if decision == "tests" {
                    serde_json::json!({ "decision": "tests", "noticed_by": TARGET })
                } else {
                    serde_json::json!({ "decision": decision })
                }
            }
        }));
    }
    lines
}

#[test]
fn a_report_and_a_recording_that_agree_earn_no_remark_at_all() {
    let audit = audited_with(&calling_it_a_gap(ASKED), &all_put("status-server-error"));
    assert!(
        wire_remarks(&audit, Standing::Violated).is_empty(),
        "every question the recording licenses was put, one of them nothing noticed, \
         and the report names that one: {:?}",
        audit.remarks
    );
}

#[test]
fn a_question_the_run_put_and_left_out_of_the_report_is_a_violation_even_among_agreeing_ones() {
    let audit = audited_with(&base(), &all_put("status-server-error"));
    assert_eq!(
        wire_remarks(&audit, Standing::Violated),
        vec![ASKED.to_owned()],
        "five of the six were noticed and the sixth was not, so exactly the sixth is \
         owed a finding: {:?}",
        audit.remarks
    );
}

#[test]
fn a_question_no_exchange_licenses_is_a_violation_however_the_run_decided_it() {
    let audit = audited_with(&base(), &[went_past(), was_put(&"c".repeat(64), "tests")]);
    assert!(
        wire_remarks(&audit, Standing::Violated).contains(&"c".repeat(64)),
        "every question comes from an exchange that happened; one that comes from \
         nowhere is a fault the run invented, and a report resting on it rests on \
         nothing: {:?}",
        audit.remarks
    );
}

/// A recording where `blunt` was put to two mutations and answered `outcome` to both.
fn answering(outcome: &str) -> Vec<serde_json::Value> {
    let mut lines = routes();
    for (seq, mutant) in [(10_u64, KILLED), (11, SURVIVED)] {
        lines.push(serde_json::json!({
            "seq": seq,
            "type": "mutant-exec",
            "mutant": {
                "mutant": mutant,
                "target": "blunt",
                "outcome": outcome,
                "duration_ms": 1
            }
        }));
    }
    lines
}

#[test]
fn a_target_whose_executions_nobody_decided_is_not_one_the_audit_demands_a_finding_about() {
    for outcome in ["errored", "inconclusive"] {
        let audit = audited_with(&base(), &answering(outcome));
        assert!(
            !audit
                .remarks
                .iter()
                .any(|remark| remark.layer == Layer::Hollow
                    && remark.standing == Standing::Violated
                    && remark.subject == "blunt"),
            "a target whose harness would not start did not notice nothing — the run \
             established nothing about it, and an audit that demanded a finding here \
             would demand that a broken harness be called a suite asserting nothing: \
             {:?}",
            audit.remarks
        );
    }
}

#[test]
fn a_part_of_a_catalog_is_not_held_to_whether_a_target_noticed_anything() {
    let mut document = base();
    merge(
        &mut document,
        serde_json::json!({ "scope": { "shard": "1/2" } }),
    );
    let audit = audited_with(&document, &never_noticed());
    assert!(
        audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Hollow
                && remark.standing == Standing::Unaudited
                && remark.detail.contains("when they are merged")),
        "leaving it to the merge is said, not done in silence: {:?}",
        audit.remarks
    );
    assert!(
        !audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Hollow && remark.standing == Standing::Violated),
        "whether a target notices anything is a statement about the whole catalog, \
         and a part has seen a slice: demanding a finding here would demand one the \
         whole would contradict: {:?}",
        audit.remarks
    );
}

/// What the audit makes of `document` beside the routes of the clean run and `engine` as the one engine recording.
fn with_engine(document: serde_json::Value, engine: Vec<serde_json::Value>) -> Audit {
    let laid = sentinel::Perturbation {
        name: "engine",
        document,
        events: Some(routes()),
        engine: Some(engine),
        shards: Vec::new(),
        outputs: Vec::new(),
    }
    .lay()
    .expect("the specimen is laid out");
    gates::proofaudit(laid.run(), laid.trace()).expect("a recording this audit can read")
}

fn drift_violations(audit: &Audit) -> Vec<String> {
    audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Drift && remark.standing == Standing::Violated)
        .map(|remark| format!("{}: {}", remark.subject, remark.detail))
        .collect()
}

#[test]
fn a_control_that_reached_a_site_its_baseline_never_did_is_owed_an_unstable_baseline_finding() {
    let moved = vec![
        sentinel::touch("baseline", &[0]),
        sentinel::touch("control", &[0, 1]),
    ];
    let quiet = with_engine(with(sentinel::drifted("held")), moved.clone());
    let said = drift_violations(&quiet);
    assert!(
        said.iter().any(|line| line.contains("records it as held")),
        "a report that calls a moved target held is refused: {said:?}"
    );
    assert!(
        said.iter()
            .any(|line| line.contains("raises no unstable-baseline finding")),
        "a moved target the report raises nothing about is refused: {said:?}"
    );
    let mut named = with(sentinel::drifted("moved"));
    merge(
        &mut named,
        serde_json::json!({ "findings": [{}, {
            "kind": "unstable-baseline",
            "subject": TARGET,
            "detail": "moved",
            "position": null
        }] }),
    );
    let answered = with_engine(named, moved);
    assert_eq!(
        drift_violations(&answered),
        Vec::<String>::new(),
        "{answered}"
    );
}

#[test]
fn a_finding_about_a_target_whose_reach_held_is_refused() {
    let mut named = with(sentinel::drifted("held"));
    merge(
        &mut named,
        serde_json::json!({ "findings": [{}, {
            "kind": "unstable-baseline",
            "subject": TARGET,
            "detail": "moved",
            "position": null
        }] }),
    );
    let held = vec![
        sentinel::touch("baseline", &[0, 1]),
        sentinel::touch("control", &[0, 1]),
    ];
    let said = drift_violations(&with_engine(named, held));
    assert!(
        said.iter()
            .any(|line| line.contains("do not show its reach moving")),
        "{said:?}"
    );
}

#[test]
fn a_control_over_other_tests_is_no_comparison_and_the_report_must_say_drift_was_not_measured() {
    let mut other = sentinel::touch("control", &[0, 1]);
    merge(
        &mut other,
        serde_json::json!({ "touch": { "passed": ["lib::works", "lib::also"], "summary": { "protocol": "libtest", "tests_run": 2 } } }),
    );
    let engine = vec![sentinel::touch("baseline", &[0]), other];
    let silent = drift_violations(&with_engine(
        with(sentinel::drifted("not-measured")),
        engine.clone(),
    ));
    assert!(
        silent.iter().any(|line| line.contains("does not say so")),
        "a target no comparable control measured is owed the limitation: {silent:?}"
    );
    let mut stated = with(sentinel::drifted("not-measured"));
    merge(
        &mut stated,
        serde_json::json!({ "limitations": [{
            "name": "drift-not-measured",
            "detail": format!("1 target was not measured ({TARGET})")
        }] }),
    );
    let audit = with_engine(stated, engine);
    assert_eq!(drift_violations(&audit), Vec::<String>::new(), "{audit}");
}

#[test]
fn a_complete_report_is_re_decided_as_the_one_build_it_measured_whole() {
    let document =
        sentinel::complete_report(&sentinel::clean().document).expect("the specimen completes");
    let laid = sentinel::Perturbation {
        name: "complete",
        document,
        events: Some(routes()),
        engine: sentinel::clean().engine,
        shards: Vec::new(),
        outputs: Vec::new(),
    }
    .lay()
    .expect("the specimen is laid out");
    let audit = gates::proofaudit(laid.run(), laid.trace()).expect("a complete report is read");
    assert_eq!(audit.violations(), 0, "{audit}");
    assert_eq!(audit.mutants, 2, "{audit}");
    assert!(
        audit
            .remarks
            .iter()
            .any(|remark| remark.subject == "verdict" && remark.standing == Standing::Unaudited),
        "a complete report states no verdict and this recording ends in no run-end, and one this audit computed would agree with itself: {audit}"
    );
}

#[test]
fn a_complete_report_of_two_builds_is_refused_rather_than_read_as_one() {
    let mut document = sentinel::complete_report(&base()).expect("the specimen completes");
    let builds = document
        .pointer_mut("/report/builds")
        .and_then(serde_json::Value::as_array_mut)
        .expect("a complete report holds its builds");
    let second = builds.first().cloned().expect("one build");
    builds.push(second);
    let directory = run_directory(&document);
    let error = gates::proofaudit(directory.path(), None).expect_err("two builds are not one");
    assert!(matches!(error, AuditError::Unprojected { .. }), "{error}");
    assert!(error.to_string().contains("2 configured builds"), "{error}");
}

#[test]
fn a_baseline_that_passed_only_on_retry_is_owed_not_measured_rather_than_a_comparison() {
    let retried = serde_json::json!({
        "type": "verify",
        "verify": {
            "target": TARGET, "outcome": "survived", "tests_run": 1, "duration_ms": 1,
            "remembered": false, "retried": true
        }
    });
    let engine = vec![
        retried,
        sentinel::touch("baseline", &[0]),
        sentinel::touch("control", &[0, 1]),
    ];
    let said = drift_violations(&with_engine(
        with(sentinel::drifted("moved")),
        engine.clone(),
    ));
    assert!(
        said.iter().any(|line| line.contains("not-measured")),
        "a retry saw what its first attempt left and a control does not, so the difference is \
         the apparatus and the report must not call it a move: {said:?}"
    );
    let mut stated = with(sentinel::drifted("not-measured"));
    merge(
        &mut stated,
        serde_json::json!({ "limitations": [{
            "name": "drift-not-measured",
            "detail": format!("1 target was not measured ({TARGET})")
        }] }),
    );
    let audit = with_engine(stated, engine);
    assert_eq!(drift_violations(&audit), Vec::<String>::new(), "{audit}");
}

#[test]
fn a_control_whose_named_tests_fall_short_of_its_summary_is_no_comparison() {
    let mut short = sentinel::touch("control", &[0, 1]);
    merge(
        &mut short,
        serde_json::json!({ "touch": { "summary": { "protocol": "libtest", "tests_run": 2 } } }),
    );
    let engine = vec![sentinel::touch("baseline", &[0]), short];
    let silent = drift_violations(&with_engine(
        with(sentinel::drifted("not-measured")),
        engine.clone(),
    ));
    assert!(
        silent.iter().any(|line| line.contains("does not say so")),
        "a control read as passing one test where its own summary counted two was read by the \
         parser rather than the harness, so it compared nothing, and the target is owed the \
         limitation rather than a move: {silent:?}"
    );
    let mut stated = with(sentinel::drifted("not-measured"));
    merge(
        &mut stated,
        serde_json::json!({ "limitations": [{
            "name": "drift-not-measured",
            "detail": format!("1 target was not measured ({TARGET})")
        }] }),
    );
    let audit = with_engine(stated, engine);
    assert_eq!(drift_violations(&audit), Vec::<String>::new(), "{audit}");
}

#[test]
fn a_custom_harness_is_compared_on_its_reach_since_it_has_no_summary_to_fall_short_of() {
    let custom = |mut touch: serde_json::Value| {
        if let Some(record) = touch
            .get_mut("touch")
            .and_then(serde_json::Value::as_object_mut)
        {
            record.insert(
                "summary".to_owned(),
                serde_json::json!({ "protocol": "custom" }),
            );
            record.insert("passed".to_owned(), serde_json::json!([]));
        }
        touch
    };
    let baseline = custom(sentinel::touch("baseline", &[0]));
    let control = custom(sentinel::touch("control", &[0, 1]));
    let said = drift_violations(&with_engine(
        with(sentinel::drifted("not-measured")),
        vec![baseline, control],
    ));
    assert!(
        said.iter().any(|line| line.contains("moved")),
        "the test-set premise is empty for a harness that names no tests, so a union that moved \
         is still a counterexample, and a report that called it not measured is refused: \
         {said:?}"
    );
}

fn knob_violations(audit: &Audit) -> Vec<String> {
    audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Knobs && remark.standing == Standing::Violated)
        .map(|remark| format!("{}: {}", remark.subject, remark.detail))
        .collect()
}

/// The clean engine recording, and one control of the target under the time zone that came to `outcome`, with `failed` failing and `reach` becoming of its reach.
fn zoned(outcome: &str, failed: &[&str], reach: &serde_json::Value) -> Vec<serde_json::Value> {
    vec![
        sentinel::touch("baseline", &[0, 1]),
        sentinel::touch("control", &[0, 1]),
        sentinel::perturbed(outcome, failed, reach),
    ]
}

/// The clean report, whose one knob record stands as `standing`, with `beside` laid over it.
fn knob_report(standing: &serde_json::Value, beside: serde_json::Value) -> serde_json::Value {
    let mut document = with(sentinel::drifted("held"));
    merge(
        &mut document,
        serde_json::json!({
            "knobs": [{ "target": TARGET, "knob": "timezone", "standing": standing }]
        }),
    );
    merge(&mut document, beside);
    document
}

/// A finding of `kind` about the target, after the clean report's own.
fn raised(kind: &str) -> serde_json::Value {
    serde_json::json!({ "findings": [{}, {
        "kind": kind,
        "subject": TARGET,
        "detail": "said",
        "position": null
    }] })
}

/// One limitation named `name` whose closing list is the target.
fn stating(name: &str) -> serde_json::Value {
    serde_json::json!({ "limitations": [{
        "name": name,
        "detail": format!("timezone established nothing it could say ({TARGET})")
    }] })
}

#[test]
fn a_control_a_knob_broke_is_owed_a_broke_record_and_an_environment_dependent_finding() {
    let broke = zoned(
        "killed",
        &["lib::works"],
        &serde_json::json!({ "state": "not-read" }),
    );
    let quiet = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "stable" }),
            serde_json::json!({}),
        ),
        broke.clone(),
    ));
    assert!(
        quiet
            .iter()
            .any(|line| line.contains("failed lib::works, and the report records")),
        "a record that calls a target a knob broke stable is refused: {quiet:?}"
    );
    assert!(
        quiet
            .iter()
            .any(|line| line.contains("raises no environment-dependent finding")),
        "a target a knob broke that the report raises nothing about is refused: {quiet:?}"
    );
    let other_test = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "broke", "failed": ["lib::other"] }),
            raised("environment-dependent"),
        ),
        broke.clone(),
    ));
    assert!(
        other_test
            .iter()
            .any(|line| line.contains("and the report records")),
        "a broke record naming a test the control did not fail is refused: {other_test:?}"
    );
    let answered = with_engine(
        knob_report(
            &serde_json::json!({ "state": "broke", "failed": ["lib::works"] }),
            raised("environment-dependent"),
        ),
        broke,
    );
    assert_eq!(
        knob_violations(&answered),
        Vec::<String>::new(),
        "{answered}"
    );
}

#[test]
fn a_knob_record_is_the_record_of_the_one_control_the_engine_ran_under_that_knob() {
    let never = vec![
        sentinel::touch("baseline", &[0, 1]),
        sentinel::touch("control", &[0, 1]),
    ];
    let invented = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "stable" }),
            serde_json::json!({}),
        ),
        never,
    ));
    assert!(
        invented
            .iter()
            .any(|line| line.contains("recorded no one control of it")),
        "a knob the report says was put and the engine never ran is refused: {invented:?}"
    );
    let unrecorded = knob_violations(&with_engine(
        with(sentinel::drifted("held")),
        zoned("survived", &[], &sentinel::recorded_reach(&[0, 1])),
    ));
    assert!(
        unrecorded
            .iter()
            .any(|line| line.contains("the report records nothing about it")),
        "a control the engine ran under a knob that the report says nothing of is refused: \
         {unrecorded:?}"
    );
    let mut twice = zoned("survived", &[], &sentinel::recorded_reach(&[0, 1]));
    twice.push(sentinel::perturbed(
        "survived",
        &[],
        &sentinel::recorded_reach(&[0, 1]),
    ));
    let repeated = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "stable" }),
            serde_json::json!({}),
        ),
        twice,
    ));
    assert!(
        repeated
            .iter()
            .any(|line| line.contains("started 2 controls")),
        "a knob put twice on one target is refused, whatever the report says: {repeated:?}"
    );
}

#[test]
fn a_control_started_in_a_way_no_knob_puts_is_a_violation() {
    let mut odd = sentinel::perturbed("survived", &[], &sentinel::recorded_reach(&[0, 1]));
    merge(
        &mut odd,
        serde_json::json!({ "perturbed": { "perturbation": {
            "environment": [{}, { "name": "LC_ALL", "value": "C" }]
        } } }),
    );
    let said = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "stable" }),
            serde_json::json!({}),
        ),
        vec![
            sentinel::touch("baseline", &[0, 1]),
            sentinel::touch("control", &[0, 1]),
            odd,
        ],
    ));
    assert!(
        said.iter()
            .any(|line| line.contains("which is not what any knob puts")),
        "a control started with two things set is no knob, and nothing it established is \
         about one: {said:?}"
    );
}

#[test]
fn a_moved_reach_is_held_to_what_moved_in_every_union() {
    let moved = zoned("survived", &[], &sentinel::recorded_reach(&[0]));
    let reach = |lost: u64| {
        serde_json::json!({
            "reached": { "gained": [], "lost": [lost] },
            "bodies": { "gained": [], "lost": [] },
            "infected": { "gained": [], "lost": [] }
        })
    };
    let right = with_engine(
        knob_report(
            &serde_json::json!({ "state": "moved", "reach": reach(1) }),
            raised("environment-dependent-reach"),
        ),
        moved.clone(),
    );
    assert_eq!(knob_violations(&right), Vec::<String>::new(), "{right}");
    let wrong = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "moved", "reach": reach(0) }),
            raised("environment-dependent-reach"),
        ),
        moved.clone(),
    ));
    assert!(
        wrong
            .iter()
            .any(|line| line.contains("reached something else, and the report")),
        "a movement the unions do not show is refused: {wrong:?}"
    );
    let unraised = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "moved", "reach": reach(1) }),
            serde_json::json!({}),
        ),
        moved,
    ));
    assert!(
        unraised
            .iter()
            .any(|line| line.contains("raises no environment-dependent-reach finding")),
        "{unraised:?}"
    );
}

#[test]
fn a_control_that_compared_nothing_names_a_reason_that_holds_and_its_limitation_names_it() {
    let mut other = sentinel::recorded_reach(&[0, 1]);
    merge(
        &mut other,
        serde_json::json!({ "touch": {
            "passed": ["lib::works", "lib::also"],
            "summary": { "protocol": "libtest", "tests_run": 2 }
        } }),
    );
    let engine = zoned("survived", &[], &other);
    let wrong_reason = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "uncompared", "why": "unrecorded" }),
            stating("knob-not-compared"),
        ),
        engine.clone(),
    ));
    assert!(
        wrong_reason
            .iter()
            .any(|line| line.contains("because of other-tests")),
        "a reason that does not hold is refused, though the standing is right: {wrong_reason:?}"
    );
    let unstated = knob_violations(&with_engine(
        knob_report(
            &serde_json::json!({ "state": "uncompared", "why": "other-tests" }),
            serde_json::json!({}),
        ),
        engine.clone(),
    ));
    assert!(
        unstated
            .iter()
            .any(|line| line.contains("no knob-not-compared limitation names")),
        "{unstated:?}"
    );
    let stated = with_engine(
        knob_report(
            &serde_json::json!({ "state": "uncompared", "why": "other-tests" }),
            stating("knob-not-compared"),
        ),
        engine,
    );
    assert_eq!(knob_violations(&stated), Vec::<String>::new(), "{stated}");
}

#[test]
fn a_knob_not_put_is_held_to_no_control_under_it_and_to_the_limitation_that_says_so() {
    let never = vec![
        sentinel::touch("baseline", &[0, 1]),
        sentinel::touch("control", &[0, 1]),
    ];
    let not_put = || serde_json::json!({ "state": "not-put", "why": "zone-missing" });
    let stated = with_engine(
        knob_report(&not_put(), stating("knob-not-put")),
        never.clone(),
    );
    assert_eq!(knob_violations(&stated), Vec::<String>::new(), "{stated}");
    let unstated = knob_violations(&with_engine(
        knob_report(&not_put(), serde_json::json!({})),
        never,
    ));
    assert!(
        unstated
            .iter()
            .any(|line| line.contains("no knob-not-put limitation names")),
        "{unstated:?}"
    );
    let ran = knob_violations(&with_engine(
        knob_report(&not_put(), stating("knob-not-put")),
        zoned("survived", &[], &sentinel::recorded_reach(&[0, 1])),
    ));
    assert!(
        ran.iter()
            .any(|line| line.contains("was not put on") && line.contains("recorded a control")),
        "a knob the report says was not put and the engine ran is refused: {ran:?}"
    );
}

#[test]
fn a_part_of_a_catalog_records_its_knobs_and_is_not_held_to_findings_it_does_not_raise() {
    let broke = zoned(
        "killed",
        &["lib::works"],
        &serde_json::json!({ "state": "not-read" }),
    );
    let shard = with_engine(
        knob_report(
            &serde_json::json!({ "state": "broke", "failed": ["lib::works"] }),
            serde_json::json!({ "scope": { "shard": "1/2" } }),
        ),
        broke,
    );
    assert_eq!(
        knob_violations(&shard),
        Vec::<String>::new(),
        "a part raises no finding a knob earns, because the merge raises it from every part: \
         {shard}"
    );
}

#[test]
fn a_control_that_entered_an_item_its_baseline_did_not_is_owed_the_finding() {
    let mut control = sentinel::touch("control", &[0]);
    merge(
        &mut control,
        serde_json::json!({ "touch": { "entered_items": [4, 5] } }),
    );
    let mut baseline = sentinel::touch("baseline", &[0]);
    merge(
        &mut baseline,
        serde_json::json!({ "touch": { "entered_items": [4] } }),
    );
    let said = drift_violations(&with_engine(
        with(sentinel::drifted("held")),
        vec![baseline, control],
    ));
    assert!(
        !said.is_empty(),
        "every site agrees and the control entered item 5 the baseline never did, which is the \
         union `select` narrows by; a report that calls it held is refused: {said:?}"
    );
}
fn repair_audit(repaired: &[&str], engine: Vec<serde_json::Value>) -> Vec<String> {
    let mut events = routes();
    for mutant in repaired {
        events.push(serde_json::json!({
            "timestamp": "2026-09-06T00:00:02Z", "elapsed_ms": 2,
            "type": "mutant-exec",
            "mutant": {
                "mutant": mutant, "target": TARGET, "args": [], "outcome": "survived",
                "duration_ms": 5
            }
        }));
        events.push(serde_json::json!({
            "type": "repair",
            "repair": {
                "mutant": mutant, "target": TARGET,
                "was": "survived", "now": "survived", "reached": "reached"
            }
        }));
    }
    let laid = sentinel::Perturbation {
        name: "repair",
        document: with(sentinel::drifted("moved")),
        events: Some(events),
        engine: Some(engine),
        shards: Vec::new(),
        outputs: Vec::new(),
    }
    .lay()
    .expect("the specimen is laid out");
    let audit =
        gates::proofaudit(laid.run(), laid.trace()).expect("a recording this audit can read");
    audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Repair && remark.standing == Standing::Violated)
        .map(|remark| format!("{}: {}", remark.subject, remark.detail))
        .collect()
}

fn repair_touch(mutant: &str, reached: &[u32]) -> serde_json::Value {
    let mut touch = sentinel::touch("repair", reached);
    merge(
        &mut touch,
        serde_json::json!({ "touch": { "mutant": mutant } }),
    );
    touch
}

#[test]
fn a_repair_touch_is_paired_with_the_repair_that_names_its_mutant_and_nothing_else() {
    let moved = || {
        vec![
            sentinel::touch("baseline", &[0]),
            sentinel::touch("control", &[0, 1]),
        ]
    };
    let mut named = moved();
    named.push(repair_touch(&"b".repeat(64), &[1]));
    let said = repair_audit(&[SURVIVED], named);
    assert!(
        !said.iter().any(|line| line.contains("repair touch")),
        "the one repair touch names the one repaired mutant: {said:?}"
    );
    let mut stray = moved();
    stray.push(repair_touch(&"a".repeat(64), &[1]));
    stray.push(repair_touch(&"b".repeat(64), &[1]));
    let said = repair_audit(&[SURVIVED], stray);
    assert!(
        said.iter()
            .any(|line| line.contains(&"a".repeat(64)) && line.contains("no repair record")),
        "a repair touch naming a mutant no repair record names is refused by name, not counted: \
         {said:?}"
    );
}

#[test]
fn a_repair_touch_that_names_no_mutation_is_unread_rather_than_paired_by_position() {
    let engine = vec![
        sentinel::touch("baseline", &[0]),
        sentinel::touch("control", &[0, 1]),
        sentinel::touch("repair", &[1]),
    ];
    let audit = with_engine(with(sentinel::drifted("moved")), engine);
    assert!(
        audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Drift
                && remark.standing == Standing::Unaudited
                && remark.detail.contains("which mutation a repair ran")),
        "{audit}"
    );
}

fn on_target(mut touch: serde_json::Value, target: &str) -> serde_json::Value {
    merge(
        &mut touch,
        serde_json::json!({ "touch": { "target": target } }),
    );
    touch
}

fn two_repairs(second_was: &str) -> Vec<String> {
    let other = "pkg/test/other";
    let mut events = vec![serde_json::json!({
        "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0,
        "type": "route",
        "route": {
            "mutant": SURVIVED, "granularity": "unreached", "fallback": null,
            "reaching": [], "discharged": [], "considered": [], "reused": null
        }
    })];
    for (target, was) in [(TARGET, "unreached"), (other, second_was)] {
        events.push(serde_json::json!({
            "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1,
            "type": "mutant-exec",
            "mutant": {
                "mutant": SURVIVED, "target": target, "args": [], "outcome": "survived",
                "duration_ms": 5
            }
        }));
        events.push(serde_json::json!({
            "type": "repair",
            "repair": {
                "mutant": SURVIVED, "target": target,
                "was": was, "now": "survived", "reached": "reached"
            }
        }));
    }
    let mut engine = Vec::new();
    for target in [TARGET, other] {
        engine.push(on_target(sentinel::touch("baseline", &[0]), target));
        engine.push(on_target(sentinel::touch("control", &[0, 1]), target));
        engine.push(on_target(repair_touch(&"b".repeat(64), &[1]), target));
    }
    let laid = sentinel::Perturbation {
        name: "two repairs",
        document: with(serde_json::json!({ "drift": [
            sentinel::moved(other),
            sentinel::moved(TARGET)
        ] })),
        events: Some(events),
        engine: Some(engine),
        shards: Vec::new(),
        outputs: Vec::new(),
    }
    .lay()
    .expect("the specimen is laid out");
    let audit =
        gates::proofaudit(laid.run(), laid.trace()).expect("a recording this audit can read");
    audit
        .remarks
        .iter()
        .filter(|remark| remark.layer == Layer::Repair && remark.standing == Standing::Violated)
        .map(|remark| format!("{}: {}", remark.subject, remark.detail))
        .collect()
}

#[test]
fn a_second_repair_starts_from_what_the_first_one_made_it() {
    let said = two_repairs("survived");
    assert!(
        !said
            .iter()
            .any(|line| line.contains("the repair says it was")),
        "the second repair was what the first one made it, not what its route did: {said:?}"
    );
    let said = two_repairs("unreached");
    assert!(
        said.iter()
            .any(|line| line.contains("the repair of it before this one made it survived")),
        "a second repair that starts from the route rather than the first repair is refused: \
         {said:?}"
    );
}

fn laid_sharded(perturbation: &sentinel::Perturbation) -> sentinel::Laid {
    perturbation
        .lay()
        .expect("the sharded specimen is laid out")
}

fn merge_audit(perturbation: &sentinel::Perturbation) -> Result<Audit, AuditError> {
    let laid = laid_sharded(perturbation);
    let shards: Vec<std::path::PathBuf> =
        laid.shards().into_iter().map(Path::to_path_buf).collect();
    gates::proofaudit_merged(laid.run(), &shards, laid.traces())
}

fn merge_violations(perturbation: &sentinel::Perturbation) -> Vec<String> {
    match merge_audit(perturbation) {
        Ok(audit) => audit
            .remarks
            .iter()
            .filter(|remark| remark.standing == Standing::Violated)
            .map(|remark| format!("{}: {}", remark.subject, remark.detail))
            .collect(),
        Err(error) => vec![format!("refused: {error}")],
    }
}

fn sharded() -> sentinel::Perturbation {
    sentinel::sharded_clean().expect("the specimen is measured in shards")
}

#[test]
fn a_shard_document_is_re_decided_against_its_own_recording_as_the_one_part_it_measured() {
    let laid = laid_sharded(&sharded());
    let first = laid.shards().into_iter().next().expect("a first shard");
    let trace = laid
        .traces()
        .expect("the shards' recordings")
        .join(format!("{}-s1", sentinel::MERGED));
    let audit = gates::proofaudit(first, Some(&trace)).expect("a shard is read as its part");
    assert_eq!(audit.violations(), 0, "{audit}");
    assert_eq!(audit.mutants, 1, "{audit}");
    assert_eq!(audit.run_id, format!("{}-s1", sentinel::MERGED), "{audit}");
}

#[test]
fn a_merged_report_is_held_to_every_shard_it_names_and_says_which_it_could_not_see() {
    let clean = sharded();
    let whole = merge_audit(&clean).expect("the merge is read with its shards");
    assert_eq!(whole.violations(), 0, "{whole}");
    assert_eq!(whole.mutants, 2, "{whole}");
    let mut half = clean.clone();
    half.shards.truncate(1);
    let half = merge_audit(&half).expect("the merge is read with one shard");
    assert!(
        half.remarks
            .iter()
            .any(|remark| remark.layer == Layer::Merge
                && remark.standing == Standing::Unaudited
                && remark.subject == format!("{}-s2", sentinel::MERGED)),
        "a shard the report names and nobody gave is unaudited, not agreed: {half}"
    );
    let alone = run_directory(&clean.document);
    let error = gates::proofaudit(alone.path(), None).expect_err("two parts are not one");
    assert!(matches!(error, AuditError::Unprojected { .. }), "{error}");
}

#[test]
fn a_shard_the_report_was_not_merged_from_is_refused_rather_than_ignored() {
    let mut stranger = sharded();
    if let Some(shard) = stranger.shards.get_mut(1) {
        merge(
            &mut shard.document,
            serde_json::json!({ "report": { "run_id": "20260906T101500Z-stranger" } }),
        );
    }
    let error = merge_audit(&stranger).expect_err("a stranger is not a part");
    assert!(
        matches!(error, AuditError::ShardNotMerged { .. }),
        "{error}"
    );
}

#[test]
fn a_shard_given_twice_is_refused_rather_than_counted_once() {
    let mut twice = sharded();
    let first = twice.shards.first().cloned().expect("a first shard");
    twice.shards.insert(0, first);
    let error = merge_audit(&twice).expect_err("the same shard twice is an operator's mistake");
    assert!(
        matches!(error, AuditError::ShardGivenTwice { .. }),
        "{error}"
    );
}

#[test]
fn only_a_merge_is_held_to_shards_and_only_a_shard_is_given_as_one() {
    let clean = sharded();
    let mut direct = clean.clone();
    direct.document = sentinel::complete_report(&base()).expect("the specimen completes");
    let error = merge_audit(&direct).expect_err("a report measured whole is no merge");
    assert!(matches!(error, AuditError::NotMerged { .. }), "{error}");
    let mut impostor = clean;
    if let Some(shard) = impostor.shards.get_mut(1) {
        shard.document = sentinel::complete_report(&base()).expect("the specimen completes");
    }
    let error = merge_audit(&impostor).expect_err("a complete report is not a shard");
    assert!(matches!(error, AuditError::NotAShard { .. }), "{error}");
}

#[test]
fn every_rule_of_a_merge_is_refused_by_name_where_its_plant_breaks_it() {
    for rule in xtask::proofaudit::merge::MergeRule::ALL {
        let plant = sentinel::merge_plant(rule).expect("a plant for every rule");
        let said = merge_violations(&plant);
        let prefix = format!("{}: ", rule.label());
        assert!(
            said.iter().any(|line| line.contains(&prefix)),
            "`{}` must draw a {} violation: {said:?}",
            plant.name,
            rule.label()
        );
    }
}

#[test]
fn a_real_run_measured_in_two_shards_and_merged_is_re_decided_clean_shard_by_shard() {
    let recorded = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/sharded-run");
    let mut shards: Vec<std::path::PathBuf> = std::fs::read_dir(recorded.join("runs"))
        .expect("the recorded shards")
        .map(|entry| entry.expect("a readable shard").path())
        .collect();
    shards.sort();
    let audit = gates::proofaudit_merged(
        &recorded.join("merged.json"),
        &shards,
        Some(&recorded.join("traces")),
    )
    .expect("a real merge is read with its shards");
    assert_eq!(audit.violations(), 0, "{audit}");
    assert_eq!(audit.mutants, 4, "{audit}");
    assert!(
        !audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Merge && remark.standing == Standing::Unaudited),
        "every shard the merge names was given and re-decided: {audit}"
    );
}

#[test]
fn a_merge_says_how_far_each_layer_got_over_its_parts_and_no_further() {
    let clean = sharded();
    let whole = merge_audit(&clean).expect("the merge is read with its shards");
    for layer in Layer::ALL {
        assert!(
            whole.coverage.contains_key(&layer),
            "{} says how far it got: {whole}",
            layer.label()
        );
    }
    assert_eq!(
        whole.coverage.get(&Layer::Merge),
        Some(&Coverage::Rederived),
        "{whole}"
    );
    let mut half = clean;
    half.shards.truncate(1);
    let half = merge_audit(&half).expect("the merge is read with one shard");
    for layer in Layer::ALL
        .into_iter()
        .filter(|layer| *layer != Layer::Merge)
    {
        assert_eq!(
            half.coverage.get(&layer),
            Some(&Coverage::Partly),
            "{} cannot have re-decided a part nobody gave: {half}",
            layer.label()
        );
    }
}

#[test]
fn the_audit_comes_to_the_verdict_the_published_contract_gives_every_case() {
    let contract = xtask::strictjson::from_str(
        &std::fs::read_to_string(gates::workspace_root().join("schema/miri-output.json"))
            .expect("the published contract"),
    )
    .expect("the contract is JSON");
    let cases = contract
        .get("cases")
        .and_then(serde_json::Value::as_array)
        .expect("the contract's cases");
    for case in cases {
        let field = |key: &str| case.get(key).and_then(serde_json::Value::as_str);
        let name = field("name").expect("a case is named");
        assert_eq!(
            Some(xtask::proofaudit::soundness::verdict(
                field("output").expect("a case has output"),
                case.get("status").and_then(serde_json::Value::as_i64),
            )),
            field("came"),
            "{name}: the audit and the runner come to what the contract says of this case"
        );
    }
}

#[test]
fn an_audit_that_left_something_unaudited_does_not_exit_as_one_that_checked_everything() {
    let unrecorded = audited(&base());
    assert_eq!(unrecorded.violations(), 0, "{unrecorded}");
    assert!(unrecorded.unaudited() > 0, "{unrecorded}");
    assert_eq!(
        unrecorded.exit_code(),
        xtask::proofaudit::EXIT_UNAUDITED,
        "a run whose recording was not given leaves layers unaudited, and a step that reads only \
         the exit code must not read that as an audit that checked everything"
    );
    assert_eq!(xtask::proofaudit::EXIT_UNAUDITED, 3);
}
