// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A clean synthetic run, and the defects planted in it that each layer of the audit must find.

use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::{Layer, REPORT_FILE};

/// The run the specimen report names.
pub const RUN: &str = "20260906T101500Z-9f1c2d";
/// The display identity of the specimen's killed mutant.
pub const KILLED: &str = "aaaaaaaaaaaaaaaaaaaa";
/// The display identity of the specimen's survivor, which the one finding names.
pub const SURVIVED: &str = "bbbbbbbbbbbbbbbbbbbb";
/// The one target the specimen run tested with.
pub const TARGET: &str = "pkg/test/lib";
/// The question about the status of the one exchange [`went_past`] holds.
pub const ASKED: &str = "f714f108a1ce93e4cae5d149115f5f2efc4d4ceb620ccecc762f1c4b914022ed";

/// A scoped run of two mutants, one killed and one survivor its finding names, over one target that passed.
#[must_use]
pub fn base() -> Value {
    json!({
        "schema": "njutest-assurance-report-v1",
        "schema_version": 2,
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
                "step_limit_reached": 0,
                "waited": 0,
                "unreached": 0,
                "equivalent": 0,
                "accepted": 0,
                "reused_killed": 0,
                "reused_survived": 0,
                "model_noticed": 0,
                "model_proved": 0
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
                "decision": {
                    "outcome": "killed", "killed_by": TARGET, "step_boundary": null
                },
                "accepted": false,
                "reuse": { "reused": false, "source_run_id": null }
            },
            {
                "id": "b".repeat(64),
                "display_id": SURVIVED,
                "path": "src/lib.rs",
                "position": { "line": 11, "column": 5, "character_column": 5 },
                "rule": "return-ok-default@1",
                "decision": {
                    "outcome": "survived", "killed_by": null, "step_boundary": null
                },
                "accepted": false,
                "reuse": { "reused": false, "source_run_id": null }
            }
        ],
        "models": [],
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

/// Lays `overrides` over `document`: objects key by key, a non-empty array position by position, anything else by replacement.
pub fn merge(document: &mut Value, overrides: Value) {
    match (document, overrides) {
        (Value::Object(into), Value::Object(from)) => {
            for (key, value) in from {
                merge(into.entry(key).or_insert(Value::Null), value);
            }
        }
        (Value::Array(into), Value::Array(from)) if !from.is_empty() => {
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

/// The clean report with `overrides` laid over it.
#[must_use]
pub fn with(overrides: Value) -> Value {
    let mut document = base();
    merge(&mut document, overrides);
    document
}

/// A route and an execution for each mutant of [`base`], with no proof removing anything.
#[must_use]
pub fn routes() -> Vec<Value> {
    let mut events = unconfirmed_routes();
    events.extend(confirmation(&"a".repeat(64), "passed", Some("killed")));
    numbered(events)
}

/// The control and the confirmation of `mutant`'s kill by [`TARGET`]: the control's `answer`, and what the second run came to.
#[must_use]
pub fn confirmation(mutant: &str, answer: &str, reproduced: Option<&str>) -> Vec<Value> {
    let answer = if answer == "passed" {
        json!({ "kind": "passed" })
    } else {
        json!({ "kind": "failed", "detail": answer })
    };
    vec![
        json!({
            "timestamp": "2026-09-06T00:00:02Z", "elapsed_ms": 2,
            "type": "control",
            "control": { "target": TARGET, "test": null, "asked_for": mutant, "answer": answer }
        }),
        json!({
            "timestamp": "2026-09-06T00:00:03Z", "elapsed_ms": 3,
            "type": "confirm",
            "confirm": {
                "mutant": mutant, "target": TARGET, "test": null, "expected": "killed",
                "answered_for": mutant, "reproduced": reproduced
            }
        }),
    ]
}

/// [`routes`] without the confirmation its kill rests on.
fn unconfirmed_routes() -> Vec<Value> {
    let mut events = Vec::new();
    for (mutant, outcome) in [(KILLED, "killed"), (SURVIVED, "survived")] {
        events.push(json!({
            "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0,
            "type": "route",
            "route": {
                "mutant": mutant, "granularity": "block", "fallback": null,
                "reaching": ["t1"], "discharged": [], "considered": [], "reused": null
            }
        }));
        events.push(json!({
            "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1,
            "type": "mutant-exec",
            "mutant": {
                "mutant": mutant, "target": "t1", "args": [], "outcome": outcome,
                "duration_ms": 5
            }
        }));
    }
    events
}

/// `events` with every sequence number counted again from one.
fn numbered(mut events: Vec<Value>) -> Vec<Value> {
    for (seq, event) in (1_u64..).zip(events.iter_mut()) {
        merge(event, json!({ "seq": seq }));
    }
    events
}

/// The recording of [`routes`], after which one target, `blunt`, was put to both mutations and noticed neither.
#[must_use]
pub fn never_noticed() -> Vec<Value> {
    let mut events = routes();
    for mutant in [KILLED, SURVIVED] {
        events.push(json!({
            "timestamp": "2026-09-06T00:00:02Z",
            "elapsed_ms": 2,
            "type": "mutant-exec",
            "mutant": {
                "mutant": mutant, "target": "blunt", "args": [], "outcome": "survived",
                "duration_ms": 5
            }
        }));
    }
    numbered(events)
}

/// One exchange that went past the `api` seam, which licenses the question [`ASKED`] among others.
#[must_use]
pub fn went_past() -> Value {
    json!({
        "type": "wire-exchange",
        "exchange": {
            "capability": "api",
            "seq": 0,
            "wire": "http",
            "method": "GET",
            "path": "/orders",
            "status": 200
        }
    })
}

/// One fault put to the suite, decided as `decision` says.
#[must_use]
pub fn was_put(fault: &str, decision: &str) -> Value {
    json!({
        "type": "wire-exec",
        "wire": {
            "fault": fault,
            "capability": "api",
            "seq": 0,
            "rule": "status-server-error",
            "decision": decision,
            "noticed_by": null
        }
    })
}

/// One touch record the engine writes about `target`, measured on `measured` over the one test `lib::works`, having reached `reached`.
#[must_use]
pub fn touch(measured: &str, reached: &[u32]) -> Value {
    json!({
        "type": "touch",
        "touch": {
            "target": TARGET,
            "measured": measured,
            "tests": 1,
            "sites": reached.len(),
            "loose": 0,
            "infected": 0,
            "passed": ["lib::works"],
            "summary": { "protocol": "libtest", "tests_run": 1 },
            "reached_sites": reached,
            "entered_bodies": [],
            "infected_sites": []
        }
    })
}

/// The report's drift record about [`TARGET`], in the standing `state` names.
#[must_use]
pub fn drifted(state: &str) -> Value {
    json!({ "drift": [{ "target": TARGET, "state": state }] })
}

/// A specimen could not be laid out on disk.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpecimenError {
    /// No temporary directory to lay it in.
    #[error("a temporary directory to lay the proofaudit specimen in: {source}")]
    Directory {
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// One file of it could not be written.
    #[error("{path}: the proofaudit specimen could not be written: {source}")]
    Unwritable {
        /// The file.
        path: String,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// One event of the recording is not a JSON object.
    #[error("event {at} of the specimen recording is not an object")]
    NotAnObject {
        /// Its position in the recording.
        at: usize,
    },
}

fn directory() -> Result<TempDir, SpecimenError> {
    tempfile::tempdir().map_err(|source| SpecimenError::Directory { source })
}

fn written(path: &Path, text: &str) -> Result<(), SpecimenError> {
    std::fs::write(path, text).map_err(|source| SpecimenError::Unwritable {
        path: path.display().to_string(),
        source,
    })
}

/// A run directory holding `document` as its report.
///
/// # Errors
/// [`SpecimenError`] when the directory or the report cannot be written.
pub fn run_directory(document: &Value) -> Result<TempDir, SpecimenError> {
    let run = directory()?;
    written(&run.path().join(REPORT_FILE), &document.to_string())?;
    Ok(run)
}

/// A recording directory holding `events` as its `trace.jsonl`, each wrapped in the envelope the runner writes, numbered by position where an event carries no envelope of its own.
///
/// # Errors
/// [`SpecimenError`] when an event is not an object, or the recording cannot be written.
pub fn recorded(events: &[Value]) -> Result<TempDir, SpecimenError> {
    let mut stream = String::new();
    for ((at, event), position) in events.iter().enumerate().zip(1_u64..) {
        let mut payload = event
            .as_object()
            .cloned()
            .ok_or(SpecimenError::NotAnObject { at })?;
        let seq = payload.remove("seq").unwrap_or_else(|| json!(position));
        let timestamp = payload
            .remove("timestamp")
            .unwrap_or_else(|| json!("2026-09-06T00:00:00Z"));
        let elapsed_ms = payload.remove("elapsed_ms").unwrap_or_else(|| json!(at));
        let envelope = json!({
            "seq": seq,
            "timestamp": timestamp,
            "elapsed_ms": elapsed_ms,
            "payload": Value::Object(payload),
        });
        stream.push_str(&envelope.to_string());
        stream.push('\n');
    }
    let trace = directory()?;
    written(&trace.path().join("trace.jsonl"), &stream)?;
    Ok(trace)
}

/// One run for the audit to re-decide: a report, and the recording beside it when the run kept one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Perturbation {
    /// What a refusal calls it.
    pub name: &'static str,
    /// The report.
    pub document: Value,
    /// The recording, as events before their envelope, or nothing for a run recorded without `--trace`.
    pub events: Option<Vec<Value>>,
    /// The one configured build's engine recording, as events before their envelope, or nothing where the run kept none.
    pub engine: Option<Vec<Value>>,
}

/// The clean specimen every perturbation starts from, on which no layer may find anything.
#[must_use]
pub fn clean() -> Perturbation {
    Perturbation {
        name: "clean",
        document: with(drifted("held")),
        events: Some(routes()),
        engine: Some(vec![touch("baseline", &[0, 1]), touch("control", &[0, 1])]),
    }
}

/// A perturbation laid out on disk, alive for as long as the audit reads it.
#[derive(Debug)]
pub struct Laid {
    run: TempDir,
    trace: Option<TempDir>,
}

impl Laid {
    /// The run directory.
    #[must_use]
    pub fn run(&self) -> &Path {
        self.run.path()
    }

    /// The recording directory, when the perturbation carries a recording.
    #[must_use]
    pub fn trace(&self) -> Option<&Path> {
        self.trace.as_ref().map(TempDir::path)
    }
}

impl Perturbation {
    /// Writes the run directory and the recording to fresh temporary directories.
    ///
    /// # Errors
    /// [`SpecimenError`] when either cannot be written.
    pub fn lay(&self) -> Result<Laid, SpecimenError> {
        let run = run_directory(&self.document)?;
        let trace = self.events.as_deref().map(recorded).transpose()?;
        if let (Some(trace), Some(engine)) = (trace.as_ref(), self.engine.as_deref()) {
            let laid = recorded(engine)?;
            let namespace = trace.path().join("builds").join("0000000000");
            std::fs::create_dir_all(&namespace).map_err(|source| SpecimenError::Unwritable {
                path: namespace.display().to_string(),
                source,
            })?;
            let into = namespace.join("engine");
            std::fs::rename(laid.path(), &into).map_err(|source| SpecimenError::Unwritable {
                path: into.display().to_string(),
                source,
            })?;
        }
        Ok(Laid { run, trace })
    }
}

/// The recording of a kill by a target the route's proof had discharged.
fn discharged_then_killed() -> Vec<Value> {
    vec![
        json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0,
            "type": "route",
            "route": {
                "mutant": KILLED, "granularity": "block", "fallback": null,
                "reaching": ["t1"],
                "discharged": [{ "target": "t2", "proof": "never-infected" }],
                "considered": [], "reused": null
            }
        }),
        json!({
            "seq": 2, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1,
            "type": "mutant-exec",
            "mutant": {
                "mutant": KILLED, "target": "t2", "args": [], "outcome": "killed",
                "duration_ms": 5
            }
        }),
    ]
}

/// The defects planted for the confirmation layer: a kill with no confirmation, one whose test failed on the original code too, and one that did not come back.
fn confirmation_plants(clean: &Perturbation) -> Vec<Perturbation> {
    let mut failed_control = unconfirmed_routes();
    failed_control.extend(confirmation(
        &"a".repeat(64),
        "failed: also on the original",
        None,
    ));
    let mut not_reproduced = unconfirmed_routes();
    not_reproduced.extend(confirmation(&"a".repeat(64), "passed", Some("survived")));
    vec![
        Perturbation {
            name: "a kill the recording holds no confirmation of",
            events: Some(numbered(unconfirmed_routes())),
            ..clean.clone()
        },
        Perturbation {
            name: "a kill whose test failed on the original code too",
            events: Some(numbered(failed_control)),
            ..clean.clone()
        },
        Perturbation {
            name: "a kill that did not come back the second time",
            events: Some(numbered(not_reproduced)),
            ..clean.clone()
        },
    ]
}

impl Layer {
    /// The defects planted for this layer, each of which it must report as a violation.
    #[must_use]
    pub fn planted(self) -> Vec<Perturbation> {
        let clean = clean();
        match self {
            Self::Accounting => vec![Perturbation {
                name: "a mutant column the records contradict",
                document: with(json!({ "accounting": { "mutants": { "killed": 5 } } })),
                ..clean
            }],
            Self::Killers => vec![Perturbation {
                name: "a kill that names no target at all",
                document: with(json!({ "mutants": [{ "decision": { "killed_by": null } }] })),
                ..clean
            }],
            Self::Findings => vec![Perturbation {
                name: "a survivor no finding names",
                document: with(json!({ "findings": [] })),
                ..clean
            }],
            Self::Acceptances => vec![Perturbation {
                name: "an unmatched acceptance the whole catalog resolves",
                document: with(json!({
                    "findings": [{
                        "kind": "unmatched-acceptance",
                        "subject": "aaaa",
                        "detail": "no single mutant",
                        "position": null
                    }]
                })),
                ..clean
            }],
            Self::Reuse => vec![Perturbation {
                name: "a reused disposition that names no source run",
                document: with(json!({
                    "mutants": [{ "reuse": { "reused": true } }],
                    "accounting": { "mutants": { "reused_killed": 1 } }
                })),
                ..clean
            }],
            Self::Proofs => vec![Perturbation {
                name: "a kill by a target a proof discharged",
                events: Some(discharged_then_killed()),
                ..clean
            }],
            Self::Hollow => vec![Perturbation {
                name: "a hollow target the report does not name",
                events: Some(never_noticed()),
                ..clean
            }],
            Self::Wire => vec![Perturbation {
                name: "a question nothing noticed that the report does not name",
                events: Some(vec![went_past(), was_put(ASKED, "unnoticed")]),
                ..clean
            }],
            Self::Model => vec![Perturbation {
                name: "a verified-v1 survivor with no model record",
                document: with(json!({ "contract": "verified-v1" })),
                ..clean
            }],
            Self::Confirmations => confirmation_plants(&clean),
            Self::Drift => vec![Perturbation {
                name: "a control that reached a site its baseline never did, recorded as held",
                engine: Some(vec![touch("baseline", &[0]), touch("control", &[0, 1])]),
                ..clean
            }],
            Self::Executions => [
                "killed",
                "unconfirmed",
                "waited",
                "unreached",
                "errored",
                "equivalent",
            ]
            .into_iter()
            .filter_map(lie)
            .collect(),
        }
    }
}

/// How one outcome is told about the specimen's survivor: the lie's name, whether a test is named, the column that counts it, and the finding it owes.
type Telling = (
    &'static str,
    bool,
    Option<&'static str>,
    Option<&'static str>,
);

/// How [`lie`] tells `outcome`, or nothing where the schema has no such outcome.
fn telling(outcome: &str) -> Option<Telling> {
    Some(match outcome {
        "killed" => ("a survivor reported as killed", true, Some("killed"), None),
        "unconfirmed" => (
            "a survivor reported as unconfirmed",
            true,
            None,
            Some("failing-test"),
        ),
        "errored" => (
            "a survivor reported as errored",
            true,
            None,
            Some("failing-test"),
        ),
        "waited" => (
            "a survivor reported as waited",
            true,
            Some("waited"),
            Some("waited-mutant"),
        ),
        "step-limit-reached" => (
            "a survivor reported as stopped at its step limit",
            true,
            Some("step_limit_reached"),
            Some("step-limit-reached-mutant"),
        ),
        "unreached" => (
            "a survivor reported as unreached",
            false,
            Some("unreached"),
            Some("surviving-mutant"),
        ),
        "equivalent" => (
            "a survivor reported as equivalent",
            false,
            Some("equivalent"),
            None,
        ),
        "compile-rejected" => (
            "a survivor reported as compile-rejected",
            false,
            Some("rejected"),
            None,
        ),
        "model-noticed" => (
            "a survivor reported as model-noticed",
            false,
            Some("model_noticed"),
            None,
        ),
        "model-proved" => (
            "a survivor reported as model-proved",
            false,
            Some("model_proved"),
            None,
        ),
        _ => return None,
    })
}

/// The clean run with its kill reported as a survivor, its columns and findings made to agree.
fn kill_reported_as_survivor() -> Perturbation {
    let clean = clean();
    let mut document = clean.document.clone();
    merge(
        &mut document,
        json!({
            "accounting": { "mutants": { "killed": 0, "survived": 2 } },
            "mutants": [{ "decision": { "outcome": "survived", "killed_by": null } }],
            "findings": [
                {
                    "kind": "surviving-mutant",
                    "subject": SURVIVED,
                    "detail": "no test noticed return-ok-default@1 at src/lib.rs:11",
                    "position": null
                },
                {
                    "kind": "surviving-mutant",
                    "subject": KILLED,
                    "detail": "no test noticed negate-condition@1 at src/lib.rs:7",
                    "position": null
                }
            ]
        }),
    );
    Perturbation {
        name: "a kill reported as a survivor",
        document,
        ..clean
    }
}

/// The clean run with its survivor reported as `outcome`, every column, finding and the verdict made to agree, and the recording left saying it survived; nothing where the schema has no such outcome.
///
/// A lie told consistently is the one an audit that only counts cannot see: the report contradicts nothing but the executions it rests on.
#[must_use]
pub fn lie(outcome: &str) -> Option<Perturbation> {
    if outcome == "survived" {
        return Some(kill_reported_as_survivor());
    }
    let (name, noticed, column, finding) = telling(outcome)?;
    let mut columns = serde_json::Map::new();
    columns.insert("survived".to_owned(), json!(0));
    if let Some(column) = column {
        columns.insert(
            column.to_owned(),
            json!(if column == "killed" { 2 } else { 1 }),
        );
    }
    if matches!(outcome, "unreached" | "equivalent" | "compile-rejected") {
        columns.insert("executed".to_owned(), json!(1));
    }
    let clean = clean();
    let mut document = clean.document.clone();
    merge(
        &mut document,
        json!({
            "verdict": if finding.is_some() { "INSUFFICIENT" } else { "SCOPE_ASSURED" },
            "accounting": { "mutants": columns },
            "mutants": [{}, {
                "decision": {
                    "outcome": outcome,
                    "killed_by": if noticed { json!(TARGET) } else { json!(null) },
                    "step_boundary": null
                }
            }],
            "findings": []
        }),
    );
    if let Some(kind) = finding {
        merge(
            &mut document,
            json!({ "findings": [{
                "kind": kind,
                "subject": SURVIVED,
                "detail": "planted",
                "position": null
            }] }),
        );
    }
    Some(Perturbation {
        name,
        document,
        ..clean
    })
}
