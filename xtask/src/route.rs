// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading how a run routed, from either recording that writes it down.

use serde::de::Error as _;
use serde_json::Value;

/// A line in a recording that is not JSON.
#[derive(Debug, thiserror::Error)]
#[error("recording line {line} is not JSON: {source}")]
pub struct ReadError {
    /// The one-based non-empty line number in the recording.
    pub line: usize,
    /// What the JSON reader found there.
    #[source]
    pub source: serde_json::Error,
}

/// Every granularity a route can be decided at.
#[cfg(feature = "testkit")]
pub const GRANULARITIES: [&str; 5] = ["all", "block", "test", "discharged", "unreached"];

/// The granularities the engine writes, which are [`GRANULARITIES`].
#[cfg(feature = "testkit")]
pub const ENGINE_GRANULARITIES: [&str; 5] = GRANULARITIES;

/// The granularities the runner writes, which are [`GRANULARITIES`].
#[cfg(feature = "testkit")]
pub const RUNNER_GRANULARITIES: [&str; 5] = GRANULARITIES;

/// One target a proof removed, and the proof that removed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discharge {
    /// The target.
    pub target: String,
    /// The proof's name.
    pub proof: String,
}

/// One routing decision, as either producer records it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Route {
    /// The mutant, as a person types it.
    pub mutant: String,
    /// Its dense catalog index, which only the engine records.
    pub index: Option<u64>,
    /// How narrow the decision was, in the producer's own words.
    pub granularity: String,
    /// What widened the route back, when something did.
    pub fallback: Option<String>,
    /// The targets that could notice the mutation.
    pub reaching: Vec<String>,
    /// The targets a proof removed.
    pub discharged: Vec<Discharge>,
    /// The targets that ran, which only the engine records.
    pub executed: Vec<String>,
    /// The targets that were measured, asked, and did not reach the mutation.
    pub considered: Vec<String>,
    /// The run this disposition was read back from.
    pub reused: Option<String>,
    /// Why the answer an earlier run left was not the one used, when there was a store of them to ask.
    pub refused: Option<String>,
}

impl Route {
    /// Whether this decision is about the mutant either identity names.
    #[must_use]
    pub fn names(&self, id: &str, display_id: &str) -> bool {
        self.mutant == id || (!display_id.is_empty() && self.mutant == display_id)
    }

    /// Whether a proof removed this target.
    #[must_use]
    pub fn discharges(&self, target: &str) -> bool {
        self.discharged
            .iter()
            .any(|discharge| discharge.target == target)
    }
}

/// One mutant execution, as either producer records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exec {
    /// The mutant, as a person types it, or its full identity when that is all the producer wrote.
    pub mutant: String,
    /// Its dense catalog index, which only the engine records.
    pub index: Option<u64>,
    /// The target it ran against.
    pub target: String,
    /// What the execution established.
    pub outcome: String,
    /// The verified runtime step notice, only for a step-limit outcome.
    pub step_notice: Option<Value>,
    /// How many tests ran, when the harness said.
    pub tests_run: Option<u64>,
    /// How long it took.
    pub duration_ms: Option<u64>,
    /// Whether the recording says the machine was given to this execution alone.
    pub alone: Isolation,
    /// Whether the harness had answered before the clock ended the process, which only the engine records.
    pub lingered: Linger,
    /// The signal the process died of, where the producer recorded one.
    pub signal: Option<i64>,
    /// Every test the harness said failed, which only the engine records.
    pub failed_tests: Vec<String>,
}

/// What a recording establishes about whether a process outlived its harness's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Linger {
    /// This producer did not record it.
    Unrecorded,
    /// The process ended with its harness's answer, or the harness never answered.
    Ended,
    /// The harness had answered and the clock ended the process after it.
    Outlived,
}

impl Linger {
    /// What `recorded` says, where the field is a boolean or absent.
    fn recorded(recorded: Option<&Value>) -> Self {
        match recorded.and_then(Value::as_bool) {
            None => Self::Unrecorded,
            Some(false) => Self::Ended,
            Some(true) => Self::Outlived,
        }
    }
}

/// What a recording establishes about whether an execution had the machine to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Isolation {
    /// This producer did not record isolation at all.
    Unrecorded,
    /// Other work was allowed to run beside this execution.
    Shared,
    /// The scheduler gave the machine to this execution alone.
    Alone,
}

impl Isolation {
    const fn recorded(value: Option<&Value>) -> Self {
        match value {
            Some(Value::Bool(false)) => Self::Shared,
            Some(Value::Bool(true)) => Self::Alone,
            None | Some(_) => Self::Unrecorded,
        }
    }
}

impl Exec {
    /// Whether this execution is about the mutant either identity names.
    #[must_use]
    pub fn names(&self, id: &str, display_id: &str) -> bool {
        self.mutant == id || (!display_id.is_empty() && self.mutant == display_id)
    }
}

/// The routes and the executions of one recording, in the order they were written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Routing {
    /// Every routing decision.
    pub routes: Vec<Route>,
    /// Every mutant execution.
    pub execs: Vec<Exec>,
    /// What the equivalence layer answered for each mutation it asked about, by display identity.
    pub equivalences: Vec<(String, String)>,
}

impl Routing {
    /// The route of one mutant, by the name either producer wrote.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn route(&self, mutant: &str) -> Option<&Route> {
        self.route_of(mutant, mutant)
    }

    /// The route of one mutant a caller holds both identities of.
    #[must_use]
    pub fn route_of(&self, id: &str, display_id: &str) -> Option<&Route> {
        self.routes.iter().find(|route| route.names(id, display_id))
    }

    /// Every execution of one mutant, in the order they ran.
    #[cfg(feature = "testkit")]
    pub fn execs_of<'a>(&'a self, mutant: &'a str) -> impl Iterator<Item = &'a Exec> {
        self.execs_for(mutant, mutant)
    }

    /// Every execution of one mutant a caller holds both identities of.
    pub fn execs_for<'a>(
        &'a self,
        id: &'a str,
        display_id: &'a str,
    ) -> impl Iterator<Item = &'a Exec> {
        self.execs
            .iter()
            .filter(move |exec| exec.names(id, display_id))
    }
}

/// Reads the routes and executions out of a recording, ignoring every valid event that is neither.
///
/// # Errors
/// A non-empty line that is not JSON is rejected.
/// An audit must never turn a corrupt evidence stream into an apparently empty one.
pub fn read(recorded: &str) -> Result<Routing, ReadError> {
    Ok(from_events(&events(recorded)?))
}

/// Reads routing records from events that have already passed the JSONL boundary.
pub(crate) fn from_events(events: &[Value]) -> Routing {
    let mut routing = Routing::default();
    for event in events {
        match text(event, "type").as_deref() {
            Some("route") => {
                if let Some(record) = event.get("route") {
                    routing.routes.push(route(record));
                }
            }
            Some("mutant-exec") => {
                if let Some(record) = event.get("mutant") {
                    routing.execs.push(exec(record));
                }
            }
            Some("note") => {
                let noted = event.get("note");
                let kind = noted.and_then(|note| text(note, "kind"));
                let detail = noted.and_then(|note| text(note, "detail"));
                if let (Some("equivalence"), Some(detail)) = (kind.as_deref(), detail)
                    && let Some((display_id, answer)) = detail.split_once(' ')
                {
                    routing
                        .equivalences
                        .push((display_id.to_owned(), answer.to_owned()));
                }
            }
            _ => {}
        }
    }
    routing
}

/// Parses every non-empty event in a recording without discarding a corrupt line.
pub(crate) fn events(recorded: &str) -> Result<Vec<Value>, ReadError> {
    recorded
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let line_number = index.saturating_add(1);
            let parsed = crate::strictjson::from_str(line).map_err(|source| ReadError {
                line: line_number,
                source,
            })?;
            nested_event(parsed).map_err(|source| ReadError {
                line: line_number,
                source,
            })
        })
        .collect()
}

/// Checks the current-v1 envelope, then gives the independent auditors a collision-free view with payload fields beside the envelope fields.
fn nested_event(event: Value) -> Result<Value, serde_json::Error> {
    let Value::Object(mut envelope) = event else {
        return Err(serde_json::Error::custom("a trace event must be an object"));
    };
    let expected = ["elapsed_ms", "payload", "seq", "timestamp"];
    let actual: Vec<&str> = envelope.keys().map(String::as_str).collect();
    if actual != expected {
        return Err(serde_json::Error::custom(format_args!(
            "a current trace event has exactly seq, timestamp, elapsed_ms, and payload; found {actual:?}"
        )));
    }
    let Some(Value::Object(payload)) = envelope.remove("payload") else {
        return Err(serde_json::Error::custom(
            "a current trace event payload must be an object",
        ));
    };
    for (name, value) in payload {
        if envelope.insert(name.clone(), value).is_some() {
            return Err(serde_json::Error::custom(format_args!(
                "trace payload field {name:?} collides with the envelope"
            )));
        }
    }
    Ok(Value::Object(envelope))
}

/// One route record, from whichever producer wrote it.
fn route(record: &Value) -> Route {
    Route {
        mutant: named(record),
        index: number(record, "index"),
        granularity: text(record, "granularity").unwrap_or_default(),
        fallback: text(record, "fallback"),
        reaching: strings(record, "reaching"),
        discharged: discharges(record),
        executed: strings(record, "executed"),
        considered: strings(record, "considered"),
        reused: text(record, "reused"),
        refused: text(record, "refused"),
    }
}

/// One execution record, from whichever producer wrote it.
fn exec(record: &Value) -> Exec {
    Exec {
        mutant: named(record),
        index: number(record, "index"),
        target: text(record, "target").unwrap_or_default(),
        outcome: text(record, "outcome").unwrap_or_default(),
        step_notice: record
            .get("step_notice")
            .filter(|notice| !notice.is_null())
            .cloned(),
        tests_run: number(record, "tests_run"),
        duration_ms: number(record, "duration_ms"),
        alone: Isolation::recorded(record.get("alone")),
        lingered: Linger::recorded(record.get("lingered")),
        signal: record.get("signal").and_then(Value::as_i64),
        failed_tests: strings(record, "failed_tests"),
    }
}

/// The mutant a record is about: the runner writes `mutant`, the engine writes `id`.
fn named(record: &Value) -> String {
    text(record, "mutant")
        .or_else(|| text(record, "id"))
        .unwrap_or_default()
}

/// Every target a proof removed, with the proof.
fn discharges(record: &Value) -> Vec<Discharge> {
    record
        .get("discharged")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| Discharge {
                    target: text(entry, "target").unwrap_or_default(),
                    proof: text(entry, "proof").unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One string field, absent when it is absent or null.
fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

/// One number field, absent when it is absent or null.
fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}

/// One array of strings, empty when it is absent.
fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
