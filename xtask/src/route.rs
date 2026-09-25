// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading how a run routed, from either recording that writes it down.

use serde::de::Error as _;
use serde_json::Value;

/// A line of a recording this audit will not read, and why.
#[derive(Debug, thiserror::Error)]
#[error("recording line {line}: {cause}")]
pub struct ReadError {
    /// The one-based non-empty line number in the recording, or 0 where the recording as a whole could not be held to its schema.
    pub line: usize,
    /// Why.
    #[source]
    pub cause: ReadCause,
}

/// Why a line of a recording is not read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadCause {
    /// It is not JSON, or not the current envelope.
    #[error("not JSON this audit reads: {source}")]
    Json {
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// It is JSON and departs from its producer's published schema, so a reader could meet an absent required field.
    #[error("off its producer's published schema: {source}")]
    OffSchema {
        /// Where and how.
        #[source]
        source: crate::schemas::OffSchema,
    },
    /// The published schema itself does not compile.
    #[error(transparent)]
    Schema(#[from] crate::schemas::SchemaError),
    /// A field a reader needs is not there, or is not the type it reads, although the line passed its schema.
    #[error("the record has no {field} a reader can read")]
    Absent {
        /// The field, as a path from the record.
        field: String,
    },
}

/// The field `key` of `record`, which every line on its schema carries.
///
/// # Errors
/// [`ReadCause::Absent`] where it is not there or is not what `read` takes.
pub(crate) fn required<'a, T>(
    record: &'a Value,
    key: &str,
    read: impl FnOnce(&'a Value) -> Option<T>,
) -> Result<T, ReadCause> {
    record
        .get(key)
        .and_then(read)
        .ok_or_else(|| ReadCause::Absent {
            field: key.to_owned(),
        })
}

impl crate::error::Coded for ReadCause {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Json { .. } => crate::error::XtCode::RecordingLine,
            Self::OffSchema { .. } => crate::error::XtCode::RecordingOffSchema,
            Self::Schema(schema) => crate::error::Coded::code(schema),
            Self::Absent { .. } => crate::error::XtCode::RecordingUnread,
        }
    }
}

impl crate::error::Coded for ReadError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::Coded::code(&self.cause)
    }
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
    /// Which store a reused answer came out of, `exact` or `carried`, which only the runner records.
    pub rule: Option<String>,
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
pub fn read(recorded: &str, producer: crate::schemas::Producer) -> Result<Routing, ReadError> {
    from_events(&events(recorded, producer)?)
}

/// Reads routing records from events that have already passed the JSONL boundary.
pub(crate) fn from_events(events: &[Value]) -> Result<Routing, ReadError> {
    let mut routing = Routing::default();
    for (at, event) in events.iter().enumerate() {
        let placed = |cause| ReadError {
            line: at.saturating_add(1),
            cause,
        };
        match text(event, "type").as_deref() {
            Some("route") => {
                let record = required(event, "route", Some).map_err(placed)?;
                routing.routes.push(route(record).map_err(placed)?);
            }
            Some("mutant-exec") => {
                let record = required(event, "mutant", Some).map_err(placed)?;
                routing.execs.push(exec(record).map_err(placed)?);
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
    Ok(routing)
}

/// Parses every non-empty event in a recording without discarding a corrupt line, holding each to `producer`'s published schema first.
///
/// # Errors
/// [`ReadError`] for the first line that is not JSON or not on its schema.
pub(crate) fn events(
    recorded: &str,
    producer: crate::schemas::Producer,
) -> Result<Vec<Value>, ReadError> {
    let checker = crate::schemas::Checker::of(producer).map_err(|source| ReadError {
        line: 0,
        cause: ReadCause::Schema(source),
    })?;
    recorded
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let line_number = index.saturating_add(1);
            let json = |source| ReadError {
                line: line_number,
                cause: ReadCause::Json { source },
            };
            let parsed = crate::strictjson::from_str(line).map_err(json)?;
            checker.check(&parsed).map_err(|source| ReadError {
                line: line_number,
                cause: ReadCause::OffSchema { source },
            })?;
            nested_event(parsed).map_err(json)
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
fn route(record: &Value) -> Result<Route, ReadCause> {
    Ok(Route {
        mutant: named(record)?,
        index: number(record, "index"),
        granularity: required(record, "granularity", owned)?,
        fallback: text(record, "fallback"),
        reaching: strings(record, "reaching"),
        discharged: discharges(record)?,
        executed: strings(record, "executed"),
        considered: strings(record, "considered"),
        reused: text(record, "reused"),
        refused: text(record, "refused"),
        rule: text(record, "rule"),
    })
}

/// One execution record, from whichever producer wrote it.
fn exec(record: &Value) -> Result<Exec, ReadCause> {
    Ok(Exec {
        mutant: named(record)?,
        index: number(record, "index"),
        target: required(record, "target", owned)?,
        outcome: required(record, "outcome", owned)?,
        step_notice: match record.get("step_notice") {
            None | Some(Value::Null) => None,
            Some(notice) => Some(notice.clone()),
        },
        tests_run: number(record, "tests_run"),
        duration_ms: number(record, "duration_ms"),
        alone: Isolation::recorded(record.get("alone")),
        lingered: Linger::recorded(record.get("lingered")),
        signal: record.get("signal").and_then(Value::as_i64),
        failed_tests: strings(record, "failed_tests"),
    })
}

/// The mutant a record is about: the runner writes `mutant`, the engine writes `id`.
fn named(record: &Value) -> Result<String, ReadCause> {
    text(record, "mutant")
        .or_else(|| text(record, "id"))
        .ok_or_else(|| ReadCause::Absent {
            field: "mutant or id".to_owned(),
        })
}

/// Every target a proof removed, with the proof; none where the producer writes no such list.
fn discharges(record: &Value) -> Result<Vec<Discharge>, ReadCause> {
    record
        .get("discharged")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    Ok(Discharge {
                        target: required(entry, "target", owned)?,
                        proof: required(entry, "proof", owned)?,
                    })
                })
                .collect::<Result<Vec<Discharge>, ReadCause>>()
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

/// A string, owned.
fn owned(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
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
