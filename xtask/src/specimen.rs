// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Completing a specimen the sentinels lay with what its test leaves out, so it is on its schema; never reached from a reader, which a gate holds.

use serde_json::Value;

use crate::schemas::Producer;

/// The neutral value of every required field a specimen may leave out, by producer, event type and record: the value a run writes when the field says nothing.
///
/// Identity fields, an index or a target, are never here: a specimen that leaves one out is off its schema, and says so.
const NEUTRAL: [Neutral; 5] = [
    Neutral {
        producer: Producer::Runner,
        event: "route",
        record: "route",
        fields: &[
            ("fallback", NeutralValue::Null),
            ("reaching", NeutralValue::Empty),
            ("tests", NeutralValue::Empty),
            ("discharged", NeutralValue::Empty),
            ("considered", NeutralValue::Empty),
            ("reused", NeutralValue::Null),
            ("refused", NeutralValue::Null),
        ],
    },
    Neutral {
        producer: Producer::Runner,
        event: "mutant-exec",
        record: "mutant",
        fields: &[
            ("args", NeutralValue::Empty),
            ("step_boundary", NeutralValue::Null),
            ("duration_ms", NeutralValue::Zero),
            ("alone", NeutralValue::False),
        ],
    },
    Neutral {
        producer: Producer::Engine,
        event: "route",
        record: "route",
        fields: &[
            ("fallback", NeutralValue::Null),
            ("reaching", NeutralValue::Empty),
            ("discharged", NeutralValue::Empty),
            ("considered", NeutralValue::Empty),
            ("executed", NeutralValue::Empty),
            ("reused", NeutralValue::Null),
        ],
    },
    Neutral {
        producer: Producer::Engine,
        event: "mutant-exec",
        record: "mutant",
        fields: &[
            ("alone", NeutralValue::False),
            ("duration_ms", NeutralValue::Zero),
            ("exit_code", NeutralValue::Zero),
            ("failed_tests", NeutralValue::Empty),
            ("signal", NeutralValue::Null),
            ("step_notice", NeutralValue::Null),
            ("tests_run", NeutralValue::Null),
            ("timeout_ms", NeutralValue::Zero),
            ("timeout_source", NeutralValue::Configured),
        ],
    },
    Neutral {
        producer: Producer::Engine,
        event: "verify",
        record: "verify",
        fields: &[
            ("duration_ms", NeutralValue::Zero),
            ("remembered", NeutralValue::False),
            ("retried", NeutralValue::False),
            ("tests_run", NeutralValue::Null),
        ],
    },
];

/// The neutral fields of one record of one event type of one producer.
#[derive(Debug, Clone, Copy)]
struct Neutral {
    producer: Producer,
    event: &'static str,
    record: &'static str,
    fields: &'static [(&'static str, NeutralValue)],
}

/// A value that says nothing.
#[derive(Debug, Clone, Copy)]
enum NeutralValue {
    Null,
    Empty,
    Zero,
    False,
    Configured,
}

impl NeutralValue {
    fn value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Empty => Value::Array(Vec::new()),
            Self::Zero => Value::from(0_u8),
            Self::False => Value::Bool(false),
            Self::Configured => Value::from("configured"),
        }
    }
}

/// Completes a specimen `payload` of `producer` with the neutral value of every required field it leaves out that the neutral table names, so a specimen says only what its test is about and is still on its schema.
pub(crate) fn completed(producer: Producer, payload: &mut serde_json::Map<String, Value>) {
    let Some(kind) = payload
        .get("type")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return;
    };
    for neutral_record in NEUTRAL {
        if neutral_record.producer != producer || neutral_record.event != kind {
            continue;
        }
        if let Some(Value::Object(inner)) = payload.get_mut(neutral_record.record) {
            for (name, neutral) in neutral_record.fields {
                inner
                    .entry((*name).to_owned())
                    .or_insert_with(|| neutral.value());
            }
        }
    }
}

/// The keys of a flat specimen that belong to the report rather than to its one part.
const ENVELOPE: [&str; 6] = [
    "schema",
    "schema_version",
    "run_id",
    "run_kind",
    "contract",
    "scope",
];

/// The keys of a flat specimen no complete report holds: what a run concluded is the recording's `run-end`, never the document's.
const CONCLUDED: [&str; 2] = ["verdict", "models"];

/// The contract whose complete report carries a model batch.
const VERIFIED: &str = "verified-v1";

/// Why a flat specimen could not be completed into the document a run writes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompletionError {
    /// The flat specimen is not a JSON object.
    #[error("the flat specimen is not a JSON object")]
    NotAnObject,
    /// The committed report the specimen is completed from does not parse.
    #[error("the committed specimen report does not parse: {source}")]
    Template {
        /// What the parser said.
        #[source]
        source: serde_json::Error,
    },
    /// The committed report holds no object where the specimen goes.
    #[error("the committed specimen report holds no {at}")]
    Shape {
        /// What it lacks.
        at: &'static str,
    },
}

/// The complete document a run writes holding the flat specimen `flat` as its one build's one part, completed from a committed report where the specimen says nothing.
///
/// # Errors
/// [`CompletionError`] where `flat` is not an object, or the committed report is not the shape it is completed into.
pub(crate) fn complete(flat: &Value) -> Result<Value, CompletionError> {
    let flat = flat.as_object().ok_or(CompletionError::NotAnObject)?;
    let mut document = crate::strictjson::from_str(include_str!("proofaudit/specimen-report.json"))
        .map_err(|source| CompletionError::Template { source })?;
    let report = document
        .get_mut("report")
        .and_then(Value::as_object_mut)
        .ok_or(CompletionError::Shape { at: "report" })?;
    for key in ENVELOPE {
        if let Some(value) = flat.get(key) {
            let mut laid = report.get(key).cloned().unwrap_or(Value::Null);
            crate::proofaudit::sentinel::merge(&mut laid, value.clone());
            report.insert(key.to_owned(), laid);
        }
    }
    let run_id = flat.get("run_id").cloned();
    if flat.get("contract").and_then(Value::as_str) == Some(VERIFIED) {
        report.insert(
            "model_completion".to_owned(),
            serde_json::json!({
                "kind": "verified",
                "batch": {
                    "owner": run_id.clone().unwrap_or(Value::Null),
                    "records": flat.get("models").cloned().unwrap_or(Value::Array(Vec::new()))
                }
            }),
        );
    }
    let part = report
        .get_mut("builds")
        .and_then(|builds| builds.get_mut(0))
        .and_then(|build| build.get_mut("parts"))
        .and_then(|parts| parts.get_mut(0))
        .and_then(Value::as_object_mut)
        .ok_or(CompletionError::Shape {
            at: "first part of its first build",
        })?;
    if let Some(run_id) = run_id.clone() {
        part.insert("run_id".to_owned(), run_id);
    }
    for (key, value) in flat {
        if ENVELOPE.contains(&key.as_str()) || CONCLUDED.contains(&key.as_str()) {
            continue;
        }
        match (part.get_mut(key), value) {
            (Some(Value::Object(into)), Value::Object(from)) => {
                let mut merged = Value::Object(into.clone());
                crate::proofaudit::sentinel::merge(&mut merged, Value::Object(from.clone()));
                part.insert(key.clone(), merged);
            }
            _ => {
                part.insert(key.clone(), value.clone());
            }
        }
    }
    let origin = serde_json::json!({
        "scope": "source",
        "build": "default",
        "run_id": run_id.unwrap_or(Value::Null),
        "part": { "kind": "whole" }
    });
    neutral_rows(part, &origin);
    Ok(document)
}

/// The columns a complete report's mutant and finding rows carry that a flat specimen leaves out, each given what a run writes where it says nothing, and `origin` for a finding.
fn neutral_rows(part: &mut serde_json::Map<String, Value>, origin: &Value) {
    if let Some(Value::Array(rows)) = part.get_mut("mutants") {
        for (at, row) in rows.iter_mut().enumerate() {
            if let Some(row) = row.as_object_mut() {
                for (name, neutral) in [
                    ("catalog_index", Value::from(at)),
                    ("item", Value::from("specimen")),
                    ("original", Value::from(">")),
                    ("replacement", Value::from(">=")),
                    ("blind_in", Value::Array(Vec::new())),
                    ("routing", Value::Null),
                ] {
                    row.entry(name.to_owned()).or_insert(neutral);
                }
            }
        }
    }
    if let Some(Value::Array(rows)) = part.get_mut("findings") {
        for row in rows.iter_mut() {
            if let Some(row) = row.as_object_mut() {
                row.entry("origin".to_owned())
                    .or_insert_with(|| origin.clone());
                row.entry("path".to_owned()).or_insert(Value::Null);
            }
        }
    }
}

/// What the runner's recording ends with where a flat specimen says the run concluded `verdict`, numbered after `events` others.
pub(crate) fn concluded(verdict: &str, events: usize) -> Value {
    serde_json::json!({
        "type": "run-end",
        "run": {
            "verdict": verdict,
            "accounting": null,
            "error": null,
            "events_emitted": events.saturating_add(1),
            "events_dropped": 0
        }
    })
}
