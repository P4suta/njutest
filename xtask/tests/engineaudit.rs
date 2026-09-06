// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gate that re-decides a completed engine run without asking the engine whether it agrees with itself.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads as a table"
)]

use std::path::Path;

use xtask::engineaudit::{Audit, AuditError, EXIT_UNREADABLE, Layer, Standing};
use xtask::gates;

const RUN: &str = "20260906T101500000Z";
const SOURCE: &str = "2a97516c354b68848cdbd8f54a226a0a55b21ed138e207ad6c5cbb9c00aa5aea";
const KILLED: &str = "676ca631e0dc6d6a4fa8600edae6fd22b0ee35079ce9ce7be7374f1941cbbeeb";
const SURVIVED: &str = "982217d08c9594367534ff67757c56928cf48418d50f0ca60902eb035ffb7676";
const REFUSED: &str = "7ba2a9ab76a444e683ca3d96c2afa11c0e0b5189737fca5603c36533701b5b5e";
const TARGET: &str = "demo/lib/demo";

fn short(id: &str) -> String {
    id.chars().take(20).collect()
}

/// A run of three candidates: one killed, one accepted survivor, one the compiler refused.
fn base() -> serde_json::Value {
    serde_json::json!({
        "document_type": "rust-mutants/run-report",
        "schema_version": 1,
        "tool_version": "0.1.0",
        "run": {
            "id": RUN,
            "started_at": "2026-09-06T10:15:00Z",
            "finished_at": "2026-09-06T10:15:02Z",
            "duration_ms": 2000,
            "interrupted": false,
            "exit_code": 0
        },
        "workspace": {
            "root_name": "demo",
            "toolchain": "rustc 1.98.0",
            "workspace_digest": "w".repeat(64),
            "catalog_digest": "c".repeat(64),
            "platform": { "os": "linux", "arch": "x86_64", "target": "x86_64-unknown-linux-gnu" }
        },
        "selection": {
            "tier": "balanced", "operators": [], "include": [], "exclude": [], "packages": []
        },
        "accounting": {
            "cataloged": 2, "refused": 1, "skipped": 0, "executed": 2,
            "killed": 1, "survived": 1, "timed_out": 0, "inconclusive": 0,
            "errored": 0, "not_run": 0, "unreached": 0, "expected": 1
        },
        "score": { "detected": 1, "decided": 2, "value": 0.5 },
        "mutants": [
            {
                "index": 0, "id": KILLED, "display_id": short(KILLED),
                "path": "src/lib.rs", "package": "demo",
                "family": "comparison", "rule": "gt-to-ge", "rule_version": 1,
                "line": 11, "column": 8,
                "start_byte": 100, "end_byte": 101, "source_digest": SOURCE,
                "original": ">", "replacement": ">=",
                "outcome": "killed", "target": TARGET, "exit_code": 101,
                "duration_ms": 7, "tests_run": 2, "retried": false,
                "expected": false, "unreached": false, "source_run_id": null
            },
            {
                "index": 1, "id": SURVIVED, "display_id": short(SURVIVED),
                "path": "src/lib.rs", "package": "demo",
                "family": "return-replacement", "rule": "return-default", "rule_version": 1,
                "line": 20, "column": 5,
                "start_byte": 200, "end_byte": 225, "source_digest": SOURCE,
                "original": "if a > b { a } else { b }", "replacement": "Default::default()",
                "outcome": "survived", "target": TARGET, "exit_code": 0,
                "duration_ms": 5, "tests_run": 2, "retried": false,
                "expected": true, "unreached": false, "source_run_id": null
            }
        ],
        "rejections": [
            {
                "index": 2, "id": REFUSED, "display_id": short(REFUSED),
                "path": "src/lib.rs", "rule": "add-to-sub",
                "code": "E0369", "diagnostic": "error[E0369]: cannot subtract"
            }
        ],
        "skips": [],
        "expectations": [
            {
                "id": SURVIVED, "reason": "the bound is equivalent under the invariant",
                "outcome": "survived", "mutant": SURVIVED,
                "standing": "met", "actual": "survived", "why": null
            }
        ],
        "findings": []
    })
}

/// The recording of that run: one route and one execution each, a build and a verify, one round that condemned the refusal.
fn recording() -> Vec<serde_json::Value> {
    let mut events = vec![
        serde_json::json!({"seq":1,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":0,
            "type":"run-start","schema":"rust-mutants-trace-v1","engine":"0.1.0"}),
        serde_json::json!({"seq":2,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":0,
            "type":"phase-start","phase":{"name":"prepare"}}),
        serde_json::json!({"seq":3,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":10,
            "type":"instrument","instrument":{"path":"src/lib.rs","guards":2,
            "runtime":"__rm_deadbeef","lines_before":40,"lines_after":40}}),
        serde_json::json!({"seq":4,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":20,
            "type":"validate-round","round":{"round":1,"condemned":0,"success":false,
            "attributed":[{"index":2,"code":"E0369","said":"cannot subtract"}],
            "unattributed":0}}),
        serde_json::json!({"seq":5,"timestamp":"2026-09-06T10:15:01Z","elapsed_ms":30,
            "type":"build","build":{"targets":[TARGET]}}),
        serde_json::json!({"seq":6,"timestamp":"2026-09-06T10:15:01Z","elapsed_ms":40,
            "type":"verify","verify":{"target":TARGET,"outcome":"passed","tests_run":2,
            "duration_ms":5}}),
        serde_json::json!({"seq":7,"timestamp":"2026-09-06T10:15:01Z","elapsed_ms":50,
            "type":"phase-end","phase":{"name":"prepare","duration_ms":50}}),
    ];
    let mut seq = 8u64;
    for (index, (id, outcome)) in [(KILLED, "killed"), (SURVIVED, "survived")]
        .into_iter()
        .enumerate()
    {
        let exit = if outcome == "killed" { 101 } else { 0 };
        events.push(
            serde_json::json!({"seq":seq,"timestamp":"2026-09-06T10:15:02Z",
            "elapsed_ms":60,"type":"route","route":{"mutant":short(id),
            "index":index,"granularity":"block",
            "reaching":[TARGET],"executed":[TARGET]}}),
        );
        seq = seq.saturating_add(1);
        events.push(
            serde_json::json!({"seq":seq,"timestamp":"2026-09-06T10:15:02Z",
            "elapsed_ms":61,"type":"mutant-exec","mutant":{"id":short(id),
            "index":index,"target":TARGET,"outcome":outcome,
            "exit_code":exit,"duration_ms":5,"tests_run":2}}),
        );
        seq = seq.saturating_add(1);
    }
    events.push(
        serde_json::json!({"seq":seq,"timestamp":"2026-09-06T10:15:02Z",
        "elapsed_ms":70,"type":"run-end","run":{"outcome":"detected",
        "events_emitted":seq,"events_dropped":0}}),
    );
    events
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

fn run_directory(document: &serde_json::Value) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        directory.path().join("run-report-v1.json"),
        document.to_string(),
    )
    .expect("the report");
    directory
}

fn recorded(events: &[serde_json::Value]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let mut stream = String::new();
    for event in events {
        stream.push_str(&event.to_string());
        stream.push('\n');
    }
    std::fs::write(directory.path().join("trace.jsonl"), stream).expect("the recording");
    directory
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
    assert_eq!(audit.exit_code(), 0, "{audit}");
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
fn a_row_without_byte_offsets_leaves_identity_unaudited() {
    let mut document = base();
    let row = document["mutants"][0]
        .as_object_mut()
        .expect("the first row");
    let _removed = row.remove("start_byte");
    let audit = audited(&document);
    assert_eq!(violations(&audit, Layer::Identity), Vec::<String>::new());
    assert!(
        audit
            .of(Layer::Identity)
            .iter()
            .any(|remark| remark.standing == Standing::Unaudited),
        "{audit}"
    );
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
    let _dropped = events.pop();
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
    let _closed = events.remove(6);
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
    assert_eq!(audited_with(&base(), &recording()).exit_code(), 0);
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
fn a_document_that_is_not_a_run_report_is_refused_by_name() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        directory.path().join("run-report-v1.json"),
        serde_json::json!({ "document_type": "rust-mutants/catalog" }).to_string(),
    )
    .expect("the document");
    let error = gates::engine_audit(&asked(directory.path(), None, None))
        .expect_err("a document that is not a run report");
    assert!(
        error.to_string().contains("rust-mutants/catalog"),
        "{error}"
    );
}

/// One perturbation of the clean run, and every layer it is the business of.
struct Perturbation {
    name: &'static str,
    layers: &'static [Layer],
    document: serde_json::Value,
    events: Vec<serde_json::Value>,
}

fn perturbations() -> Vec<Perturbation> {
    let moved_line = {
        let mut events = recording();
        events[2]["instrument"]["lines_after"] = serde_json::json!(41);
        events
    };
    let disagreeing = {
        let mut events = recording();
        events[8]["mutant"]["outcome"] = serde_json::json!("survived");
        events
    };
    vec![
        Perturbation {
            name: "an identity that does not re-mint",
            layers: &[Layer::Identity],
            document: with(serde_json::json!({ "mutants": [{ "start_byte": 104 }] })),
            events: recording(),
        },
        Perturbation {
            name: "a column the rows do not come to",
            layers: &[Layer::Accounting, Layer::Score],
            document: with(serde_json::json!({ "accounting": { "killed": 2 } })),
            events: recording(),
        },
        Perturbation {
            name: "a score that is not its own ratio",
            layers: &[Layer::Score],
            document: with(
                serde_json::json!({ "score": { "detected": 1, "decided": 2, "value": 0.9 } }),
            ),
            events: recording(),
        },
        Perturbation {
            name: "a survivor no finding names",
            layers: &[Layer::Findings],
            document: with(serde_json::json!({
                "accounting": { "expected": 0 },
                "mutants": [{}, { "expected": false }],
                "expectations": []
            })),
            events: recording(),
        },
        Perturbation {
            name: "a met claim on a row nobody marked",
            layers: &[Layer::Expectations],
            document: with(serde_json::json!({
                "accounting": { "expected": 0 },
                "mutants": [{}, { "expected": false }],
                "findings": [{ "kind": "surviving-mutant", "mutant": short(SURVIVED),
                               "detail": "no test noticed it" }],
                "run": { "exit_code": 1 }
            })),
            events: recording(),
        },
        Perturbation {
            name: "an exit code that does not follow",
            layers: &[Layer::Exit],
            document: with(serde_json::json!({ "run": { "exit_code": 1 } })),
            events: recording(),
        },
        Perturbation {
            name: "an instrumentation that moved a line",
            layers: &[Layer::Trace],
            document: base(),
            events: moved_line,
        },
        Perturbation {
            name: "an execution that disagrees with its row",
            layers: &[Layer::Trace],
            document: base(),
            events: disagreeing,
        },
    ]
}

#[test]
fn every_layer_is_silent_on_the_clean_run_and_loud_on_the_perturbations_that_are_its_own() {
    let clean = audited_with(&base(), &recording());
    assert_eq!(clean.violations(), 0, "{clean}");
    for perturbation in perturbations() {
        let audit = audited_with(&perturbation.document, &perturbation.events);
        for layer in Layer::ALL {
            let expected = perturbation.layers.contains(&layer);
            assert_eq!(
                audit.violated(layer),
                expected,
                "{}: {} {} about it: {audit}",
                perturbation.name,
                layer.label(),
                if expected { "said nothing" } else { "spoke" }
            );
        }
    }
}

/// The runs of three fixtures, recorded by the engine and committed beside this test.
const SAMPLES: [(&str, usize, usize); 3] = [
    ("engine-run-simple", 6, 0),
    ("engine-run-rejected", 8, 4),
    ("engine-run-unreached", 4, 0),
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
        let audit = gates::engine_audit(&asked(&run, Some(&trace), None))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(audit.mutants, mutants, "{name}: {audit}");
        assert_eq!(audit.rejections, rejections, "{name}: {audit}");
        assert_eq!(audit.violations(), 0, "{name}: {audit}");
        assert_eq!(audit.exit_code(), 0, "{name}");
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
    let _lost = lines.remove(lines.len() / 2);
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
