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
    pub cause: ReadCauseError,
}

/// Why a line of a recording is not read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadCauseError {
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
        source: crate::schemas::OffSchemaError,
    },
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
/// [`ReadCauseError::Absent`] where it is not there or is not what `read` takes.
pub(crate) fn required<'a, T>(
    record: &'a Value,
    key: &str,
    read: impl FnOnce(&'a Value) -> Option<T>,
) -> Result<T, ReadCauseError> {
    record
        .get(key)
        .and_then(read)
        .ok_or_else(|| ReadCauseError::Absent {
            field: key.to_owned(),
        })
}

impl crate::error::Coded for ReadCauseError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Json { .. } => crate::error::XtCode::RecordingLine,
            Self::OffSchema { .. } => crate::error::XtCode::RecordingOffSchema,
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

/// A field only one of the two producers writes: what this recording says, or that its producer does not write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded<T> {
    /// The producer of this recording does not write the field, so the recording says nothing of it.
    Unrecorded,
    /// What the recording says.
    Said(T),
}

/// One routing decision, as either producer records it.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub executed: Recorded<Vec<String>>,
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
    pub failed_tests: Recorded<Vec<String>>,
    /// Each test that declined to measure and its words, as `(test, why)`, which only the engine records (ADR 0043).
    pub declined: Recorded<Vec<(String, String)>>,
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

/// One sealed execution a recording holds: one test of one module run with one mutant active, and what it came to (ADR 0046).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    /// The mutant, as a person types it.
    pub mutant: String,
    /// The target whose module ran.
    pub target: String,
    /// The test it ran.
    pub test: String,
    /// What it came to, as a report spells it.
    pub came_to: String,
}

impl Sealed {
    /// Whether this execution is about the mutant either identity names.
    #[must_use]
    pub fn names(&self, id: &str, display_id: &str) -> bool {
        self.mutant == id || (!display_id.is_empty() && self.mutant == display_id)
    }
}

/// One execution of one mutant, the only shape a reader is handed one in, so a reader of either kind says what it makes of the other (ADR 0046).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Execution {
    /// A native execution: what the process said of itself, which is a lead.
    Native(Exec),
    /// A sealed execution: what the host observed, which a verdict rests on.
    Sealed(Sealed),
}

impl Execution {
    /// Whether this execution is about the mutant either identity names.
    #[must_use]
    pub fn names(&self, id: &str, display_id: &str) -> bool {
        match self {
            Self::Native(exec) => exec.names(id, display_id),
            Self::Sealed(sealed) => sealed.names(id, display_id),
        }
    }
}

/// What a layer of either audit reads of the executions a row can rest on, each handed to it as an [`Execution`] it has to place (ADR 0046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reads {
    /// No execution of a mutation.
    Nothing,
    /// Native executions alone, because what it holds is only ever native: a process started, or a disposition run again with sealing off.
    Native,
    /// Native and sealed executions, and a defect only a sealed execution shows is planted for it.
    Both,
}

/// The routes and the executions of one recording, in the order they were written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Routing {
    /// Every routing decision.
    pub routes: Vec<Route>,
    /// Every execution, native and sealed, which only [`Routing::executions`] hands out.
    executions: Vec<Execution>,
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

    /// Every execution, native and sealed, in the order they were written.
    #[must_use]
    pub fn executions(&self) -> &[Execution] {
        &self.executions
    }

    /// Every execution of one mutant, in the order they ran.
    #[cfg(feature = "testkit")]
    pub fn executions_of<'a>(&'a self, mutant: &'a str) -> impl Iterator<Item = &'a Execution> {
        self.executions_for(mutant, mutant)
    }

    /// Every execution of one mutant a caller holds both identities of, in the order they ran.
    pub fn executions_for<'a>(
        &'a self,
        id: &'a str,
        display_id: &'a str,
    ) -> impl Iterator<Item = &'a Execution> {
        self.executions
            .iter()
            .filter(move |execution| execution.names(id, display_id))
    }
}

/// Reads the routes and executions out of a runner's or an engine's recording, ignoring every valid event that is neither.
///
/// # Errors
/// A route or execution missing what it must say.
pub fn read<L: crate::schemas::Lines>(recorded: &Checked<L>) -> Result<Routing, ReadError> {
    from_events(recorded.events())
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
                routing
                    .executions
                    .push(Execution::Native(exec(record).map_err(placed)?));
            }
            Some("sealed-exec") => {
                let record = required(event, "sealed", Some).map_err(placed)?;
                routing
                    .executions
                    .push(Execution::Sealed(sealed(record).map_err(placed)?));
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

/// A recording read once, every non-empty line of it held to the published schema of the producer `L` names, which every reader takes instead of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked<L> {
    events: Vec<Value>,
    lines: std::marker::PhantomData<L>,
}

impl<L: crate::schemas::Lines> Checked<L> {
    /// Parses every non-empty line of `recorded` without discarding a corrupt one, holding each to `L`'s schema among `checkers` first.
    ///
    /// # Errors
    /// [`ReadError`] for the first line that is not JSON or not on its schema.
    pub fn read(recorded: &str, checkers: &crate::schemas::Checkers) -> Result<Self, ReadError> {
        let checker = checkers.lines(L::PRODUCER);
        let events = recorded
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| {
                let line_number = index.saturating_add(1);
                let json = |source| ReadError {
                    line: line_number,
                    cause: ReadCauseError::Json { source },
                };
                let parsed = crate::strictjson::from_str(line).map_err(json)?;
                checker.check(&parsed).map_err(|source| ReadError {
                    line: line_number,
                    cause: ReadCauseError::OffSchema { source },
                })?;
                nested_event(parsed).map_err(json)
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            events,
            lines: std::marker::PhantomData,
        })
    }

    /// Every event, in the order the recording holds them, its payload beside its envelope.
    #[must_use]
    pub fn events(&self) -> &[Value] {
        &self.events
    }

    /// Every event, owned.
    #[must_use]
    pub fn into_events(self) -> Vec<Value> {
        self.events
    }
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
fn route(record: &Value) -> Result<Route, ReadCauseError> {
    Ok(Route {
        mutant: named(record)?,
        index: number(record, "index"),
        granularity: required(record, "granularity", owned)?,
        fallback: text(record, "fallback"),
        reaching: texts(record, "reaching")?,
        discharged: discharges(record)?,
        executed: recorded_texts(record, "executed")?,
        considered: texts(record, "considered")?,
        reused: text(record, "reused"),
        refused: text(record, "refused"),
        rule: text(record, "rule"),
    })
}

/// One execution record, from whichever producer wrote it.
fn sealed(record: &Value) -> Result<Sealed, ReadCauseError> {
    Ok(Sealed {
        mutant: required(record, "mutant", owned)?,
        target: required(record, "target", owned)?,
        test: required(record, "test", owned)?,
        came_to: required(record, "came_to", owned)?,
    })
}

fn exec(record: &Value) -> Result<Exec, ReadCauseError> {
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
        failed_tests: recorded_texts(record, "failed_tests")?,
        declined: declines(record)?,
    })
}

/// Each test a record says declined to measure, with its words, or that its producer does not record declines, as the runner's records do not.
///
/// # Errors
/// [`ReadCauseError::Absent`] for a list that is not one, or an entry that is not a test and its words.
pub fn declines(record: &Value) -> Result<Recorded<Vec<(String, String)>>, ReadCauseError> {
    let Some(entries) = record.get("declined") else {
        return Ok(Recorded::Unrecorded);
    };
    let Some(entries) = entries.as_array() else {
        return Err(ReadCauseError::Absent {
            field: "declined".to_owned(),
        });
    };
    entries
        .iter()
        .map(|entry| match (text(entry, "test"), text(entry, "why")) {
            (Some(test), Some(why)) => Ok((test, why)),
            (None, _) | (_, None) => Err(ReadCauseError::Absent {
                field: "declined[].test and declined[].why".to_owned(),
            }),
        })
        .collect::<Result<Vec<(String, String)>, ReadCauseError>>()
        .map(Recorded::Said)
}

/// The mutant a record is about: the runner writes `mutant`, the engine writes `id`.
fn named(record: &Value) -> Result<String, ReadCauseError> {
    text(record, "mutant")
        .or_else(|| text(record, "id"))
        .ok_or_else(|| ReadCauseError::Absent {
            field: "mutant or id".to_owned(),
        })
}

/// Every target a proof removed, with the proof, which both producers write on every route.
fn discharges(record: &Value) -> Result<Vec<Discharge>, ReadCauseError> {
    required(record, "discharged", Value::as_array)?
        .iter()
        .map(|entry| {
            Ok(Discharge {
                target: required(entry, "target", owned)?,
                proof: required(entry, "proof", owned)?,
            })
        })
        .collect()
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

/// The strings of the array field `key`, which every line on its schema carries.
///
/// # Errors
/// [`ReadCauseError::Absent`] where the field is not there, is not an array, or holds anything but strings.
fn texts(value: &Value, key: &str) -> Result<Vec<String>, ReadCauseError> {
    required(value, key, Value::as_array)?
        .iter()
        .map(|entry| {
            owned(entry).ok_or_else(|| ReadCauseError::Absent {
                field: format!("{key}[]"),
            })
        })
        .collect()
}

/// The strings of the array field `key` where this record's producer writes it, and that it does not where the field is not there.
///
/// # Errors
/// What [`texts`] refuses, for a field that is there.
fn recorded_texts(value: &Value, key: &str) -> Result<Recorded<Vec<String>>, ReadCauseError> {
    match value.get(key) {
        None => Ok(Recorded::Unrecorded),
        Some(_) => texts(value, key).map(Recorded::Said),
    }
}
