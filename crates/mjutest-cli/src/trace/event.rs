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
