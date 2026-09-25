// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A clean synthetic run, and the defects planted in it that each layer of the audit must find.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::{DISPLAY_ID_LENGTH, Layer, REPORT_FILE};

/// The run the specimen report names.
pub const RUN: &str = "20260906T101500000Z";
/// The digest of the one source file every row of the specimen names.
pub const SOURCE: &str = "2a97516c354b68848cdbd8f54a226a0a55b21ed138e207ad6c5cbb9c00aa5aea";
/// The identity of the specimen's killed mutant.
pub const KILLED: &str = "676ca631e0dc6d6a4fa8600edae6fd22b0ee35079ce9ce7be7374f1941cbbeeb";
/// The identity of the specimen's accepted survivor.
pub const SURVIVED: &str = "982217d08c9594367534ff67757c56928cf48418d50f0ca60902eb035ffb7676";
/// The identity of the candidate the compiler refused.
pub const REFUSED: &str = "7ba2a9ab76a444e683ca3d96c2afa11c0e0b5189737fca5603c36533701b5b5e";
/// The one target the specimen run built.
pub const TARGET: &str = "demo/lib/demo";

/// The short form a person types of `id`.
#[must_use]
pub fn short(id: &str) -> String {
    id.chars().take(DISPLAY_ID_LENGTH).collect()
}

/// The row of the mutant a test noticed.
fn killed() -> Value {
    json!({
        "index": 0, "id": KILLED, "display_id": short(KILLED),
        "path": "src/lib.rs", "package": "demo",
        "family": "comparison", "rule": "gt-to-ge", "item": "larger",
        "rule_version": 1,
        "line": 11, "column": 8,
        "start_byte": 100, "end_byte": 101, "source_digest": SOURCE,
        "original": ">", "replacement": ">=",
        "outcome": "killed", "target": TARGET, "exit_code": 101,
        "duration_ms": 7, "tests_run": 2, "killed_by": ["larger_works"],
        "signal": null, "step_notice": null, "retried": false, "lingered": false,
        "not_run_reason": null,
        "route": {"granularity": "block", "fallback": null,
            "reaching": [TARGET], "discharged": [], "executed": [TARGET], "tests": {}},
        "identical": "not-measured", "expected": false, "unreached": false,
        "source_run_id": null
    })
}

/// The row of the survivor a reviewer accepted.
fn survived() -> Value {
    json!({
        "index": 1, "id": SURVIVED, "display_id": short(SURVIVED),
        "path": "src/lib.rs", "package": "demo",
        "family": "return-replacement", "rule": "return-default", "item": "larger",
        "rule_version": 1,
        "line": 20, "column": 5,
        "start_byte": 200, "end_byte": 225, "source_digest": SOURCE,
        "original": "if a > b { a } else { b }", "replacement": "Default::default()",
        "outcome": "survived", "target": TARGET, "exit_code": 0,
        "duration_ms": 5, "tests_run": 2, "killed_by": [], "signal": null,
        "step_notice": null, "retried": false, "lingered": false, "not_run_reason": null,
        "route": {"granularity": "block", "fallback": null,
            "reaching": [TARGET], "discharged": [], "executed": [TARGET], "tests": {}},
        "identical": "not-measured", "expected": true, "unreached": false,
        "source_run_id": null
    })
}

/// A run of three candidates: one killed, one accepted survivor, one the compiler refused.
#[must_use]
pub fn base() -> Value {
    json!({
        "document_type": "rust-mutants/run-report",
        "schema_version": super::SCHEMA_VERSION,
        "tool_version": "0.1.0",
        "run": {
            "id": RUN,
            "started_at": "2026-09-06T10:15:00Z",
            "finished_at": "2026-09-06T10:15:02Z",
            "duration_ms": 2000,
            "interrupted": false,
            "exit_code": 0,
            "shard": null,
            "jobs": {"asked": "auto", "used": 1}
        },
        "workspace": {
            "root_name": "demo",
            "toolchain": "rustc 1.98.0",
            "workspace_digest": "d".repeat(64),
            "catalog_digest": "c".repeat(64),
            "platform": { "os": "linux", "arch": "x86_64", "target": "x86_64-unknown-linux-gnu" }
        },
        "selection": {
            "tier": "balanced", "operators": [], "include": [], "exclude": [], "packages": [],
            "build": [], "mutant_steps": 50_000_000
        },
        "targets": [{
            "id": TARGET, "kind": "lib", "harness": true, "tests": 2, "limitations": []
        }],
        "established_tests": 0,
        "accounting": {
            "cataloged": 2, "refused": 1, "skipped": 0, "executed": 2,
            "killed": 1, "survived": 1, "step_limit_reached": 0, "waited": 0,
            "inconclusive": 0, "errored": 0, "not_run": 0, "unreached": 0,
            "discharged": 0, "expected": 1
        },
        "score": { "detected": 1, "decided": 2, "value": 0.5 },
        "mutants": [killed(), survived()],
        "rejections": [
            {
                "index": 2, "id": REFUSED, "display_id": short(REFUSED),
                "path": "src/lib.rs", "rule": "add-to-sub",
                "code": "E0369", "diagnostic": "error[E0369]: cannot subtract",
                "isolated": true
            }
        ],
        "skips": [],
        "expectations": [
            {
                "id": SURVIVED, "reason": "the bound is equivalent under the invariant",
                "outcome": "survived", "mutant": SURVIVED,
                "locator": null, "covered": null,
                "standing": "met", "actual": "survived", "why": null
            }
        ],
        "findings": []
    })
}

/// The recording of that run: one route and one execution each, a build and a verify, one round that condemned the refusal.
#[must_use]
pub fn recording() -> Vec<Value> {
    let mut events = vec![
        json!({"seq":1,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":0,
            "type":"run-start","schema":"rust-mutants-trace-v1","engine":"0.1.0",
            "context":{"kind":"standalone","run_id":"engine-audit-specimen",
            "build_selection":"c5a587d94348b75388f86ec2495002bcecf82b4abb21333627414c945c0746ed"}}),
        json!({"seq":2,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":0,
            "type":"phase-start","phase":{"name":"prepare","duration_ms":null}}),
        json!({"seq":3,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":10,
            "type":"instrument","instrument":{"path":"src/lib.rs","guards":2,
            "module":"__rm_deadbeef","lines_before":40,"lines_after":40}}),
        json!({"seq":4,"timestamp":"2026-09-06T10:15:00Z","elapsed_ms":20,
            "type":"validate-round","round":{"round":1,"condemned":0,"success":false,
            "attributed":[{"index":2,"code":"E0369","said":"cannot subtract"}],
            "written":1,"unattributed":[]}}),
        json!({"seq":5,"timestamp":"2026-09-06T10:15:01Z","elapsed_ms":30,
            "type":"build","build":{"targets":[TARGET],
            "details":[{"id":TARGET,"kind":"lib","harness":true,"limitations":[]}]}}),
        json!({"seq":6,"timestamp":"2026-09-06T10:15:01Z","elapsed_ms":40,
            "type":"verify","verify":{"target":TARGET,"outcome":"survived","tests_run":2,
            "duration_ms":5,"remembered":false,"retried":false}}),
        json!({"seq":7,"timestamp":"2026-09-06T10:15:01Z","elapsed_ms":50,
            "type":"phase-end","phase":{"name":"prepare","duration_ms":50}}),
    ];
    events.extend(judged((8, 9), 0, KILLED, "killed"));
    events.extend(judged((10, 11), 1, SURVIVED, "survived"));
    events.push(json!({"seq":12,"timestamp":"2026-09-06T10:15:02Z",
        "elapsed_ms":70,"type":"run-end","run":{"outcome":"detected","error":null,
        "events_emitted":12,"events_dropped":0}}));
    events
}

/// The route and the execution of one mutant, at the two sequence numbers given.
fn judged(seq: (u64, u64), index: u64, id: &str, outcome: &str) -> [Value; 2] {
    let exit = if outcome == "killed" { 101 } else { 0 };
    [
        json!({"seq":seq.0,"timestamp":"2026-09-06T10:15:02Z",
            "elapsed_ms":60,"type":"route","route":{"mutant":short(id),
            "index":index,"granularity":"block",
            "reaching":[TARGET],"executed":[TARGET]}}),
        json!({"seq":seq.1,"timestamp":"2026-09-06T10:15:02Z",
            "elapsed_ms":61,"type":"mutant-exec","mutant":{"id":short(id),
            "index":index,"target":TARGET,"outcome":outcome,
            "exit_code":exit,"duration_ms":5,"tests_run":2,"lingered":false}}),
    ]
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

/// The ledger that accepts each of `ids` as an equivalent survivor.
#[must_use]
pub fn ledger(ids: &[&str]) -> String {
    ids.iter()
        .map(|id| {
            format!(
                "[[mutation.expect]]\nid = \"{id}\"\nreason = \"equivalent\"\noutcome = \"survived\"\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A specimen could not be laid out on disk.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpecimenError {
    /// No temporary directory to lay it in.
    #[error("a temporary directory to lay the engine-audit specimen in: {source}")]
    Directory {
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// One file of it could not be written.
    #[error("{path}: the engine-audit specimen could not be written: {source}")]
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
    /// One event of the recording lacks a field of the envelope every event carries.
    #[error("event {at} of the specimen recording has no `{field}` to put in its envelope")]
    Envelope {
        /// Its position in the recording.
        at: usize,
        /// The missing field.
        field: &'static str,
    },
}

impl crate::error::Coded for SpecimenError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Directory { .. } | Self::Unwritable { .. } => {
                crate::error::XtCode::SpecimenUnwritable
            }
            Self::NotAnObject { .. } | Self::Envelope { .. } => crate::error::XtCode::SpecimenEvent,
        }
    }
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

/// A recording directory holding `events` as its `trace.jsonl`, each wrapped in the envelope the engine writes.
///
/// # Errors
/// [`SpecimenError`] when an event lacks an envelope field, or the recording cannot be written.
pub fn recorded(events: &[Value]) -> Result<TempDir, SpecimenError> {
    let mut stream = String::new();
    for (at, event) in events.iter().enumerate() {
        let mut payload = event
            .as_object()
            .cloned()
            .ok_or(SpecimenError::NotAnObject { at })?;
        let mut taken = |field: &'static str| {
            payload
                .remove(field)
                .ok_or(SpecimenError::Envelope { at, field })
        };
        let seq = taken("seq")?;
        let timestamp = taken("timestamp")?;
        let elapsed_ms = taken("elapsed_ms")?;
        crate::specimen::completed(crate::schemas::Producer::Engine, &mut payload);
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

/// One run for the audit to re-decide: a report, its recording, and the evidence beside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Perturbation {
    /// What a refusal calls it.
    pub name: &'static str,
    /// The report.
    pub document: Value,
    /// The recording, as events before their envelope.
    pub events: Vec<Value>,
    /// The reports of the other parts of this catalog.
    pub shards: Vec<Value>,
    /// The ledger of accepted survivors, as the configuration file holds it.
    pub ledger: Option<String>,
    /// Documents the run keeps beside its report, by file name.
    pub beside: Vec<(&'static str, Value)>,
}

/// The clean specimen every perturbation starts from, on which no layer may find anything.
#[must_use]
pub fn clean() -> Perturbation {
    Perturbation {
        name: "clean",
        document: base(),
        events: recording(),
        shards: Vec::new(),
        ledger: Some(ledger(&[SURVIVED])),
        beside: Vec::new(),
    }
}

/// A perturbation laid out on disk, alive for as long as the audit reads it.
#[derive(Debug)]
pub struct Laid {
    run: TempDir,
    trace: TempDir,
    #[expect(
        dead_code,
        reason = "held so the directory the shard and ledger paths point into lives as long as they do"
    )]
    aside: TempDir,
    shards: Vec<PathBuf>,
    ledger: Option<PathBuf>,
}

impl Laid {
    /// The run directory.
    #[must_use]
    pub fn run(&self) -> &Path {
        self.run.path()
    }

    /// The recording directory.
    #[must_use]
    pub fn trace(&self) -> &Path {
        self.trace.path()
    }

    /// The shard reports.
    #[must_use]
    pub fn shards(&self) -> &[PathBuf] {
        &self.shards
    }

    /// The ledger, when the perturbation carries one.
    #[must_use]
    pub fn ledger(&self) -> Option<&Path> {
        self.ledger.as_deref()
    }
}

impl Perturbation {
    /// Writes the run directory, the recording, every shard, and the ledger to fresh temporary directories.
    ///
    /// # Errors
    /// [`SpecimenError`] when any of them cannot be written.
    pub fn lay(&self) -> Result<Laid, SpecimenError> {
        let run = run_directory(&self.document)?;
        for (file, document) in &self.beside {
            written(&run.path().join(file), &document.to_string())?;
        }
        let trace = recorded(&self.events)?;
        let aside = directory()?;
        let mut shards = Vec::new();
        for (at, shard) in self.shards.iter().enumerate() {
            let path = aside.path().join(format!("shard-{at}.json"));
            written(&path, &shard.to_string())?;
            shards.push(path);
        }
        let ledger = self
            .ledger
            .as_deref()
            .map(|text| {
                let path = aside.path().join(".rust-mutants.toml");
                written(&path, text).map(|()| path)
            })
            .transpose()?;
        Ok(Laid {
            run,
            trace,
            aside,
            shards,
            ledger,
        })
    }
}

/// The clean recording with `overrides` laid over the event at `at`.
fn amended(at: usize, overrides: Value) -> Vec<Value> {
    let mut events = recording();
    if let Some(event) = events.get_mut(at) {
        merge(event, overrides);
    }
    events
}

/// The clean recording with `event` put at `at` and every sequence number counted again.
fn inserted(at: usize, event: Value) -> Vec<Value> {
    let mut events = recording();
    events.insert(at.min(events.len()), event);
    for (seq, event) in (1_u64..).zip(events.iter_mut()) {
        merge(event, json!({ "seq": seq }));
    }
    events
}

/// A report whose second mutant a measurement discharged from the one target.
fn discharged() -> Value {
    with(json!({
        "accounting": { "killed": 1, "survived": 0, "not_run": 1, "executed": 1, "discharged": 1,
                        "expected": 0 },
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
        "expectations": [],
        "run": { "exit_code": 1 }
    }))
}

/// The recording of [`discharged`]: the second mutant's route removed its one target, and nothing ran it.
fn discharging() -> Vec<Value> {
    let mut events = recording();
    events.retain(|event| {
        event.get("type") != Some(&json!("mutant-exec"))
            || event.pointer("/mutant/id") != Some(&json!(short(SURVIVED)))
    });
    for event in &mut events {
        if event.pointer("/route/mutant") == Some(&json!(short(SURVIVED))) {
            merge(
                event,
                json!({ "route": {
                    "granularity": "discharged",
                    "reaching": [],
                    "executed": [],
                    "discharged": [{ "target": TARGET, "proof": "branch-never-taken" }]
                } }),
            );
        }
    }
    for (seq, event) in (1_u64..).zip(events.iter_mut()) {
        merge(event, json!({ "seq": seq }));
        if event.get("type") == Some(&json!("run-end")) {
            merge(event, json!({ "run": { "events_emitted": seq } }));
        }
    }
    events
}

/// A report whose two mutants are each put to one of the target's two tests.
#[must_use]
pub fn routed_by_test() -> Value {
    with(json!({
        "mutants": [
            {"route": {"granularity": "test", "reaching": [TARGET], "executed": [TARGET],
                       "tests": {TARGET: ["tests::max_picks_the_larger"]}}},
            {"route": {"granularity": "test", "reaching": [TARGET], "executed": [TARGET],
                       "tests": {TARGET: ["tests::min_picks_the_smaller"]}}}
        ]
    }))
}

/// The record the guards left for [`routed_by_test`]: each test reached one of the two mutations.
#[must_use]
pub fn touched() -> Value {
    json!({
        "targets": {
            TARGET: {
                "reached": {
                    "tests": {
                        "tests::max_picks_the_larger": [0],
                        "tests::min_picks_the_smaller": [1]
                    },
                    "loose": []
                },
                "ran": ["tests::max_picks_the_larger", "tests::min_picks_the_smaller"]
            }
        },
        "limitations": []
    })
}

/// The record the guards left for the clean run, with the item both of its mutants sit in and `entered` as given.
#[must_use]
pub fn entered(entered: &Value, measurable: bool) -> Value {
    json!({
        "targets": {
            TARGET: {
                "reached": { "tests": { "larger_works": [0, 1] } },
                "entered": entered,
                "ran": ["larger_works", "smaller_works"]
            }
        },
        "limitations": [],
        "items": [{
            "index": 0, "package": "demo", "path": "src/lib.rs", "name": "larger",
            "span": { "start": 50, "end": 300 }, "body": { "start": 60, "end": 290 },
            "measurable": measurable
        }]
    })
}

impl Layer {
    /// The defects planted for this layer, each of which it must report as a violation.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "one total match holds every layer's planted defects, so a layer added without one does not compile"
    )]
    pub fn planted(self) -> Vec<Perturbation> {
        let clean = clean();
        match self {
            Self::Identity => vec![Perturbation {
                name: "an identity that does not re-mint",
                document: with(json!({ "mutants": [{ "start_byte": 104 }] })),
                ..clean
            }],
            Self::Accounting => vec![Perturbation {
                name: "a column the rows do not come to",
                document: with(json!({ "accounting": { "killed": 2 } })),
                ..clean
            }],
            Self::Score => vec![Perturbation {
                name: "a score that is not its own ratio",
                document: with(json!({ "score": { "detected": 1, "decided": 2, "value": 0.9 } })),
                ..clean
            }],
            Self::Findings => vec![Perturbation {
                name: "a survivor no finding names",
                document: with(json!({
                    "accounting": { "expected": 0 },
                    "mutants": [{}, { "expected": false }],
                    "expectations": []
                })),
                ..clean
            }],
            Self::Expectations => vec![Perturbation {
                name: "a met claim on a row nobody marked",
                document: with(json!({
                    "accounting": { "expected": 0 },
                    "mutants": [{}, { "expected": false }],
                    "findings": [{ "kind": "surviving-mutant", "mutant": short(SURVIVED),
                                   "detail": "no test noticed it" }],
                    "run": { "exit_code": 1 }
                })),
                ..clean
            }],
            Self::Exit => vec![Perturbation {
                name: "an exit code that does not follow",
                document: with(json!({ "run": { "exit_code": 1 } })),
                ..clean
            }],
            Self::Merge => vec![Perturbation {
                name: "a mutant in two parts of one catalog",
                shards: vec![base()],
                ..clean
            }],
            Self::Proofs => vec![
                Perturbation {
                    name: "a measurement that does not account for a target the run built",
                    beside: vec![
                        (
                            "reached-v1.json",
                            json!({
                                "targets": {"demo/test/elsewhere": []},
                                "instrumented": [],
                                "limitations": []
                            }),
                        ),
                        ("catalog-v1.json", json!({ "mutants": [] })),
                    ],
                    ..clean.clone()
                },
                Perturbation {
                    name: "a branch discharge whose body the target ran",
                    document: discharged(),
                    events: discharging(),
                    ledger: Some(ledger(&[])),
                    beside: vec![
                        (
                            "reached-v1.json",
                            json!({
                                "targets": { TARGET: [{ "file": "src/lib.rs",
                                    "start": { "line": 11, "column": 9 },
                                    "end": { "line": 11, "column": 20 } }] },
                                "instrumented": [],
                                "limitations": []
                            }),
                        ),
                        (
                            "catalog-v1.json",
                            json!({ "mutants": [{
                                "display_id": short(SURVIVED),
                                "path": "src/lib.rs",
                                "branch": { "start_line": 10, "start_column": 5,
                                            "end_line": 12, "end_column": 5 }
                            }] }),
                        ),
                    ],
                    ..clean
                },
            ],
            Self::Sites => vec![Perturbation {
                name: "a file whose walk decided less than it saw",
                events: inserted(
                    1,
                    json!({
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
                ),
                ..clean
            }],
            Self::Trace => vec![
                Perturbation {
                    name: "an instrumentation that moved a line",
                    events: amended(2, json!({ "instrument": { "lines_after": 41 } })),
                    ..clean.clone()
                },
                Perturbation {
                    name: "an execution that disagrees with its row",
                    events: amended(8, json!({ "mutant": { "outcome": "survived" } })),
                    ..clean
                },
            ],
            Self::Ledger => vec![Perturbation {
                name: "an acceptance the run does not hold",
                ledger: Some(ledger(&[SURVIVED, &"d".repeat(64)])),
                ..clean
            }],
            Self::Work => vec![Perturbation {
                name: "a row that ran a target its route never reached",
                document: with(json!({
                    "mutants": [{ "route": { "executed": [TARGET, "demo/test/elsewhere"] } }]
                })),
                ..clean
            }],
            Self::Touch => vec![
                Perturbation {
                    name: "a route narrowed by guards that kept no record",
                    document: routed_by_test(),
                    ..clean.clone()
                },
                Perturbation {
                    name: "a test the guards say reached a mutation and the route dropped",
                    document: {
                        let mut document = routed_by_test();
                        merge(
                            &mut document,
                            json!({ "mutants": [{ "route": { "tests": { TARGET: [] } } }] }),
                        );
                        document
                    },
                    beside: vec![("touched-v1.json", touched())],
                    ..clean
                },
            ],
            Self::Entry => vec![
                Perturbation {
                    name: "a test that noticed a mutation and never entered the item it is in",
                    beside: vec![(
                        "touched-v1.json",
                        entered(&json!({ "tests": { "smaller_works": [0] } }), true),
                    )],
                    ..clean.clone()
                },
                Perturbation {
                    name: "a site reached inside an item nothing can record entering",
                    beside: vec![(
                        "touched-v1.json",
                        entered(&json!({ "tests": { "larger_works": [0] } }), false),
                    )],
                    ..clean
                },
            ],
        }
    }
}
