// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The wire shape of one trace event: an envelope of sequence number and moment around a typed record.

use serde::{Deserialize, Serialize};

use crate::config::Contract;
use crate::report::{ConclusionAccounting, RunKind};

/// The schema name carried by every current `run-start` event.
#[cfg(any(test, feature = "testkit"))]
pub const SCHEMA: &str = "njutest-trace-v1";
#[cfg(not(any(test, feature = "testkit")))]
pub(super) const SCHEMA: &str = "njutest-trace-v1";

/// One event of a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// Monotonic from 1; delivery order is sequence order.
    pub seq: u64,
    /// The moment the event was recorded, RFC 3339 in UTC.
    pub timestamp: String,
    /// Milliseconds since the recording started.
    pub elapsed_ms: u64,
    /// The typed record, nested so envelope and payload fields cannot collide.
    pub payload: Payload,
}

/// The typed record of an event, tagged by `type` on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub enum Payload {
    /// The first event of every recording.
    RunStart {
        /// What the run is.
        start: StartRecord,
    },
    /// A phase began.
    PhaseStart {
        /// The phase.
        phase: PhaseRecord,
    },
    /// A phase ended.
    PhaseEnd {
        /// The phase, with its duration.
        phase: PhaseRecord,
    },
    /// A process ran.
    Exec {
        /// The record.
        exec: ExecRecord,
    },
    /// The run said how far it had got.
    Progress {
        /// The record.
        progress: ProgressRecord,
    },
    /// The run kept a file or a directory.
    Artifact {
        /// The record.
        artifact: ArtifactRecord,
    },
    /// One mutant's routing decision, with the proofs that narrowed it.
    Route {
        /// The record.
        route: RouteRecord,
    },
    /// One mutant ran against one target.
    MutantExec {
        /// The record.
        mutant: MutantExecRecord,
    },
    /// One fault put to one target, which no reader of mutant executions ever sees.
    FaultExec {
        /// The record.
        fault: FaultExecRecord,
    },
    /// Which targets reach one fault, which no reader of mutant routes ever sees.
    FaultRoute {
        /// The record.
        route: FaultRouteRecord,
    },
    /// A fault the compiler refused, so it was never put.
    FaultRejected {
        /// The record.
        rejected: FaultRejectedRecord,
    },
    /// Whether one fault, run alone, wrote a path the phase left in the tree, and whether its test did without it.
    FaultAttribution {
        /// The record.
        attribution: FaultAttributionRecord,
    },
    /// What the original code did on the target a fault's detection is confirmed against.
    FaultControl {
        /// The record.
        control: FaultControlRecord,
    },
    /// What a run established about one site a fault was asked at.
    Fault {
        /// The record, as the report holds it.
        fault: crate::report::faults::FaultRecord,
    },
    /// A survivor a target told apart only with the call at its site failing beside it.
    Beside {
        /// The record, as the report holds it.
        beside: crate::report::faults::BesideRecord,
    },
    /// One pair of runs of a target behind evidence beside a fault.
    BesideRun {
        /// The record.
        pair: crate::report::faults::BesideRun,
    },
    /// One run of a test a crash was put to: stopped at the call, run again over what it left, or run in a fresh scratch.
    CrashExec {
        /// The record.
        crash: CrashExecRecord,
    },
    /// One thing a run did about a crash besides running a test: refused it, left it alone, routed it, or saw its stop write into the tree.
    CrashStep {
        /// The record.
        step: CrashStepRecord,
    },
    /// What a run established about one call that writes a crash was asked at.
    Crash {
        /// The record, as the report holds it.
        crash: crate::report::crashes::CrashRecord,
    },
    /// What the probe pass measured for one target.
    ProbeExec {
        /// The record.
        probe: ProbeExecRecord,
    },
    /// One exchange that went past a seam the run was watching.
    WireExchange {
        /// The record.
        exchange: WireExchangeRecord,
    },
    /// One fault a recording licensed, put to the suite, and what came of it.
    WireExec {
        /// The record.
        wire: WireExecRecord,
    },
    /// One mutant planted for a routing layer, and how the engine routed it before the baseline.
    Sentinel {
        /// The record.
        sentinel: SentinelRecord,
    },
    /// One closed model-checking question and its typed answer.
    Model {
        /// The same independently auditable record retained in the report.
        model: Box<crate::report::ModelRecord>,
    },
    /// What one original-code control established about one target's baseline reach.
    Drift {
        /// The record.
        drift: DriftRecord,
    },
    /// What one control under one knob established about one target, as the report keeps it.
    Knob {
        /// The record.
        knob: crate::report::knobs::KnobRecord,
    },
    /// Something worth writing down that has no shape of its own yet.
    Note {
        /// The record.
        note: NoteRecord,
    },
    /// The last event of every recording.
    RunEnd {
        /// What the run concluded, and what the recording lost.
        run: RunRecord,
    },
}

impl Payload {
    /// The `type` this record carries on the wire.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::RunStart { .. } => "run-start",
            Self::PhaseStart { .. } => "phase-start",
            Self::PhaseEnd { .. } => "phase-end",
            Self::Exec { .. } => "exec",
            Self::Progress { .. } => "progress",
            Self::Artifact { .. } => "artifact",
            Self::Route { .. } => "route",
            Self::MutantExec { .. } => "mutant-exec",
            Self::FaultExec { .. } => "fault-exec",
            Self::FaultControl { .. } => "fault-control",
            Self::FaultAttribution { .. } => "fault-attribution",
            Self::FaultRoute { .. } => "fault-route",
            Self::FaultRejected { .. } => "fault-rejected",
            Self::Fault { .. } => "fault",
            Self::Beside { .. } => "beside",
            Self::BesideRun { .. } => "beside-run",
            Self::CrashExec { .. } => "crash-exec",
            Self::CrashStep { .. } => "crash-step",
            Self::Crash { .. } => "crash",
            Self::ProbeExec { .. } => "probe-exec",
            Self::WireExchange { .. } => "wire-exchange",
            Self::WireExec { .. } => "wire-exec",
            Self::Sentinel { .. } => "sentinel",
            Self::Model { .. } => "model",
            Self::Drift { .. } => "drift",
            Self::Knob { .. } => "knob",
            Self::Note { .. } => "note",
            Self::RunEnd { .. } => "run-end",
        }
    }
}

/// What the run is: enough to tell two recordings apart without reading them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRecord {
    /// The trace schema this recording claims.
    pub schema: String,
    /// The runner version that recorded.
    pub njutest: String,
    /// The engine version it drove.
    pub rust_mutants: String,
    /// The run's identity.
    pub run_id: String,
    /// How much of the workspace it looked at.
    pub run_kind: RunKind,
    /// Which contract it answered to.
    pub contract: Contract,
}

impl StartRecord {
    /// The record of a run of this build.
    #[must_use]
    pub fn of(run_id: &str, run_kind: RunKind, contract: Contract) -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            njutest: crate::VERSION.to_owned(),
            rust_mutants: rust_mutants::VERSION.to_owned(),
            run_id: run_id.to_owned(),
            run_kind,
            contract,
        }
    }
}

/// One phase boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseRecord {
    /// What the phase is called.
    pub name: String,
    /// How long it took, on the end event alone.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub duration_ms: Option<u64>,
}

/// One executed process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRecord {
    /// The command line, verbatim.
    pub argv: Vec<String>,
    /// The directory it ran in.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub dir: Option<String>,
    /// The names of the variables it ran with, never the values.
    pub env_names: Vec<String>,
    /// The bound the caller put on it.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub timeout_ms: Option<u64>,
    /// How it came to an end, which is one thing and not a status beside a flag.
    ///
    /// The engine's type, not a second one: two products recording the same kind of event in two vocabularies is what a reader holding both streams has to reconcile by hand, and a pair of a status and a "the clock fired" boolean can say a process was killed by a bound and also exited 101 (ADR 0023).
    pub stopped: rust_mutants::execute::Stopped,
    /// How long it took.
    pub duration_ms: u64,
    /// How much it said.
    pub output_bytes: u64,
    /// The digest of everything it said.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub output_sha256: Option<String>,
    /// Whether the preserved copy was cut.
    pub output_truncated: bool,
    /// Where the preserved copy is, relative to the recording directory.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub output_path: Option<String>,
    /// Why it could not be run, when it could not.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub error: Option<String>,
    /// The capture itself, for a sink that preserves it.
    /// Never serialized.
    #[serde(skip)]
    pub output: Vec<u8>,
}

impl ExecRecord {
    /// The record of one supervised run: the spec's command line, directory, environment names, and timeout, and the result's exit code, timeout flag, duration, output, and error.
    ///
    /// # Errors
    /// A timeout or measured duration does not fit the trace wire exactly, or a command, directory, or environment name is not valid UTF-8.
    pub fn of(
        spec: &rust_mutants::runner::Spec,
        result: &rust_mutants::runner::RunResult,
    ) -> Result<Self, rust_mutants::trace::ExecRecordError> {
        let timeout_ms = spec
            .timeout
            .map(|timeout| {
                u64::try_from(timeout.as_millis()).map_err(|_overflow| {
                    rust_mutants::trace::ExecRecordError::MillisecondsOutsideWire {
                        field: "timeout",
                    }
                })
            })
            .transpose()?;
        let duration_ms = u64::try_from(result.duration.as_millis()).map_err(|_overflow| {
            rust_mutants::trace::ExecRecordError::MillisecondsOutsideWire {
                field: "measured process",
            }
        })?;
        let trace_text = |value: &std::ffi::OsStr,
                          field: &'static str|
         -> Result<String, rust_mutants::trace::ExecRecordError> {
            value
                .to_str()
                .map(str::to_owned)
                .ok_or(rust_mutants::trace::ExecRecordError::NonUtf8 { field })
        };
        Ok(Self {
            argv: spec
                .argv
                .iter()
                .map(|arg| trace_text(arg, "argument"))
                .collect::<Result<Vec<_>, _>>()?,
            dir: spec
                .dir
                .as_ref()
                .map(|dir| trace_text(dir.as_os_str(), "working directory"))
                .transpose()?,
            env_names: spec
                .env
                .as_ref()
                .map(|env| {
                    env.iter()
                        .map(|(key, _)| trace_text(key, "environment name"))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default(),
            timeout_ms,
            stopped: rust_mutants::execute::Stopped::of(result),
            duration_ms,
            output_bytes: 0,
            output_sha256: None,
            output_truncated: false,
            output_path: None,
            error: result.error().map(|failure| failure.to_string()),
            output: result.output.clone(),
        })
    }
}

/// How far the run had got, as the user interface saw it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressRecord {
    /// What was happening, in the words a person watching reads.
    pub message: String,
    /// What it was about, by the name a later command takes.
    /// Empty where the step is about nothing that has one.
    ///
    /// Held apart from the message because the two have different readers: an audit follows this back to one mutation, and a person watching a run learns nothing from a digest.
    pub subject: String,
    /// How many of it are done.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub done: Option<u64>,
    /// How many there are.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub total: Option<u64>,
}

/// Something the run kept for a person to look at.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRecord {
    /// What kind of thing it is.
    pub kind: String,
    /// Where it is.
    pub path: String,
    /// How big it is, when that was measured.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub bytes: Option<u64>,
}

/// One target a proof removed from a reaching set, beside the proof that removed it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DischargeRecord {
    /// The target that was not run.
    pub target: String,
    /// What removed it.
    pub proof: String,
}

/// How one mutant's tests were chosen, and what narrowed the choice.
///
/// No `Default`, because there is no granularity a route is decided at when nobody decided it.
/// A record standing for a routing that did not happen would read as one that did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRecord {
    /// The mutant a person types.
    pub mutant: String,
    /// How the route was decided.
    pub granularity: rust_mutants::session::Granularity,
    /// What the measurement could not support, on a route it did not decide on its own.
    /// Every one of these widened the route.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub fallback: Option<rust_mutants::session::Fallback>,
    /// The targets to run, cheapest first.
    pub reaching: Vec<String>,
    /// Which tests of a target the mutation is put to, for each target a measurement narrowed.
    /// A target that is not named here runs every test it has.
    pub tests: Vec<AskedRecord>,
    /// The targets a proof removed, each beside the proof that removed it.
    pub discharged: Vec<DischargeRecord>,
    /// The measured targets that were asked and did not reach the mutation.
    pub considered: Vec<String>,
    /// The run this disposition was read back from, when it was not established here.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub reused: Option<String>,
    /// Why the answer an earlier run left was not the one this run used, when there was a store to ask.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub refused: Option<String>,
}

/// Which tests of one target a route puts the mutation to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskedRecord {
    /// The target.
    pub target: String,
    /// The tests, by their libtest path.
    pub tests: Vec<String>,
}

/// One mutant run against one target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutantExecRecord {
    /// The mutant a person types.
    pub mutant: String,
    /// The target it ran against, or `package-suite` when no proof said which tests could notice it.
    pub target: String,
    /// The arguments the target was given, verbatim.
    pub args: Vec<String>,
    /// What the run established.
    pub outcome: String,
    /// The checked step boundary, present exactly when `outcome` is `step_limit_reached`.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub step_boundary: Option<crate::report::StepBoundary>,
    /// How long it took.
    pub duration_ms: u64,
    /// Whether the machine was given to this execution, which a run does once when a budget expires.
    pub alone: bool,
}

/// Which execution of a fault one record is, so a detection can be held to the confirmation it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaultRole {
    /// The execution that asked the target.
    First,
    /// The execution that asked again after a failure, with the control between.
    Confirmation,
    /// The execution run alone to see whether it writes a path the phase left in the tree.
    Attribution,
    /// The same test run alone without the fault, to see whether it writes that path anyway.
    #[serde(rename = "attribution-control")]
    AttributionControl,
}

/// Whether one fault, run alone on one target, wrote a path, and whether the target did without it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultAttributionRecord {
    /// The fault.
    pub fault: String,
    /// The target it was run on.
    pub target: String,
    /// The path of the tree the phase left written.
    pub path: String,
    /// Whether the path was there after the fault ran alone.
    pub faulted: bool,
    /// Whether the target passed with the fault in place, which is what makes the write the program's rather than the test's own failure's.
    pub passed: bool,
    /// What the same target did run alone without the fault.
    pub unfaulted: Unfaulted,
}

/// What a target run alone without a fault did to a path its faulted run wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unfaulted {
    /// It was not run, because the faulted run wrote nothing or its test failed.
    NotAsked,
    /// It wrote the path too, so the fault is not what wrote it.
    Wrote,
    /// It did not write the path, so the fault did.
    DidNotWrite,
}

/// Which targets reach one fault.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultRouteRecord {
    /// The fault a person types.
    pub fault: String,
    /// Every target whose baseline reached the site, which is empty where nothing did.
    pub reaching: Vec<String>,
}

/// A fault the compiler refused.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultRejectedRecord {
    /// The fault a person types.
    pub fault: String,
    /// The first line of what the compiler said.
    pub diagnostic: String,
}

/// What the original code did on one target, answered for one fault's confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultControlRecord {
    /// The fault whose detection this confirms.
    pub fault: String,
    /// The target.
    pub target: String,
    /// Whether the target passed on the original code.
    pub passed: bool,
}

/// One fault run against one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultExecRecord {
    /// The fault a person types.
    pub fault: String,
    /// Which execution of the fault this is.
    pub role: FaultRole,
    /// The target it ran against.
    pub target: String,
    /// The arguments the target was given, verbatim.
    pub args: Vec<String>,
    /// What the run established.
    pub outcome: String,
    /// How long it took.
    pub duration_ms: u64,
    /// Whether the machine was given to this execution, which a run does once when a budget expires.
    pub alone: bool,
}

/// One run of a test a crash was put to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashExecRecord {
    /// The crash a person types.
    pub crash: String,
    /// The target the test is in.
    pub target: String,
    /// The test.
    pub test: String,
    /// Which run it was: `crash`, stopped at the call; `next`, over what a crash left; or `fresh`, in a scratch of its own.
    pub stage: String,
    /// The exit status, which is how a stop at the call is told from a test that failed.
    pub exit_code: i64,
    /// What the engine made of it.
    pub outcome: String,
    /// Whether the runtime published the notice that it stopped at the call, which is what makes the exit status a stop rather than a status the test chose.
    pub noticed: bool,
    /// What the engine issued a `crash` run and found published, which `noticed` is decided on and an audit decides again; nothing on a `next` or `fresh` run.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub issued: Option<CrashNoticeRecord>,
    /// The files a stopped run left in its scratch, on a `crash` run that stopped; empty otherwise.
    pub left: Vec<String>,
    /// The tests a `next` or `fresh` run failed.
    pub failed: Vec<String>,
}

/// One thing a run did about a crash besides running a test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashStepRecord {
    /// The crash a person types.
    pub crash: String,
    /// What the run did.
    pub taken: CrashStep,
}

/// What a run did about a crash besides running a test, in the order its decision is re-derived from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CrashStep {
    /// The compiler refused the crash, so nothing ran.
    Rejected,
    /// An earlier stop wrote into the tree under measurement, so nothing ran.
    Tainted,
    /// The targets and tests that reach the call, in the order they are asked.
    Route {
        /// Every target, with its tests where the route names them.
        asked: Vec<CrashAsked>,
    },
    /// A stop of this crash wrote into the tree under measurement.
    Outside,
}

/// One target a crash's route asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashAsked {
    /// The target.
    pub target: String,
    /// The tests that reach the call, or nothing where which of them does is not known.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub tests: Option<Vec<String>>,
}

/// What the engine issued one crashed run, and what it read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashNoticeRecord {
    /// The mutation the run had active, in full.
    pub mutant: String,
    /// The catalog it was of.
    pub catalog: String,
    /// The nonce issued to this run alone.
    pub nonce: String,
    /// The notice's text as read, or nothing where none was published.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub read: Option<String>,
}

/// What the probe pass measured for one target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeExecRecord {
    /// The target.
    pub target: String,
    /// `measured` when the pass read that target's log, `not-measured` when it did not.
    pub outcome: String,
    /// How many mutants the target infected.
    /// A target the pass did not measure carries no facts, and none is not zero.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub infected: Option<u64>,
}

/// What one original-code control, run to confirm a kill, established about whether one target reached what its baseline did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftRecord {
    /// The mutation whose kill the control was confirming.
    pub mutant: String,
    /// What it established, as the report records it.
    pub observed: crate::report::drift::Drift,
}

/// How much of one exchange the wire said to read, and what that reading found.
///
/// Was a string whose legal values a doc comment listed, beside three fields each free to be absent when it said `http` and present when it said `raw`.
/// Sixteen shapes for two facts, and an audit re-mints a fault identity from exactly these, so a recording that spelled one of the other fourteen would have the audit and the run name the same exchange differently and neither able to say which was wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// Nothing but the byte counts, because the seam was watched as bytes.
    Raw,
    /// One HTTP round trip.
    Http {
        /// What was asked for.
        method: String,
        /// Where it was asked of.
        path: String,
        /// What the upstream answered.
        status: u16,
    },
}

/// The four fields the recording has always carried, which is the shape rather than what it means.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairedRead {
    wire: String,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    method: Option<String>,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    path: Option<String>,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    status: Option<u16>,
}

/// What the recording calls a seam nothing parsed.
const RAW: &str = "raw";

/// What it calls one read as HTTP.
const HTTP: &str = "http";

impl Serialize for Read {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let paired = match self {
            Self::Raw => PairedRead {
                wire: RAW.to_owned(),
                method: None,
                path: None,
                status: None,
            },
            Self::Http {
                method,
                path,
                status,
            } => PairedRead {
                wire: HTTP.to_owned(),
                method: Some(method.clone()),
                path: Some(path.clone()),
                status: Some(*status),
            },
        };
        paired.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Read {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let held = PairedRead::deserialize(deserializer)?;
        match (held.wire.as_str(), held.method, held.path, held.status) {
            (RAW, None, None, None) => Ok(Self::Raw),
            (HTTP, Some(method), Some(path), Some(status)) => Ok(Self::Http {
                method,
                path,
                status,
            }),
            (RAW | HTTP, _, _, _) => Err(serde::de::Error::custom(
                "an exchange says how much of it was read and carries some other amount of \
                 it; an audit re-mints the fault identities from these fields, so a reader \
                 cannot tell whether the protocol or what was read from it is the wrong half",
            )),
            (said, _, _, _) => Err(serde::de::Error::custom(format!(
                "an exchange says it was read as {said:?}, which is not something this run \
                 knows how to read"
            ))),
        }
    }
}

/// One exchange that went past a seam, as much of it as the wire says to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireExchangeRecord {
    /// The capability the seam serves.
    pub capability: String,
    /// Where it fell in the order on that seam, from zero.
    pub seq: u64,
    /// What the run had running, where it could tell.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub during: Option<String>,
    /// How long the round trip took.
    pub duration_ms: u64,
    /// How much of it was read, and what that reading found, as one closed object.
    pub read: Read,
    /// How many bytes went up.
    pub request_bytes: u64,
    /// How many came back.
    pub response_bytes: u64,
}

/// One fault put to the suite, and what the suite did with it.
///
/// The rule and the decision are the sets the run holds them as, not their names: a doc comment listing the legal values beside a `String` is a closed set written in prose, which is the shape the compiler cannot check.
/// The Current v1 recordings nest the answer so its fields cannot collide with the fault identity or rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireExecRecord {
    /// The fault's identity.
    pub fault: String,
    /// The seam it names.
    pub capability: String,
    /// The exchange it names, by its place in the order.
    pub seq: u64,
    /// What it asked the seam to do.
    pub rule: crate::wire::rule::Rule,
    /// Who decided it, and — where somebody did — who that was.
    #[serde(rename = "answer")]
    pub decision: crate::report::SeamDecision,
}

/// One mutant planted for a routing layer, what that layer had to do with it, and what the engine did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SentinelRecord {
    /// The layer it was planted for.
    pub layer: rust_mutants::sentinel::Planted,
    /// The planted mutant, by its locator.
    pub mutant: String,
    /// How the layer must route it.
    pub expected: rust_mutants::sentinel::Expected,
    /// How the engine routed it.
    pub routed: String,
    /// Whether the route was the one expected.
    pub sighted: bool,
}

impl SentinelRecord {
    /// The record of one sighting.
    #[must_use]
    pub fn of(sighting: &rust_mutants::sentinel::Sighting) -> Self {
        Self {
            layer: sighting.expectation.planted,
            mutant: sighting.expectation.mutant.to_string(),
            expected: sighting.expectation.expected,
            routed: sighting.routed(),
            sighted: sighting.sighted(),
        }
    }
}

/// A free-form note, for what has no shape of its own yet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteRecord {
    /// What kind of note it is, so a reader can filter.
    pub kind: String,
    /// What it says.
    pub detail: String,
}

/// What the run concluded, and what the recording lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    /// The verdict, or what stopped the run from reaching one.
    pub verdict: String,
    /// What the run counted, when it got far enough to count.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub accounting: Option<RunAccounting>,
    /// The error that ended the run, when one did.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub error: Option<String>,
    /// Events the sink kept before this one.
    /// A recording cannot count the event it is writing, so this is the honest number rather than a guess that the last one lands.
    pub events_emitted: u64,
    /// Events the sink lost before this one.
    /// A recording is honest about its own losses; a reader finds anything lost afterwards as a sequence gap.
    pub events_dropped: u64,
}

/// Complete-run counts projected from the final build lattice without collapsing build-qualified soundness evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunAccounting {
    /// Target rows across all configured builds.
    pub targets: crate::report::TargetAccounting,
    /// Final mutation decisions after the cross-build/model lattice.
    pub mutants: crate::report::MutantAccounting,
    /// One exact inventory per configured build, in request order.
    pub soundness_by_build: Vec<BuildSoundnessRecord>,
}

/// One configured build's independently retained soundness inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSoundnessRecord {
    /// The canonical configured-build name.
    pub build: crate::report::BuildName,
    /// The inventory measured for that build alone.
    pub accounting: crate::report::SoundnessAccounting,
}

impl From<ConclusionAccounting> for RunAccounting {
    fn from(accounting: ConclusionAccounting) -> Self {
        Self {
            targets: accounting.targets,
            mutants: accounting.mutants,
            soundness_by_build: accounting
                .soundness_by_build
                .into_iter()
                .map(|build| BuildSoundnessRecord {
                    build: build.build().clone(),
                    accounting: build.accounting(),
                })
                .collect(),
        }
    }
}
