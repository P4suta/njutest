// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The wire shape of one trace event: an envelope of sequence number and
//! moment around a typed record. The vocabulary grows with every phase of
//! the engine; the envelope and the run-start / run-end pair are frozen.

use serde::{Deserialize, Serialize};

/// The schema name carried by every `run-start` event. It names the recipe
/// version; a future shape becomes `rust-mutants-trace-v2`.
pub const SCHEMA: &str = "rust-mutants-trace-v1";

/// One event of a recording.
///
/// The envelope is `seq` (monotonic from 1, in delivery order), `timestamp`
/// (RFC 3339, UTC), `elapsed_ms` (since the recording started), and the
/// record's `type` with its fields.
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
        /// [`SCHEMA`].
        schema: String,
        /// The engine version that recorded.
        engine: String,
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
    /// A workspace was opened.
    Open {
        /// The record.
        open: OpenRecord,
    },
    /// A snapshot was taken, or refused.
    Snapshot {
        /// The record.
        snapshot: SnapshotRecord,
    },
    /// A process ran.
    Exec {
        /// The record.
        exec: ExecRecord,
    },
    /// A free-form note: progress, a decision, a limitation.
    Note {
        /// The record.
        note: NoteRecord,
    },
    /// The last event of a finished recording, with the accounting.
    RunEnd {
        /// The record.
        run: RunRecord,
    },
}

impl Payload {
    /// The `type` as it appears on the wire.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::RunStart { .. } => "run-start",
            Self::PhaseStart { .. } => "phase-start",
            Self::PhaseEnd { .. } => "phase-end",
            Self::Open { .. } => "open",
            Self::Snapshot { .. } => "snapshot",
            Self::Exec { .. } => "exec",
            Self::Note { .. } => "note",
            Self::RunEnd { .. } => "run-end",
        }
    }
}

/// A phase boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseRecord {
    /// The phase's name.
    pub name: String,
    /// How long it took; only on `phase-end`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// What a sweep of the temporary area did on the way in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepRecord {
    /// The directory swept.
    pub parent: String,
    /// Directories removed.
    pub removed: u64,
    /// Bytes they held, as far as the walk could measure.
    pub removed_bytes: u64,
    /// Directories still locked by a running process.
    pub live: u64,
    /// Directories preserved on purpose.
    pub kept: u64,
    /// Directories that could not be judged or removed.
    pub failures: u64,
}

/// A workspace was opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRecord {
    /// The absolute source root.
    pub root: String,
    /// The snapshot directory.
    pub snapshot_dir: String,
    /// Whether the snapshot got its stable name.
    pub stable_dir: bool,
    /// The sweep that ran first, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<SweepRecord>,
}

/// A snapshot was taken, or refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRecord {
    /// The tree copied.
    pub source_root: String,
    /// Where the copy landed.
    pub dir: String,
    /// Regular files copied.
    pub files: u64,
    /// Bytes copied.
    pub bytes: u64,
    /// The workspace digest, absent when the snapshot was refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_digest: Option<String>,
    /// How long the copy took.
    pub duration_ms: u64,
    /// The refusal, rendered, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One process execution.
///
/// The recorder reduces `env_names` to names and digests `output` into
/// `output_bytes` and `output_sha256`; a caller may hand over the raw entries
/// and the raw capture and the event keeps neither.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ExecRecord {
    /// The command line, verbatim.
    pub argv: Vec<String>,
    /// The working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// The names of the environment variables set for the process. Never a
    /// value: the recorder strips `=value` from every entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_names: Vec<String>,
    /// The timeout, if one applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// The exit code, or the runner's stand-in when there is none.
    pub exit_code: i32,
    /// Whether the timeout fired.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub timed_out: bool,
    /// How long the process ran.
    pub duration_ms: u64,
    /// Bytes of output captured.
    #[serde(default)]
    pub output_bytes: u64,
    /// The SHA-256 of the whole capture, when there was any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_sha256: Option<String>,
    /// Whether the preserved copy was cut at [`super::OUTPUT_FILE_LIMIT`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub output_truncated: bool,
    /// Where a sink preserved the output, relative to its directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    /// The failure to start or wait, rendered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The raw capture, for a sink that preserves it. Never serialized.
    #[serde(skip)]
    pub output: Vec<u8>,
}

/// A free-form note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRecord {
    /// What kind of note.
    pub kind: String,
    /// The note.
    pub detail: String,
}

/// The accounting that closes a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// How the run ended, in the caller's words.
    pub outcome: String,
    /// The error that ended it, rendered, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Events the sink kept before this one.
    pub events_emitted: u64,
    /// Events the sink could not keep before this one.
    pub events_dropped: u64,
}
