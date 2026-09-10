// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The wire shape of one trace event: an envelope of sequence number and moment around a typed record.

use serde::{Deserialize, Serialize};

use crate::config::Contract;
use crate::report::{Accounting, RunKind};

/// The schema name carried by every `run-start` event. It names the recipe version; a future shape becomes `mjutest-trace-v2`.
pub const SCHEMA: &str = "mjutest-trace-v1";

/// One event of a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Monotonic from 1; delivery order is sequence order.
    pub seq: u64,
    /// The moment the event was recorded, RFC 3339 in UTC.
    pub timestamp: String,
    /// Milliseconds since the recording started.
    pub elapsed_ms: u64,
    /// The typed record.
    #[serde(flatten)]
    pub payload: Payload,
}

/// The typed record of an event, tagged by `type` on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
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
    /// What the probe pass measured for one target.
    ProbeExec {
        /// The record.
        probe: ProbeExecRecord,
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
            Self::ProbeExec { .. } => "probe-exec",
            Self::Note { .. } => "note",
            Self::RunEnd { .. } => "run-end",
        }
    }
}

/// What the run is: enough to tell two recordings apart without reading them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRecord {
    /// [`SCHEMA`].
    pub schema: String,
    /// The runner version that recorded.
    pub mjutest: String,
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
            mjutest: crate::VERSION.to_owned(),
            rust_mutants: rust_mutants::VERSION.to_owned(),
            run_id: run_id.to_owned(),
            run_kind,
            contract,
        }
    }
}

/// One phase boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseRecord {
    /// What the phase is called.
    pub name: String,
    /// How long it took, on the end event alone.
    pub duration_ms: Option<u64>,
}

/// One executed process.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ExecRecord {
    /// The command line, verbatim.
    pub argv: Vec<String>,
    /// The directory it ran in.
    pub dir: Option<String>,
    /// The names of the variables it ran with, never the values.
    pub env_names: Vec<String>,
    /// The bound the caller put on it.
    pub timeout_ms: Option<u64>,
    /// What it exited with, when it exited at all. Absent is absent: the engine's sentinel does not travel, because a reader would have to know it to avoid reading it as a status.
    pub exit_code: Option<i32>,
    /// Whether the bound is why it stopped.
    pub timed_out: bool,
    /// How long it took.
    pub duration_ms: u64,
    /// How much it said.
    pub output_bytes: u64,
    /// The digest of everything it said.
    pub output_sha256: Option<String>,
    /// Whether the preserved copy was cut.
    pub output_truncated: bool,
    /// Where the preserved copy is, relative to the recording directory.
    pub output_path: Option<String>,
    /// Why it could not be run, when it could not.
    pub error: Option<String>,
    /// The capture itself, for a sink that preserves it. Never serialized.
    #[serde(skip)]
    pub output: Vec<u8>,
}

impl ExecRecord {
    /// The record of one supervised run: the spec's command line, directory, environment names, and timeout, and the result's exit code, timeout flag, duration, output, and error.
    #[must_use]
    pub fn of(spec: &rust_mutants::runner::Spec, result: &rust_mutants::runner::RunResult) -> Self {
        Self {
            argv: spec
                .argv
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            dir: spec
                .dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned()),
            env_names: spec
                .env
                .as_ref()
                .map(|env| {
                    env.iter()
                        .map(|(key, _)| key.to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default(),
            timeout_ms: spec
                .timeout
                .map(|timeout| u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)),
            exit_code: (result.exit_code != rust_mutants::runner::EXIT_CODE_UNAVAILABLE)
                .then_some(result.exit_code),
            timed_out: result.timed_out,
            duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
            output_bytes: 0,
            output_sha256: None,
            output_truncated: false,
            output_path: None,
            error: result.error.as_ref().map(ToString::to_string),
            output: result.output.clone(),
        }
    }
}

/// How far the run had got, as the user interface saw it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProgressRecord {
    /// What was happening.
    pub message: String,
    /// How many of it are done.
    pub done: Option<u64>,
    /// How many there are.
    pub total: Option<u64>,
}

/// Something the run kept for a person to look at.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArtifactRecord {
    /// What kind of thing it is.
    pub kind: String,
    /// Where it is.
    pub path: String,
    /// How big it is, when that was measured.
    pub bytes: Option<u64>,
}

/// One target a proof removed from a reaching set, beside the proof that removed it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DischargeRecord {
    /// The target that was not run.
    pub target: String,
    /// What removed it.
    pub proof: String,
}

/// How one mutant's tests were chosen, and what narrowed the choice.
///
/// [ADR 0004](../../../../docs/adr/0004-proof-layers-not-budgets.md) decision 4
/// asks that every layer be visible: the granularity a route was decided at,
/// the fallback that widened it, and every target a proof discharged with the
/// proof's name. A reader who sees a run go faster asks which proof did it, and
/// the answer is here rather than in a configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RouteRecord {
    /// The mutant a person types.
    pub mutant: String,
    /// `all`, `block`, `test`, `discharged`, or `unreached`.
    pub granularity: String,
    /// What the measurement could not support, on a route it did not decide on its own. Every one of these widened the route.
    pub fallback: Option<String>,
    /// The targets to run, cheapest first.
    pub reaching: Vec<String>,
    /// Which tests of a target the mutation is put to, for each target a measurement narrowed. A target that is not named here runs every test it has.
    pub tests: Vec<AskedRecord>,
    /// The targets a proof removed, each beside the proof that removed it.
    pub discharged: Vec<DischargeRecord>,
    /// The measured targets that were asked and did not reach the mutation.
    ///
    /// A layer that removes an execution names who was in a position to notice
    /// and did not. A count cannot be checked against anything; a list can be
    /// held to the targets the run says it measured, which is what
    /// `xtask proofaudit` does with it.
    pub considered: Vec<String>,
    /// The run this disposition was read back from, when it was not established here.
    pub reused: Option<String>,
    /// Why the answer an earlier run left was not the one this run used, when there was a store to ask.
    ///
    /// A believed record is an execution that did not happen, so reuse is a
    /// layer like any other and decision 4 asks the same of it. Saying only
    /// when it applied leaves a run that reuses nothing looking exactly like a
    /// run with nothing to reuse, and a store that has quietly stopped
    /// answering is the failure this field is here to make visible.
    #[serde(default)]
    pub refused: Option<String>,
}

/// Which tests of one target a route puts the mutation to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AskedRecord {
    /// The target.
    pub target: String,
    /// The tests, by their libtest path.
    pub tests: Vec<String>,
}

/// One mutant run against one target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MutantExecRecord {
    /// The mutant a person types.
    pub mutant: String,
    /// The target it ran against, or `package-suite` when no proof said which tests could notice it.
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

/// What the probe pass measured for one target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProbeExecRecord {
    /// The target.
    pub target: String,
    /// `measured` when the pass read that target's log, `not-measured` when it did not.
    pub outcome: String,
    /// How many mutants the target infected. A target the pass did not measure carries no facts, and none is not zero.
    pub infected: Option<u64>,
}

/// A free-form note, for what has no shape of its own yet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NoteRecord {
    /// What kind of note it is, so a reader can filter.
    pub kind: String,
    /// What it says.
    pub detail: String,
}

/// What the run concluded, and what the recording lost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// The verdict, or what stopped the run from reaching one.
    pub verdict: String,
    /// What the run counted, when it got far enough to count.
    pub accounting: Option<Accounting>,
    /// The error that ended the run, when one did.
    pub error: Option<String>,
    /// Events the sink kept before this one. A recording cannot count the event it is writing, so this is the honest number rather than a guess that the last one lands.
    pub events_emitted: u64,
    /// Events the sink lost before this one. A recording is honest about its own losses; a reader finds anything lost afterwards as a sequence gap.
    pub events_dropped: u64,
}
