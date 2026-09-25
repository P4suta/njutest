// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gate that re-decides a completed engine run without asking the engine whether it agrees with itself.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads as a table"
)]

use std::path::Path;

use njutest_devkit::result::{ResultState, result_state};
use xtask::engineaudit::sentinel::{
    self, KILLED, SOURCE, SURVIVED, TARGET, base, recording, routed_by_test, short, touched, with,
};
use xtask::engineaudit::{Audit, AuditError, EXIT_UNREADABLE, Evidence, Layer, Source, Standing};
use xtask::gates;

fn run_directory(document: &serde_json::Value) -> tempfile::TempDir {
    sentinel::run_directory(document).expect("a run directory")
}

fn recorded(events: &[serde_json::Value]) -> tempfile::TempDir {
    sentinel::recorded(events).expect("a recording")
}

const fn asked<'a>(
    run: &'a Path,
    trace: Option<&'a Path>,
    ledger: Option<&'a Path>,
) -> gates::EngineRun<'a> {
    gates::EngineRun {
        run,
        trace,
        shards: &[],
        ledger,
        sites: true,
    }
}

fn audited(document: &serde_json::Value) -> Audit {
    let directory = run_directory(document);
    gates::engine_audit(&asked(directory.path(), None, None)).expect("a report this audit can read")
}

fn audited_with(document: &serde_json::Value, events: &[serde_json::Value]) -> Audit {
    let run = run_directory(document);
    let trace = recorded(events);
    gates::engine_audit(&asked(run.path(), Some(trace.path()), None))
        .expect("a report this audit can read")
}

fn violations(audit: &Audit, layer: Layer) -> Vec<String> {
    audit
        .of(layer)
        .into_iter()
        .filter(|remark| remark.standing == Standing::Violated)
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_clean_run_is_silent_on_every_layer_it_can_re_decide() {
    let audit = audited_with(&base(), &recording());
    assert_eq!(audit.violations(), 0, "{audit}");
    assert_eq!(audit.mutants, 2, "{audit}");
    assert_eq!(audit.rejections, 1, "{audit}");
    assert_eq!(audit.exit_code(), undecided(&audit), "{audit}");
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
fn an_id_that_does_not_re_mint_from_its_own_fields_is_a_violation() {
    let audit = audited(&with(serde_json::json!({
        "mutants": [{ "start_byte": 104 }]
    })));
    let found = violations(&audit, Layer::Identity);
    assert_eq!(found.len(), 1, "{audit}");
    assert!(found[0].contains("does not re-mint"), "{found:?}");
}

#[test]
fn a_row_without_byte_offsets_is_not_a_report() {
    let mut document = base();
    let row = document["mutants"][0]
        .as_object_mut()
        .expect("the first row");
    assert!(
        row.remove("start_byte").is_some(),
        "the fixture has an offset"
    );
    let run = run_directory(&document);
    let error = gates::engine_audit(&asked(run.path(), None, None))
        .expect_err("a report missing an identity input must fail at the boundary");
    assert!(matches!(error, AuditError::OffSchema { .. }), "{error}");
}

#[test]
fn the_indices_of_the_accepted_and_the_refused_are_the_whole_catalog() {
    let audit = audited(&with(serde_json::json!({
        "rejections": [{ "index": 7 }]
    })));
    let found = violations(&audit, Layer::Identity);
    assert!(
        found.iter().any(|remark| remark.contains("index")),
        "{audit}"
    );
}

#[test]
fn the_outcome_columns_must_add_up_to_executed() {
    let audit = audited(&with(serde_json::json!({
        "accounting": { "executed": 3 }
    })));
    let found = violations(&audit, Layer::Accounting);
    assert!(
        found.iter().any(|remark| remark.contains("executed")),
        "{audit}"
    );
}

#[test]
fn the_skipped_column_is_the_checked_sum_of_skip_records() {
    let audit = audited(&with(serde_json::json!({
        "skips": [{
            "reason": "test-code", "path": "src/lib.rs", "count": 2,
            "explanation": "the code measures itself"
        }]
    })));
    let found = violations(&audit, Layer::Accounting);
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("skip records come to 2")),
        "{audit}"
    );
}

#[test]
fn a_column_the_rows_do_not_come_to_is_a_violation() {
    let audit = audited(&with(serde_json::json!({
        "accounting": { "killed": 2, "survived": 0 }
    })));
    let found = violations(&audit, Layer::Accounting);
    assert!(found.len() >= 2, "{audit}");
}

#[test]
fn a_score_present_when_nothing_was_decided_is_a_violation() {
    let audit = audited(&with(serde_json::json!({
        "accounting": { "cataloged": 0, "executed": 0, "killed": 0, "survived": 0, "expected": 0, "refused": 0 },
        "mutants": [],
        "rejections": [],
        "expectations": []
    })));
    let found = violations(&audit, Layer::Score);
    assert_eq!(found.len(), 1, "{audit}");
    assert!(found[0].contains("decided nothing"), "{found:?}");
}

#[test]
fn a_score_that_is_not_the_ratio_of_its_own_columns_is_a_violation() {
    let audit = audited(&with(serde_json::json!({
        "score": { "detected": 1, "decided": 2, "value": 0.9 }
    })));
    assert_eq!(violations(&audit, Layer::Score).len(), 1, "{audit}");
}

#[test]
fn a_step_limit_is_accounted_for_but_never_counted_as_detected() {
    let document = with(serde_json::json!({
        "run": { "exit_code": 2 },
        "selection": { "mutant_steps": 10 },
        "accounting": {
            "killed": 0, "survived": 1, "step_limit_reached": 1, "waited": 0,
            "inconclusive": 0, "errored": 0
        },
        "score": { "detected": 0, "decided": 1, "value": 0.0 },
        "mutants": [{
            "outcome": "step_limit_reached", "expected": false, "killed_by": [],
            "step_notice": {
                "nonce": "00000000000000000000000000000000",
                "catalog": "c".repeat(64), "mutant": KILLED, "limit": 10, "observed": 11
            }
        }, {}],
        "findings": [{
            "kind": "step-limit-reached-mutant", "mutant": short(KILLED),
            "detail": "the verified count was reached"
        }]
    }));
    let audit = audited(&document);
    assert!(violations(&audit, Layer::Accounting).is_empty(), "{audit}");
    assert!(violations(&audit, Layer::Score).is_empty(), "{audit}");
    assert!(violations(&audit, Layer::Findings).is_empty(), "{audit}");
    assert!(violations(&audit, Layer::Exit).is_empty(), "{audit}");
}

#[test]
fn a_step_limit_notice_must_bind_the_selected_allowance_catalog_and_mutant() {
    let document = with(serde_json::json!({
        "run": { "exit_code": 2 },
        "selection": { "mutant_steps": 10 },
        "accounting": {
            "killed": 0, "survived": 1, "step_limit_reached": 1, "waited": 0,
            "inconclusive": 0, "errored": 0
        },
        "score": { "detected": 0, "decided": 1, "value": 0.0 },
        "mutants": [{
            "outcome": "step_limit_reached", "expected": false, "killed_by": [],
            "step_notice": {
                "nonce": "0".repeat(32), "catalog": "e".repeat(64),
                "mutant": SURVIVED, "limit": 9, "observed": 9
            }
        }, {}],
        "findings": [{
            "kind": "step-limit-reached-mutant", "mutant": short(KILLED), "detail": "bad"
        }]
    }));
    let audit = audited(&document);
    assert_eq!(
        violations(&audit, Layer::Accounting).len(),
        4,
        "another catalog, another mutant, another allowance, and a count not one past it; \
         a nonce that is not hex is refused by the schema before any layer reads it: {audit}"
    );
}

#[test]
fn a_waited_mutant_is_an_infrastructure_finding_not_a_detection() {
    let document = with(serde_json::json!({
        "run": { "exit_code": 2 },
        "accounting": {
            "killed": 0, "survived": 1, "step_limit_reached": 0, "waited": 1,
            "inconclusive": 0, "errored": 0
        },
        "score": { "detected": 0, "decided": 1, "value": 0.0 },
        "mutants": [
            { "outcome": "waited", "retried": true, "lingered": false, "expected": false, "killed_by": [] },
            {}
        ],
        "findings": [{
            "kind": "waited-mutant", "mutant": short(KILLED),
            "detail": "the wall-clock bound expired twice"
        }]
    }));
    let audit = audited(&document);
    assert!(violations(&audit, Layer::Accounting).is_empty(), "{audit}");
    assert!(violations(&audit, Layer::Score).is_empty(), "{audit}");
    assert!(violations(&audit, Layer::Findings).is_empty(), "{audit}");
    assert!(violations(&audit, Layer::Exit).is_empty(), "{audit}");
}

#[test]
fn a_survivor_nobody_expected_and_no_finding_names_is_a_violation() {
    let audit = audited(&with(serde_json::json!({
        "accounting": { "expected": 0 },
        "mutants": [{}, { "expected": false }],
        "expectations": []
    })));
    let found = violations(&audit, Layer::Findings);
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("surviving-mutant")),
        "{audit}"
    );
}

#[test]
fn a_met_expectation_marks_its_row_expected() {
    let audit = audited(&with(serde_json::json!({
        "accounting": { "expected": 0 },
        "mutants": [{}, { "expected": false }],
        "findings": [{ "kind": "surviving-mutant", "mutant": short(SURVIVED), "detail": "no test noticed it" }]
    })));
    let found = violations(&audit, Layer::Expectations);
    assert_eq!(found.len(), 1, "{audit}");
    assert!(found[0].contains("not marked expected"), "{found:?}");
}

#[test]
fn the_exit_code_follows_the_findings() {
    let audit = audited(&with(serde_json::json!({
        "accounting": { "expected": 0 },
        "mutants": [{}, { "expected": false }],
        "expectations": [],
        "findings": [{ "kind": "surviving-mutant", "mutant": short(SURVIVED), "detail": "no test noticed it" }]
    })));
    let found = violations(&audit, Layer::Exit);
    assert_eq!(found.len(), 1, "{audit}");
    assert!(found[0].contains("earns 1"), "{found:?}");
}

#[test]
fn an_interrupted_run_that_did_not_exit_130_is_a_violation() {
    let audit = audited(&with(serde_json::json!({
        "run": { "interrupted": true }
    })));
    assert_eq!(violations(&audit, Layer::Exit).len(), 1, "{audit}");
}

#[test]
fn the_parts_of_one_catalog_recount_to_the_whole() {
    let run = run_directory(&base());
    let other = with(serde_json::json!({ "mutants": [{ "index": 0 }] }));
    let part = tempfile::tempdir().expect("a temporary directory");
    let path = part.path().join("run-report-v1.json");
    std::fs::write(&path, other.to_string()).expect("the part");
    let audit = gates::engine_audit(&gates::EngineRun {
        run: run.path(),
        trace: None,
        shards: &[path],
        ledger: None,
        sites: false,
    })
    .expect("a report this audit can read");
    let found = violations(&audit, Layer::Merge);
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("is in two parts")),
        "{audit}"
    );
}

#[test]
fn a_mutant_exec_that_disagrees_with_its_row_is_a_violation() {
    let mut events = recording();
    events[8]["mutant"]["outcome"] = serde_json::json!("survived");
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found.iter().any(|remark| remark.contains("disagrees")),
        "{audit}"
    );
}

#[test]
fn a_row_says_it_lingered_exactly_when_its_recorded_execution_did() {
    let mut claimed = base();
    claimed["mutants"][0]["lingered"] = serde_json::json!(true);
    let audit = audited_with(&claimed, &recording());
    assert!(
        violations(&audit, Layer::Trace)
            .iter()
            .any(|remark| remark.contains("lingered")),
        "a row that says its process outlived its harness's answer, over an execution the \
         recording says ended with it, rests on nothing: {audit}"
    );
    let mut events = recording();
    events[8]["mutant"]["lingered"] = serde_json::json!(true);
    let audit = audited_with(&base(), &events);
    assert!(
        violations(&audit, Layer::Trace)
            .iter()
            .any(|remark| remark.contains("lingered")),
        "and a row that hides an execution the recording says lingered hides it: {audit}"
    );
}

#[test]
fn a_kill_recorded_from_a_signal_sent_from_outside_is_a_violation() {
    let mut events = recording();
    events[8]["mutant"]["signal"] = serde_json::json!(9);
    events[8]["mutant"]["failed_tests"] = serde_json::json!([]);
    let audit = audited_with(&base(), &events);
    assert!(
        violations(&audit, Layer::Trace)
            .iter()
            .any(|remark| remark.contains("signal 9")),
        "a SIGKILL with no failing test named is what a cancelled job or an out-of-memory \
         killer leaves, and a kill kept from it hides a survivor from every run that reads it \
         back: {audit}"
    );
    let mut raised = recording();
    raised[8]["mutant"]["signal"] = serde_json::json!(6);
    raised[8]["mutant"]["failed_tests"] = serde_json::json!([]);
    let audit = audited_with(&base(), &raised);
    assert!(
        !violations(&audit, Layer::Trace)
            .iter()
            .any(|remark| remark.contains("signal 6")),
        "an abort the process raised itself is a kill a mutation can cause: {audit}"
    );
}

#[test]
fn an_instrument_record_that_moved_a_line_is_a_violation() {
    let mut events = recording();
    events[2]["instrument"]["lines_after"] = serde_json::json!(41);
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found.iter().any(|remark| remark.contains("40 lines to 41")),
        "{audit}"
    );
}

#[test]
fn a_recording_that_dropped_events_is_a_violation() {
    let mut events = recording();
    let last = events.len().saturating_sub(1);
    events[last]["run"]["events_dropped"] = serde_json::json!(3);
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("dropped 3 events")),
        "{audit}"
    );
}

#[test]
fn a_recording_that_never_ended_is_a_violation() {
    let mut events = recording();
    assert!(
        events.pop().is_some(),
        "the fixture has a run-end to remove"
    );
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found.iter().any(|remark| remark.contains("run-end")),
        "{audit}"
    );
}

#[test]
fn a_phase_the_recording_never_closed_is_a_violation() {
    let mut events = recording();
    let closed = events.remove(6);
    assert_eq!(closed["type"], "phase-end", "the fixture removes the close");
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found.iter().any(|remark| remark.contains("never ended")),
        "{audit}"
    );
}

#[test]
fn condemned_indices_must_be_the_rejections() {
    let mut events = recording();
    events[3]["round"]["attributed"] = serde_json::json!([
        { "index": 0, "code": "E0369", "said": "cannot subtract" }
    ]);
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert_eq!(found.len(), 2, "{audit}");
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("does not refuse")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("no round condemned")),
        "{found:?}"
    );
}

#[test]
fn a_target_the_build_produced_and_nothing_verified_is_a_violation() {
    let mut events = recording();
    events[4]["build"]["targets"] = serde_json::json!([TARGET, "demo/test/ui"]);
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found.iter().any(|remark| remark.contains("demo/test/ui")),
        "{audit}"
    );
}

#[test]
fn a_target_the_build_records_as_configured_out_needs_no_verification() {
    let mut events = recording();
    events[4]["build"]["targets"] = serde_json::json!([TARGET, "demo/test/ui"]);
    events[4]["build"]["details"] = serde_json::json!([
        {"id": TARGET, "kind": "lib", "harness": true, "limitations": []},
        {
            "id": "demo/test/ui",
            "kind": "test",
            "harness": true,
            "limitations": ["target-skipped-by-configuration"]
        }
    ]);
    let audit = audited_with(&base(), &events);
    assert!(violations(&audit, Layer::Trace).is_empty(), "{audit}");
}

#[test]
fn a_discharged_target_that_then_ran_is_a_violation() {
    let mut events = recording();
    events[7]["route"]["discharged"] =
        serde_json::json!([{ "target": TARGET, "proof": "branch-never-taken" }]);
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found.iter().any(|remark| remark.contains("removed")),
        "{audit}"
    );
}

#[test]
fn an_unreached_route_that_executed_is_a_violation() {
    let mut events = recording();
    events[7]["route"]["granularity"] = serde_json::json!("unreached");
    let audit = audited_with(&base(), &events);
    let found = violations(&audit, Layer::Trace);
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("no measured target reaches")),
        "{audit}"
    );
}

#[test]
fn a_run_without_a_recording_leaves_the_trace_layers_unaudited() {
    let audit = audited(&base());
    assert_eq!(audit.violations(), 0, "{audit}");
    let remarks = audit.of(Layer::Trace);
    assert_eq!(remarks.len(), 1, "{audit}");
    assert_eq!(remarks[0].standing, Standing::Unaudited);
    assert!(
        audit.of(Layer::Merge)[0].standing == Standing::Unaudited,
        "{audit}"
    );
    assert!(
        audit.of(Layer::Ledger)[0].standing == Standing::Unaudited,
        "{audit}"
    );
}

#[test]
fn a_survivor_the_ledger_does_not_explain_fails_the_dogfood_gate() {
    let ledger = tempfile::tempdir().expect("a temporary directory");
    let path = ledger.path().join(".rust-mutants.toml");
    std::fs::write(&path, "[mutation]\ntier = \"balanced\"\n").expect("the ledger");
    let document = with(serde_json::json!({
        "accounting": { "expected": 0 },
        "mutants": [{}, { "expected": false }],
        "expectations": [],
        "run": { "exit_code": 1 },
        "findings": [{ "kind": "surviving-mutant", "mutant": short(SURVIVED), "detail": "no test noticed it" }]
    }));
    let run = run_directory(&document);
    let audit = gates::engine_audit(&asked(run.path(), None, Some(&path)))
        .expect("a report this audit can read");
    let found = violations(&audit, Layer::Ledger);
    assert_eq!(found.len(), 1, "{audit}");
    assert!(found[0].contains("either killed or accepted"), "{found:?}");
}

#[test]
fn a_ledger_that_accepts_what_the_run_does_not_hold_is_a_violation() {
    let ledger = tempfile::tempdir().expect("a temporary directory");
    let path = ledger.path().join(".rust-mutants.toml");
    std::fs::write(
        &path,
        format!(
            "[[mutation.expect]]\nid = \"{SURVIVED}\"\nreason = \"equivalent\"\noutcome = \
             \"survived\"\n\n[[mutation.expect]]\nid = \"{}\"\nreason = \"gone\"\noutcome = \
             \"survived\"\n",
            "d".repeat(64)
        ),
    )
    .expect("the ledger");
    let run = run_directory(&base());
    let audit = gates::engine_audit(&asked(run.path(), None, Some(&path)))
        .expect("a report this audit can read");
    let found = violations(&audit, Layer::Ledger);
    assert_eq!(found.len(), 1, "{audit}");
    assert!(found[0].contains("the run does not hold it"), "{found:?}");
}

#[test]
fn a_ledger_that_explains_every_survivor_is_silent() {
    let ledger = tempfile::tempdir().expect("a temporary directory");
    let path = ledger.path().join(".rust-mutants.toml");
    std::fs::write(
        &path,
        format!(
            "[[mutation.expect]]\nid = \"{SURVIVED}\"\nreason = \"equivalent\"\noutcome = \
             \"survived\"\n"
        ),
    )
    .expect("the ledger");
    let run = run_directory(&base());
    let audit = gates::engine_audit(&asked(run.path(), None, Some(&path)))
        .expect("a report this audit can read");
    assert_eq!(
        violations(&audit, Layer::Ledger),
        Vec::<String>::new(),
        "{audit}"
    );
}

#[test]
fn the_exit_code_follows_the_violations() {
    let clean = audited_with(&base(), &recording());
    assert_eq!(clean.exit_code(), undecided(&clean), "{clean}");
    let broken = audited(&with(
        serde_json::json!({ "mutants": [{ "start_byte": 104 }] }),
    ));
    assert_eq!(broken.exit_code(), 1, "{broken}");
}

#[test]
fn a_directory_without_a_report_is_neither_clean_nor_broken() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let error = gates::engine_audit(&asked(directory.path(), None, None))
        .expect_err("a directory with no report");
    assert!(matches!(error, AuditError::Unreadable { .. }), "{error}");
    assert_eq!(EXIT_UNREADABLE, 2);
}

#[test]
fn every_explicit_evidence_path_must_be_readable() {
    let run = run_directory(&base());
    let missing = run.path().join("missing");

    let error = gates::engine_audit(&asked(run.path(), Some(&missing), None))
        .expect_err("an explicitly requested recording must exist");
    assert!(matches!(error, AuditError::Unreadable { .. }), "{error}");

    let shards = [missing.clone()];
    let error = gates::engine_audit(&gates::EngineRun {
        run: run.path(),
        trace: None,
        shards: &shards,
        ledger: None,
        sites: false,
    })
    .expect_err("an explicitly requested shard must exist");
    assert!(matches!(error, AuditError::Unreadable { .. }), "{error}");

    let error = gates::engine_audit(&asked(run.path(), None, Some(&missing)))
        .expect_err("an explicitly requested ledger must exist");
    assert!(matches!(error, AuditError::Unreadable { .. }), "{error}");
}

#[test]
fn malformed_explicit_evidence_is_neither_absent_nor_unaudited() {
    let run = run_directory(&base());
    let trace = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(trace.path().join("trace.jsonl"), "not json\n").expect("the corrupt recording");
    let error = gates::engine_audit(&asked(run.path(), Some(trace.path()), None))
        .expect_err("corrupt recording evidence must fail closed");
    assert!(
        matches!(error, AuditError::MalformedRecording { .. }),
        "{error}"
    );

    let shard = run.path().join("shard.json");
    std::fs::write(&shard, "not json").expect("the corrupt shard");
    let shards = [shard];
    let error = gates::engine_audit(&gates::EngineRun {
        run: run.path(),
        trace: None,
        shards: &shards,
        ledger: None,
        sites: false,
    })
    .expect_err("a corrupt shard must fail closed");
    assert!(
        matches!(error, AuditError::MalformedEvidence { .. }),
        "{error}"
    );

    let ledger = run.path().join("ledger.toml");
    std::fs::write(&ledger, "[[").expect("the corrupt ledger");
    let error = gates::engine_audit(&asked(run.path(), None, Some(&ledger)))
        .expect_err("a corrupt ledger must fail closed");
    assert!(
        matches!(error, AuditError::MalformedLedger { .. }),
        "{error}"
    );
}

#[test]
fn optional_run_evidence_is_optional_only_when_not_found() {
    let run = run_directory(&base());
    std::fs::write(run.path().join("reached-v1.json"), "not json")
        .expect("the corrupt optional evidence");
    let error = gates::engine_audit(&asked(run.path(), None, None))
        .expect_err("present but corrupt optional evidence must fail closed");
    assert!(
        matches!(error, AuditError::MalformedEvidence { .. }),
        "{error}"
    );

    std::fs::write(run.path().join("reached-v1.json"), "{}").expect("replace the corrupt evidence");
    std::fs::write(run.path().join("probe"), "not a directory")
        .expect("the unreadable probe directory");
    let error = gates::engine_audit(&asked(run.path(), None, None))
        .expect_err("a present probe path that cannot be traversed must fail closed");
    assert!(matches!(error, AuditError::Unreadable { .. }), "{error}");
}

#[test]
fn a_document_that_is_not_a_run_report_is_refused_by_name() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let document = with(serde_json::json!({
        "document_type": "rust-mutants/catalog"
    }));
    std::fs::write(
        directory.path().join("run-report-v1.json"),
        document.to_string(),
    )
    .expect("the document");
    let error = gates::engine_audit(&asked(directory.path(), None, None))
        .expect_err("a document that is not a run report");
    assert!(
        error.to_string().contains("rust-mutants/catalog"),
        "{error}"
    );
}

#[test]
fn a_historical_report_is_not_silently_read_as_the_current_contract() {
    let document = with(serde_json::json!({ "schema_version": 1 }));
    let run = run_directory(&document);
    let error = gates::engine_audit(&asked(run.path(), None, None))
        .expect_err("v1 and v1 assign different meanings to outcome columns");
    assert!(
        matches!(error, AuditError::UnsupportedVersion { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("Some(1)"), "{error}");
}

#[test]
fn the_report_boundary_rejects_unknown_fields_and_closed_state_values() {
    let mut unknown = base();
    unknown["mutants"][0]["outocme"] = serde_json::json!("killed");
    let run = run_directory(&unknown);
    let error = gates::engine_audit(&asked(run.path(), None, None))
        .expect_err("an unknown field must not look like ignored evidence");
    assert!(matches!(error, AuditError::OffSchema { .. }), "{error}");

    let mut open_state = base();
    open_state["mutants"][0]["outcome"] = serde_json::json!("probably-killed");
    let run = run_directory(&open_state);
    let error = gates::engine_audit(&asked(run.path(), None, None))
        .expect_err("an outcome outside the closed contract must not enter the audit");
    assert!(matches!(error, AuditError::OffSchema { .. }), "{error}");
}

#[test]
fn the_report_boundary_rejects_duplicate_keys() {
    let encoded = base().to_string();
    let key = format!("\"schema_version\":{}", xtask::engineaudit::SCHEMA_VERSION);
    let duplicated = encoded.replacen(&key, &format!("{key},{key}"), 1);
    assert_ne!(
        duplicated, encoded,
        "the duplicate was planted, so the refusal below is about it"
    );
    let error = xtask::engineaudit::audit("duplicate.json", &duplicated, &Evidence::default())
        .expect_err("a duplicate key must not silently choose a winner");
    assert!(matches!(error, AuditError::Unparsable { .. }), "{error}");
}

#[test]
fn nullable_report_fields_are_required_even_when_their_value_is_null() {
    let mut missing_score = base();
    assert!(
        missing_score
            .as_object_mut()
            .expect("the report object")
            .remove("score")
            .is_some(),
        "the fixture has a nullable score"
    );
    let error = xtask::engineaudit::audit(
        "missing-score.json",
        &missing_score.to_string(),
        &Evidence::default(),
    )
    .expect_err("a missing nullable key is different from an explicit null");
    assert!(matches!(error, AuditError::OffSchema { .. }), "{error}");

    let mut missing_route = base();
    assert!(
        missing_route["mutants"][0]
            .as_object_mut()
            .expect("the mutant object")
            .remove("route")
            .is_some(),
        "the fixture has a nullable route"
    );
    let error = xtask::engineaudit::audit(
        "missing-route.json",
        &missing_route.to_string(),
        &Evidence::default(),
    )
    .expect_err("a missing route cannot be silently read as no route");
    assert!(matches!(error, AuditError::OffSchema { .. }), "{error}");
}

#[test]
fn owned_evidence_is_exact_after_the_duplicate_key_boundary() {
    let report = base().to_string();
    for (path, reached, touched) in [
        (
            "reached-v1.json",
            Some(r#"{"targets":{},"limitations":[]}"#),
            None,
        ),
        (
            "touched-v1.json",
            None,
            Some(r#"{"targets":{},"limitations":[],"rogue":true}"#),
        ),
        (
            "touched-null.json",
            None,
            Some(r#"{"targets":{},"limitations":[],"narrowing":null}"#),
        ),
    ] {
        let evidence = Evidence {
            recorded: None,
            shards: Vec::new(),
            ledger: None,
            sites: false,
            reached: reached.map(|text| Source { path, text }),
            catalog: None,
            probe_logs: Vec::new(),
            touched: touched.map(|text| Source { path, text }),
        };
        let error = xtask::engineaudit::audit("report.json", &report, &evidence)
            .expect_err("an owned evidence document must match its exact schema");
        assert!(
            matches!(error, AuditError::MalformedEvidence { .. }),
            "{error}"
        );
    }
}

/// Every layer a planted perturbation is the business of, beyond the one it was planted for.
fn layers_of(name: &str) -> &'static [Layer] {
    match name {
        "a column the rows do not come to" => &[Layer::Score],
        "a survivor no finding names" | "a met claim on a row nobody marked" => &[Layer::Ledger],
        "a row that ran a target its route never reached"
        | "a route narrowed by guards that kept no record"
        | "a test the guards say reached a mutation and the route dropped" => &[Layer::Trace],
        _ => &[],
    }
}

#[test]
fn every_layer_is_silent_on_the_clean_run_and_loud_on_the_perturbations_that_are_its_own() {
    let clean = audited_with(&base(), &recording());
    assert_eq!(clean.violations(), 0, "{clean}");
    let mut wrong = Vec::new();
    for planted_for in Layer::ALL {
        for perturbation in planted_for.planted() {
            let laid = perturbation.lay().expect("the perturbation on disk");
            let audit = gates::engine_audit(&gates::EngineRun {
                run: laid.run(),
                trace: Some(laid.trace()),
                shards: laid.shards(),
                ledger: laid.ledger(),
                sites: true,
            })
            .expect("a report this audit can read");
            let spoke: Vec<Layer> = Layer::ALL
                .into_iter()
                .filter(|layer| audit.violated(*layer))
                .collect();
            let expected: Vec<Layer> = Layer::ALL
                .into_iter()
                .filter(|layer| {
                    *layer == planted_for || layers_of(perturbation.name).contains(layer)
                })
                .collect();
            if spoke != expected {
                wrong.push(format!(
                    "{}: expected {expected:?}, spoke {spoke:?}\n{audit}",
                    perturbation.name
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n\n"));
}

/// The runs of three fixtures, recorded by the engine and committed beside this test.
const SAMPLES: [(&str, usize, usize); 3] = [
    ("engine-run-simple", 13, 0),
    ("engine-run-rejected", 16, 4),
    ("engine-run-unreached", 8, 0),
];

fn sample(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name)
}

#[test]
fn every_committed_run_re_decides_with_nothing_the_audit_disagrees_with() {
    for (name, mutants, rejections) in SAMPLES {
        let run = sample(name);
        let trace = run.join("trace");
        let audited = gates::engine_audit(&asked(&run, Some(&trace), None));
        assert_eq!(
            result_state(&audited),
            ResultState::Returned,
            "{name}: {audited:?}"
        );
        let audit = match audited {
            Ok(audit) => audit,
            Err(_already_reported) => continue,
        };
        assert_eq!(audit.mutants, mutants, "{name}: {audit}");
        assert_eq!(audit.rejections, rejections, "{name}: {audit}");
        assert_eq!(audit.violations(), 0, "{name}: {audit}");
        assert_eq!(audit.exit_code(), undecided(&audit), "{name}");
    }
}

#[test]
fn a_committed_run_read_without_its_recording_leaves_the_trace_layer_unaudited() {
    let run = sample("engine-run-simple");
    let audit =
        gates::engine_audit(&asked(&run, None, None)).expect("a report this audit can read");
    assert_eq!(audit.violations(), 0, "{audit}");
    let remarks = audit.of(Layer::Trace);
    assert_eq!(remarks.len(), 1, "{audit}");
    assert_eq!(remarks[0].standing, Standing::Unaudited);
}

#[test]
fn a_committed_run_whose_recording_lost_a_line_is_a_violation() {
    let run = sample("engine-run-simple");
    let recorded = std::fs::read_to_string(run.join("trace/trace.jsonl")).expect("the recording");
    let mut lines: Vec<&str> = recorded.lines().collect();
    let lost = lines.remove(lines.len() / 2);
    assert!(!lost.is_empty(), "the fixture removes a non-empty event");
    let trace = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(trace.path().join("trace.jsonl"), lines.join("\n"))
        .expect("the shortened recording");
    let audit = gates::engine_audit(&asked(&run, Some(trace.path()), None))
        .expect("a report this audit can read");
    assert!(
        violations(&audit, Layer::Trace)
            .iter()
            .any(|remark| remark.contains("is missing")),
        "{audit}"
    );
}

#[test]
fn a_run_the_interruption_stopped_is_not_a_run_that_lost_its_routes() {
    let document = with(serde_json::json!({
        "run": { "interrupted": true, "exit_code": 130 },
        "accounting": {
            "cataloged": 3, "executed": 2, "not_run": 1
        },
        "mutants": [{}, {}, {
            "index": 3, "id": "c".repeat(64), "display_id": "c".repeat(20),
            "path": "src/lib.rs", "package": "demo",
            "family": "comparison", "rule": "gt-to-ge", "item": "larger",
            "rule_version": 1,
            "line": 30, "column": 8,
            "start_byte": 300, "end_byte": 301, "source_digest": SOURCE,
            "original": ">", "replacement": ">=",
            "outcome": "not_run", "target": "", "exit_code": 0,
            "duration_ms": 0, "tests_run": null, "killed_by": [], "signal": null,
            "step_notice": null, "retried": false, "lingered": false, "not_run_reason": "interrupted",
            "route": null, "identical": "not-measured",
            "expected": false, "unreached": false, "source_run_id": null
        }]
    }));
    let audit = audited_with(&document, &recording());
    let found = violations(&audit, Layer::Trace);
    assert!(
        found.is_empty(),
        "a mutation the interruption never reached is one the recording is silent about by \
         design: {found:?}"
    );
    assert!(
        audit
            .of(Layer::Trace)
            .iter()
            .any(|remark| remark.standing == Standing::Unaudited
                && remark.subject == "interrupted"),
        "and the audit says so rather than passing over it: {audit}"
    );
}

/// One run whose measurement discharged a target from one mutant.
fn discharging() -> (serde_json::Value, serde_json::Value, serde_json::Value) {
    let report = with(serde_json::json!({
        "accounting": { "killed": 1, "survived": 0, "not_run": 1, "executed": 1, "discharged": 1 },
        "score": { "detected": 1, "decided": 1, "value": 1.0 },
        "mutants": [
            {},
            {
                "outcome": "not_run",
                "target": "",
                "not_run_reason": "discharged",
                "expected": false,
                "route": {
                    "granularity": "discharged",
                    "reaching": [],
                    "discharged": [{ "target": TARGET, "proof": "branch-never-taken" }],
                    "executed": []
                }
            }
        ],
        "findings": [{ "kind": "discharged-mutant", "mutant": SURVIVED, "detail": "d" }],
        "expectations": []
    }));
    let reached = serde_json::json!({
        "targets": { TARGET: [{ "file": "src/lib.rs", "start": { "line": 3, "column": 1 },
                                "end": { "line": 3, "column": 9 } }] },
        "instrumented": [],
        "limitations": []
    });
    let catalog = serde_json::json!({
        "mutants": [{
            "display_id": short(SURVIVED),
            "path": "src/lib.rs",
            "branch": { "start_line": 10, "start_column": 5, "end_line": 12, "end_column": 5 }
        }]
    });
    (report, reached, catalog)
}

/// Audits a run whose directory also holds the evidence its proofs rest on.
fn audited_with_evidence(
    report: &serde_json::Value,
    reached: &serde_json::Value,
    catalog: &serde_json::Value,
) -> Audit {
    let directory = run_directory(report);
    std::fs::write(
        directory.path().join("reached-v1.json"),
        reached.to_string(),
    )
    .expect("the measurement");
    std::fs::write(
        directory.path().join("catalog-v1.json"),
        catalog.to_string(),
    )
    .expect("the catalog");
    gates::engine_audit(&asked(directory.path(), None, None)).expect("a report this audit can read")
}

#[test]
fn a_discharge_the_run_re_derives_is_silent() {
    let (report, reached, catalog) = discharging();
    let audit = audited_with_evidence(&report, &reached, &catalog);
    assert_eq!(
        violations(&audit, Layer::Proofs),
        Vec::<String>::new(),
        "{audit}"
    );
}

#[test]
fn a_target_discharged_by_a_branch_that_ran_the_body_is_a_violation() {
    let (report, _, catalog) = discharging();
    let ran = serde_json::json!({
        "targets": { TARGET: [{ "file": "src/lib.rs", "start": { "line": 11, "column": 9 },
                                "end": { "line": 11, "column": 20 } }] },
        "instrumented": [],
        "limitations": []
    });
    let audit = audited_with_evidence(&report, &ran, &catalog);
    assert!(
        violations(&audit, Layer::Proofs)
            .iter()
            .any(|said| said.contains("may have noticed")),
        "{audit}"
    );
}

#[test]
fn a_branch_discharge_without_the_evidence_it_rests_on_is_unaudited() {
    let (report, ..) = discharging();
    let audit = audited(&report);
    assert_eq!(violations(&audit, Layer::Proofs), Vec::<String>::new());
    assert!(
        audit
            .of(Layer::Proofs)
            .iter()
            .any(|remark| remark.standing == Standing::Unaudited),
        "a discharge whose premises the run did not keep is one nobody can check: {audit}"
    );
}

#[test]
fn the_discharged_column_equals_the_records() {
    let (report, reached, catalog) = discharging();
    let mut miscounted = report;
    miscounted["accounting"]["discharged"] = serde_json::json!(3);
    let audit = audited_with_evidence(&miscounted, &reached, &catalog);
    assert!(
        violations(&audit, Layer::Proofs)
            .iter()
            .any(|said| said.contains("3 mutants were discharged")),
        "{audit}"
    );
}

#[test]
fn a_file_whose_walk_decided_less_than_it_saw_is_a_violation() {
    let mut events = recording();
    events.insert(
        1,
        serde_json::json!({
            "seq": 0,
            "timestamp": "2026-01-01T00:00:01Z",
            "elapsed_ms": 1,
            "type": "discover-file",
            "discover": {
                "path": "src/lib.rs",
                "candidates": 2,
                "sites": [{ "line": 1, "column": 1, "rule": "gt-to-ge", "form": "C", "skip": null, "note": null }],
                "skips": []
            }
        }),
    );
    for (at, event) in events.iter_mut().enumerate() {
        event["seq"] = serde_json::json!(at.saturating_add(1));
    }
    let audit = audited_with(&base(), &events);
    assert!(
        violations(&audit, Layer::Sites)
            .iter()
            .any(|remark| remark.contains("passed over without saying so")),
        "a place the walk saw and said nothing about is the one thing a reader cannot ask the \
         engine to explain: {audit}"
    );
}

#[test]
fn a_file_passed_over_whole_is_silent_on_the_census() {
    let mut events = recording();
    events.insert(
        1,
        serde_json::json!({
            "seq": 0,
            "timestamp": "2026-01-01T00:00:01Z",
            "elapsed_ms": 1,
            "type": "discover-file",
            "discover": {
                "path": "src/testutil.rs",
                "candidates": 0,
                "sites": [],
                "skips": [{ "reason": "test-only-file", "count": 4 }]
            }
        }),
    );
    for (at, event) in events.iter_mut().enumerate() {
        event["seq"] = serde_json::json!(at.saturating_add(1));
    }
    let audit = audited_with(&base(), &events);
    assert!(
        violations(&audit, Layer::Sites).is_empty(),
        "a file nothing walked has no decisions to account for: {audit}"
    );
}

#[test]
fn a_whole_file_skip_must_name_at_least_one_hidden_place() {
    let mut events = recording();
    events.insert(
        1,
        serde_json::json!({
            "seq": 0,
            "timestamp": "2026-01-01T00:00:01Z",
            "elapsed_ms": 1,
            "type": "discover-file",
            "discover": {
                "path": "src/testutil.rs",
                "candidates": 0,
                "sites": [],
                "skips": [{ "reason": "test-only-file", "count": 0 }]
            }
        }),
    );
    for (at, event) in events.iter_mut().enumerate() {
        event["seq"] = serde_json::json!(at.saturating_add(1));
    }
    let audit = audited_with(&base(), &events);
    assert!(
        violations(&audit, Layer::Sites)
            .iter()
            .any(|remark| remark.contains("names no skipped place")),
        "an empty whole-file exception must not bypass the census: {audit}"
    );
}

#[test]
fn the_work_a_report_claims_is_the_work_its_recording_holds() {
    let audit = audited_with(&base(), &recording());
    assert!(
        violations(&audit, Layer::Work).is_empty(),
        "{:?}",
        audit.remarks
    );
    assert!(
        !audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Work && remark.standing == Standing::Unaudited),
        "the sample names its targets and its routes, so there is nothing to leave unaudited: {:?}",
        audit.remarks
    );
}

#[test]
fn a_row_that_ran_a_target_its_route_never_reached_is_a_violation() {
    let mut document = base();
    document["mutants"][0]["route"]["executed"] =
        serde_json::json!([TARGET, "demo/test/elsewhere"]);
    let audit = audited_with(&document, &recording());
    assert!(
        !violations(&audit, Layer::Work).is_empty(),
        "a process started against a target nothing routed it to is work nobody asked for: {:?}",
        audit.remarks
    );
}

#[test]
fn a_route_that_reaches_and_discharges_more_targets_than_the_run_built_is_a_violation() {
    let mut document = base();
    document["mutants"][0]["route"]["discharged"] =
        serde_json::json!([{"target": "demo/test/elsewhere", "proof": "branch-never-taken"}]);
    let audit = audited_with(&document, &recording());
    assert!(
        !violations(&audit, Layer::Work).is_empty(),
        "one target cannot be reached once and discharged once: {:?}",
        audit.remarks
    );
}

#[test]
fn a_report_that_names_no_targets_leaves_the_work_unaudited() {
    let mut document = base();
    document["targets"] = serde_json::json!([]);
    let audit = audited_with(&document, &recording());
    assert!(
        audit
            .remarks
            .iter()
            .any(|remark| remark.layer == Layer::Work && remark.standing == Standing::Unaudited),
        "a reader who cannot see how many targets there were cannot say what a whole run \
         would have cost: {:?}",
        audit.remarks
    );
    assert!(
        violations(&audit, Layer::Work).is_empty(),
        "{:?}",
        audit.remarks
    );
}

#[test]
fn a_report_that_claims_fewer_executions_than_the_recording_holds_is_a_violation() {
    let mut document = base();
    document["mutants"][1]["route"]["executed"] = serde_json::json!([]);
    let audit = audited_with(&document, &recording());
    assert!(
        !violations(&audit, Layer::Work).is_empty(),
        "the recording holds two executions and the report claims one: {:?}",
        audit.remarks
    );
}

#[test]
fn an_outcome_an_earlier_run_established_is_work_this_one_did_not_do() {
    let mut document = base();
    document["mutants"][1]["source_run_id"] = serde_json::json!("20260901T000000000Z");
    document["mutants"][1]["route"]["executed"] = serde_json::json!([]);
    let mut events = recording();
    events.retain(|event| {
        event["type"] != "mutant-exec"
            || event["mutant"]["id"] != serde_json::json!(short(SURVIVED))
    });
    let audit = audited_with(&document, &events);
    assert!(
        violations(&audit, Layer::Work).is_empty(),
        "a row an earlier run answered for started nothing, and the recording agrees: {:?}",
        audit.remarks
    );
}

#[test]
fn a_measurement_that_does_not_account_for_a_target_the_run_built_is_a_violation() {
    let reached = serde_json::json!({
        "targets": {"demo/test/elsewhere": []},
        "instrumented": [],
        "limitations": []
    });
    let audit = audited_with_evidence(&base(), &reached, &serde_json::json!({"mutants": []}));
    assert!(
        violations(&audit, Layer::Proofs)
            .iter()
            .any(|one| one.contains(TARGET)),
        "the run built {TARGET} and the measurement neither names it nor says it could not read \
         it, so a route that narrowed by this measurement narrowed by a target nobody looked \
         at: {:?}",
        audit.remarks
    );
}

#[test]
fn a_measurement_that_says_it_could_not_read_a_target_has_accounted_for_it() {
    let reached = serde_json::json!({
        "targets": {"demo/test/elsewhere": []},
        "instrumented": [],
        "limitations": [format!("coverage-not-measured:{TARGET}")]
    });
    let audit = audited_with_evidence(&base(), &reached, &serde_json::json!({"mutants": []}));
    assert!(
        !violations(&audit, Layer::Proofs)
            .iter()
            .any(|one| one.contains("nobody looked at")),
        "a measurement that says which target it could not read has accounted for it: {:?}",
        audit.remarks
    );
}

/// A run directory holding a report and the record the guards left beside it.
fn with_record(document: &serde_json::Value, record: &serde_json::Value) -> Audit {
    let directory = run_directory(document);
    std::fs::write(directory.path().join("touched-v1.json"), record.to_string())
        .expect("the record");
    gates::engine_audit(&asked(directory.path(), None, None)).expect("a report this audit can read")
}

#[test]
fn a_route_the_guards_decided_is_re_decided_from_what_they_recorded() {
    let audit = with_record(&routed_by_test(), &touched());
    assert!(
        violations(&audit, Layer::Touch).is_empty(),
        "{:?}",
        violations(&audit, Layer::Touch)
    );
    assert!(
        audit
            .of(Layer::Touch)
            .into_iter()
            .all(|remark| remark.standing != Standing::Unaudited),
        "the record is there, so nothing about it is unaudited: {:?}",
        audit.of(Layer::Touch)
    );
}

#[test]
fn a_test_the_record_says_reached_a_mutation_and_the_route_dropped_is_a_violation() {
    let mut document = routed_by_test();
    document["mutants"][0]["route"]["tests"][TARGET] = serde_json::json!([]);
    let audit = with_record(&document, &touched());
    let said = violations(&audit, Layer::Touch);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("tests::max_picks_the_larger"),
        "the violation names the test that would not have run: {said:?}"
    );
}

#[test]
fn a_target_the_record_says_reached_a_mutation_and_the_route_left_out_is_a_violation() {
    let mut document = routed_by_test();
    document["mutants"][0]["route"]["reaching"] = serde_json::json!([]);
    document["mutants"][0]["route"]["tests"] = serde_json::json!({});
    let audit = with_record(&document, &touched());
    let said = violations(&audit, Layer::Touch);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("a kill reported as a survivor"),
        "{said:?}"
    );
}

#[test]
fn a_target_the_route_keeps_that_the_record_says_reached_nothing_is_a_violation() {
    let mut document = routed_by_test();
    let other = "demo/test/parity";
    document["targets"] = serde_json::json!([
        {"id": TARGET, "kind": "lib", "harness": true, "tests": 2, "limitations": []},
        {"id": other, "kind": "test", "harness": true, "tests": 1, "limitations": []}
    ]);
    document["mutants"][0]["route"]["reaching"] = serde_json::json!([TARGET, other]);
    let mut recorded = touched();
    recorded["targets"][other] = serde_json::json!({
        "reached": {"tests": {"a_parity_test": []}, "loose": []},
        "ran": ["a_parity_test"]
    });
    let audit = with_record(&document, &recorded);
    let said = violations(&audit, Layer::Touch);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("kept for nothing"), "{said:?}");
}

#[test]
fn a_target_the_run_built_that_the_record_neither_names_nor_excuses_is_a_violation() {
    let mut document = routed_by_test();
    document["targets"] = serde_json::json!([
        {"id": TARGET, "kind": "lib", "harness": true, "tests": 2, "limitations": []},
        {"id": "demo/test/parity", "kind": "test", "harness": true, "tests": 1,
         "limitations": []}
    ]);
    let audit = with_record(&document, &touched());
    let said = violations(&audit, Layer::Touch);
    assert!(
        said.iter()
            .any(|one| one.contains("demo/test/parity") && one.contains("nobody asked")),
        "{said:?}"
    );
}

#[test]
fn a_target_the_record_excuses_is_accounted_for_rather_than_unnamed() {
    let mut document = routed_by_test();
    document["targets"] = serde_json::json!([
        {"id": TARGET, "kind": "lib", "harness": true, "tests": 2, "limitations": []},
        {"id": "demo/doc/demo", "kind": "doc", "harness": true, "tests": 1,
         "limitations": []}
    ]);
    let mut recorded = touched();
    recorded["limitations"] = serde_json::json!(["touch-not-recorded:demo/doc/demo"]);
    let audit = with_record(&document, &recorded);
    assert!(
        violations(&audit, Layer::Touch).is_empty(),
        "{:?}",
        violations(&audit, Layer::Touch)
    );
}

#[test]
fn a_run_that_narrowed_by_the_guards_and_kept_no_record_of_them_is_a_violation() {
    let audit = audited(&routed_by_test());
    let said = violations(&audit, Layer::Touch);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("kept no record"), "{said:?}");
}

#[test]
fn a_run_that_narrowed_by_nothing_the_guards_said_has_nothing_of_theirs_to_re_decide() {
    let audit = audited(&base());
    assert!(
        audit.of(Layer::Touch).is_empty(),
        "{:?}",
        audit.of(Layer::Touch)
    );
}

/// A run whose guards discharged the second mutant from the target, by the proof named.
fn discharged_by_the_guards(proof: &str) -> serde_json::Value {
    with(serde_json::json!({
        "accounting": { "killed": 1, "survived": 0, "not_run": 1, "executed": 1, "discharged": 1 },
        "score": { "detected": 1, "decided": 1, "value": 1.0 },
        "mutants": [
            {},
            {
                "outcome": "not_run",
                "target": "",
                "not_run_reason": "discharged",
                "expected": false,
                "route": {
                    "granularity": "discharged",
                    "reaching": [],
                    "discharged": [{ "target": TARGET, "proof": proof }],
                    "executed": []
                }
            }
        ],
        "findings": [{ "kind": "discharged-mutant", "mutant": SURVIVED, "detail": "d" }],
        "expectations": []
    }))
}

/// The record those guards left: one test reached both mutations and saw only the first one's branches part.
fn record_of_a_comparison(infected: &[u32]) -> serde_json::Value {
    serde_json::json!({
        "targets": { TARGET: {
            "reached": {"tests": {"tests::max_picks_the_larger": [0, 1]}, "loose": []},
            "infected": {"tests": {"tests::max_picks_the_larger": infected}, "loose": []},
            "ran": ["tests::max_picks_the_larger"]
        }},
        "limitations": [],
        "narrowing": { "compared": [0, 1], "bodies": {} }
    })
}

#[test]
fn a_never_infected_discharge_the_guards_support_needs_no_probe_log() {
    let audit = with_record(
        &discharged_by_the_guards("never-infected"),
        &record_of_a_comparison(&[0]),
    );
    assert_eq!(
        violations(&audit, Layer::Proofs),
        Vec::<String>::new(),
        "{audit}"
    );
    assert!(
        !audit
            .of(Layer::Proofs)
            .iter()
            .any(|remark| remark.to_string().contains("probe whose log")),
        "the guards recorded it, so nothing is left over for the probe to answer: {audit}"
    );
}

#[test]
fn a_never_infected_discharge_the_guards_contradict_is_a_violation() {
    let audit = with_record(
        &discharged_by_the_guards("never-infected"),
        &record_of_a_comparison(&[0, 1]),
    );
    assert!(
        violations(&audit, Layer::Proofs)
            .iter()
            .any(|said| said.contains("answered differently")),
        "{audit}"
    );
}

#[test]
fn a_never_infected_discharge_of_a_mutant_no_guard_compares_still_asks_for_the_probe() {
    let mut record = record_of_a_comparison(&[0]);
    record["narrowing"]["compared"] = serde_json::json!([0]);
    let audit = with_record(&discharged_by_the_guards("never-infected"), &record);
    assert!(
        audit
            .of(Layer::Proofs)
            .iter()
            .any(|remark| remark.standing == Standing::Unaudited
                && remark.to_string().contains("probe whose log")),
        "silence about a guard that never compared is not evidence: {audit}"
    );
}

/// The record for a branch proof: the marker mutant 1 rests on, and which tests entered that body.
fn record_of_a_body(entered: &[u32]) -> serde_json::Value {
    serde_json::json!({
        "targets": { TARGET: {
            "reached": {"tests": {"tests::max_picks_the_larger": [0, 1]}, "loose": []},
            "bodies": {"tests": {"tests::max_picks_the_larger": entered}, "loose": []},
            "ran": ["tests::max_picks_the_larger"]
        }},
        "limitations": [],
        "narrowing": { "compared": [], "bodies": { "1": 1 } }
    })
}

#[test]
fn a_branch_discharge_the_guards_support_needs_no_coverage_build() {
    let audit = with_record(
        &discharged_by_the_guards("branch-never-taken"),
        &record_of_a_body(&[]),
    );
    assert_eq!(
        violations(&audit, Layer::Proofs),
        Vec::<String>::new(),
        "{audit}"
    );
    assert!(
        !audit
            .of(Layer::Proofs)
            .iter()
            .any(|remark| remark.to_string().contains("did not keep")),
        "the marker is exact where a region is inferred: {audit}"
    );
}

#[test]
fn a_branch_discharge_the_guards_contradict_is_a_violation() {
    let audit = with_record(
        &discharged_by_the_guards("branch-never-taken"),
        &record_of_a_body(&[1]),
    );
    assert!(
        violations(&audit, Layer::Proofs)
            .iter()
            .any(|said| said.contains("entered the body")),
        "{audit}"
    );
}

/// The same record with a third test, so a mutation two of them reached is a route through a filter rather than the whole target.
fn record_of_three() -> serde_json::Value {
    let mut recorded = touched();
    recorded["targets"][TARGET]["reached"]["tests"] = serde_json::json!({
        "tests::max_picks_the_larger": [0, 1],
        "tests::min_picks_the_smaller": [1]
    });
    recorded["targets"][TARGET]["infected"] = serde_json::json!({
        "tests": { "tests::max_picks_the_larger": [0, 1] }, "loose": []
    });
    recorded["targets"][TARGET]["ran"] = serde_json::json!([
        "tests::max_picks_the_larger",
        "tests::min_picks_the_smaller",
        "tests::is_even_is_even"
    ]);
    recorded["narrowing"] = serde_json::json!({ "compared": [1], "bodies": {} });
    recorded
}

#[test]
fn a_route_the_guards_narrowed_by_a_comparison_is_re_decided_from_it() {
    let mut document = routed_by_test();
    document["targets"] = serde_json::json!([{ "id": TARGET, "kind": "lib", "harness": true, "tests": 3,
                             "limitations": [] }]);
    document["mutants"][1]["route"]["tests"][TARGET] =
        serde_json::json!(["tests::max_picks_the_larger"]);
    let recorded = record_of_three();
    let audit = with_record(&document, &recorded);
    assert!(
        violations(&audit, Layer::Touch).is_empty(),
        "a test that reached the site and never saw the two branches part is one the route \
         may leave out: {:?}",
        violations(&audit, Layer::Touch)
    );

    document["mutants"][1]["route"]["tests"][TARGET] = serde_json::json!([
        "tests::max_picks_the_larger",
        "tests::min_picks_the_smaller"
    ]);
    let audit = with_record(&document, &recorded);
    assert!(
        !violations(&audit, Layer::Touch).is_empty(),
        "and a route that keeps it anyway disagrees with the record"
    );
}

#[test]
fn a_target_every_test_of_which_reached_a_mutation_is_asked_whole_rather_than_narrowed() {
    let mut document = routed_by_test();
    document["mutants"][1]["route"]["tests"] = serde_json::json!({});
    let mut recorded = touched();
    recorded["targets"][TARGET]["reached"]["tests"] = serde_json::json!({
        "tests::max_picks_the_larger": [0, 1],
        "tests::min_picks_the_smaller": [1]
    });
    recorded["targets"][TARGET]["infected"] = serde_json::json!({
        "tests": { "tests::max_picks_the_larger": [1] }, "loose": []
    });
    recorded["narrowing"] = serde_json::json!({ "compared": [1], "bodies": {} });
    let audit = with_record(&document, &recorded);
    assert!(
        violations(&audit, Layer::Touch).is_empty(),
        "asking for the whole of a target is asking for the same tests through one fewer \
         question, and a run that did it is not a run that dropped a test: {:?}",
        violations(&audit, Layer::Touch)
    );
}

#[test]
fn a_mutant_a_proof_removed_is_a_finding_of_its_own_and_not_one_nobody_ran() {
    let audit = with_record(
        &discharged_by_the_guards("never-infected"),
        &record_of_a_comparison(&[0]),
    );
    assert!(
        violations(&audit, Layer::Findings).is_empty(),
        "a run that says a proof removed it has reported what it found: {:?}",
        violations(&audit, Layer::Findings)
    );

    let mut document = discharged_by_the_guards("never-infected");
    document["findings"][0]["kind"] = serde_json::json!("not-run-mutant");
    let audit = with_record(&document, &record_of_a_comparison(&[0]));
    let said = violations(&audit, Layer::Findings);
    assert!(
        said.iter().any(|one| one.contains("discharged-mutant")),
        "a proof that removed it is not the same hole as a mutant nothing ran: {said:?}"
    );
    assert!(
        said.iter().any(|one| one.contains("not-run-mutant")),
        "{said:?}"
    );
}

#[test]
fn a_mutant_a_filter_left_out_is_accounted_for_rather_than_reported_as_a_hole() {
    let mut document = discharged_by_the_guards("never-infected");
    document["accounting"]["discharged"] = serde_json::json!(0);
    document["mutants"][1]["not_run_reason"] = serde_json::json!("unselected");
    document["mutants"][1]["route"] = serde_json::json!({
        "granularity": "all", "fallback": null, "reaching": [], "discharged": [],
        "executed": [], "tests": {}
    });
    document["findings"] = serde_json::json!([]);
    let audit = with_record(&document, &record_of_a_comparison(&[0]));
    assert!(
        violations(&audit, Layer::Findings).is_empty(),
        "a mutant nobody selected is not a gap in the tests: {:?}",
        violations(&audit, Layer::Findings)
    );
}

#[test]
fn a_filter_decision_needs_a_select_record_and_no_route_for_an_unvalidated_mutant() {
    let mut document = base();
    document["accounting"] = serde_json::json!({
        "cataloged": 2, "refused": 1, "skipped": 0, "executed": 1,
        "killed": 1, "survived": 0, "step_limit_reached": 0, "waited": 0,
        "inconclusive": 0, "errored": 0, "not_run": 1, "unreached": 0, "discharged": 0,
        "expected": 0
    });
    document["score"] = serde_json::json!({"detected": 1, "decided": 1, "value": 1.0});
    document["mutants"][1]["outcome"] = serde_json::json!("not_run");
    document["mutants"][1]["target"] = serde_json::json!("");
    document["mutants"][1]["exit_code"] = serde_json::json!(0);
    document["mutants"][1]["duration_ms"] = serde_json::json!(0);
    document["mutants"][1]["tests_run"] = serde_json::Value::Null;
    document["mutants"][1]["expected"] = serde_json::json!(false);
    document["mutants"][1]["not_run_reason"] = serde_json::json!("unselected");
    document["mutants"][1]["route"] = serde_json::Value::Null;
    document["expectations"] = serde_json::json!([]);

    let survivor = short(SURVIVED);
    let mut events = recording();
    events.retain(|event| {
        let named = event
            .pointer("/route/mutant")
            .or_else(|| event.pointer("/mutant/id"))
            .and_then(serde_json::Value::as_str);
        named != Some(survivor.as_str())
    });
    let end = events.pop().expect("run end");
    events.push(serde_json::json!({
        "timestamp":"2026-09-06T10:15:02Z", "elapsed_ms":62,
        "type":"select", "select":{"mutant":survivor,"reason":"unselected"}
    }));
    events.push(end);
    let emitted = u64::try_from(events.len()).expect("event count");
    for (at, event) in events.iter_mut().enumerate() {
        event["seq"] = serde_json::json!(u64::try_from(at + 1).expect("sequence"));
    }
    events.last_mut().expect("run end")["run"]["events_emitted"] = serde_json::json!(emitted);

    let audit = audited_with(&document, &events);
    assert!(
        violations(&audit, Layer::Trace).is_empty(),
        "selection is the entire auditable decision for a candidate that was never placed in the build: {audit}"
    );
    assert!(
        violations(&audit, Layer::Proofs).is_empty(),
        "the select record agrees with the report: {audit}"
    );
}

#[test]
fn the_audit_reads_the_version_and_the_standings_the_schema_file_names() {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(include_str!(
        "../../schema/rust-mutants-run-report-v1.json"
    ))
    .expect("the committed run-report schema");
    assert_eq!(
        schema["properties"]["schema_version"]["const"].as_u64(),
        Some(xtask::engineaudit::SCHEMA_VERSION),
        "the audit re-decides the version of the report the schema file describes, and is kept \
         apart from the engine's constant so that it stays a second reading"
    );
    let named: std::collections::BTreeSet<&str> = schema
        .pointer("/properties/expectations/items/properties/standing/enum")
        .and_then(serde_json::Value::as_array)
        .expect("the standing enum")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let known: std::collections::BTreeSet<&str> =
        xtask::engineaudit::CLAIM_STANDINGS.into_iter().collect();
    assert_eq!(
        known, named,
        "every standing the schema allows is one the audit knows how to re-decide"
    );
}

/// The committed simple run, with its touched record rewritten by `edit`, re-decided.
fn simple_with_touched(edit: impl FnOnce(&mut serde_json::Value)) -> Audit {
    let committed = sample("engine-run-simple");
    let run = tempfile::tempdir().expect("a temporary directory");
    for name in [
        "run-report-v1.json",
        "catalog-v1.json",
        "reached-v1.json",
        "touched-v1.json",
    ] {
        std::fs::copy(committed.join(name), run.path().join(name)).expect("a committed document");
    }
    let path = run.path().join("touched-v1.json");
    let mut touched: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&path).expect("the touched record"),
    )
    .expect("a touched document");
    edit(&mut touched);
    std::fs::write(&path, touched.to_string()).expect("the edited record");
    gates::engine_audit(&asked(run.path(), Some(&committed.join("trace")), None))
        .expect("a report this audit can read")
}

#[test]
fn a_committed_run_is_held_to_the_items_its_tests_entered() {
    let whole = simple_with_touched(|_| {});
    assert!(violations(&whole, Layer::Entry).is_empty(), "{whole}");
    assert!(
        whole.of(Layer::Entry).is_empty(),
        "the committed run keeps its item catalog, so nothing about entry is left unaudited: \
         {whole}"
    );
    let lost = simple_with_touched(|touched| {
        let targets = touched["targets"]
            .as_object_mut()
            .expect("the record names targets");
        for target in targets.values_mut() {
            target["entered"] = serde_json::json!({});
        }
    });
    let found = violations(&lost, Layer::Entry);
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("reached it, and the entry markers say")),
        "a site the guards saw reached inside an item nobody entered: {lost}"
    );
    assert!(
        found
            .iter()
            .any(|remark| remark.contains("noticed it, and the entry markers say")),
        "and a kill by a test that never entered the item: {lost}"
    );
}

#[test]
fn a_committed_run_whose_item_catalog_is_gone_leaves_entry_unaudited_rather_than_passed() {
    let audit = simple_with_touched(|touched| {
        touched["items"] = serde_json::json!([]);
    });
    assert!(violations(&audit, Layer::Entry).is_empty(), "{audit}");
    assert!(
        audit
            .of(Layer::Entry)
            .iter()
            .any(|remark| remark.standing == Standing::Unaudited),
        "{audit}"
    );
}

#[test]
fn a_mutant_named_after_an_item_the_catalog_does_not_hold_it_in_is_a_violation() {
    let audit = simple_with_touched(|touched| {
        let items = touched["items"]
            .as_array_mut()
            .expect("the record keeps an item catalog");
        for item in items {
            if item["name"] == "max" {
                item["name"] = serde_json::json!("min");
            }
        }
    });
    assert!(
        violations(&audit, Layer::Entry)
            .iter()
            .any(|remark| remark.contains("the catalog names the item holding it min")),
        "{audit}"
    );
}
/// The exit code an audit with no violation earns: 3 where it left anything unaudited, 0 only where it decided everything.
fn undecided(audit: &Audit) -> u8 {
    if audit.unaudited() > 0 {
        xtask::proofaudit::EXIT_UNAUDITED
    } else {
        0
    }
}
