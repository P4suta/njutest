// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The wire shape of one trace event: an envelope of sequence number and moment around a typed record. The vocabulary grows with every phase of the engine; the envelope and the run-start / run-end pair are frozen.

use serde::{Deserialize, Serialize};

/// The schema name carried by every `run-start` event. It names the recipe version; a future shape becomes `rust-mutants-trace-v2`.
pub const SCHEMA: &str = "rust-mutants-trace-v1";

/// Every type a recording can hold, in the order [`Payload::type_name`] answers with.
pub const EVERY_TYPE: [&str; 24] = [
    "run-start",
    "phase-start",
    "phase-end",
    "open",
    "snapshot",
    "exec",
    "discover-file",
    "instrument",
    "validate-round",
    "bisect",
    "build",
    "verify",
    "touch",
    "witness",
    "skip-claim",
    "route",
    "cache",
    "select",
    "identical",
    "evidence",
    "kept",
    "mutant-exec",
    "note",
    "run-end",
];

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
    /// One file was searched for candidates.
    DiscoverFile {
        /// The record.
        discover: DiscoverFileRecord,
    },
    /// One file was rewritten to hold its mutants.
    Instrument {
        /// The record.
        instrument: InstrumentRecord,
    },
    /// One round of "condemn what the compiler refused and compile again".
    ValidateRound {
        /// The record.
        round: ValidateRoundRecord,
    },
    /// The suspects of an unattributable failure were narrowed by halving.
    Bisect {
        /// The record.
        bisect: BisectRecord,
    },
    /// The test binaries were named.
    Build {
        /// The record.
        build: BuildRecord,
    },
    /// One target was run with nothing active, before anything about a mutant is believed.
    Verify {
        /// The record.
        verify: VerifyRecord,
    },
    /// One target's guards said which of its tests reached them.
    Touch {
        /// The record.
        touch: TouchRecord,
    },
    /// One branch claim was put to the compiler.
    Witness {
        /// The record.
        witness: WitnessRecord,
    },
    /// One `rust-mutants: skip` marker was read, and either hid something or did not.
    SkipClaim {
        /// The record.
        claim: SkipClaimRecord,
    },
    /// A directory the run would have removed was kept, because it was asked to keep it.
    Kept {
        /// The record.
        kept: KeptRecord,
    },
    /// How one mutant's targets were chosen, and which of them ran.
    Route {
        /// The record.
        route: RouteRecord,
    },
    /// What an earlier run of this exact tree said about one mutant, and whether this run used it.
    Cache {
        /// The record.
        cache: CacheRecord,
    },
    /// Why one mutant was never executed.
    Select {
        /// The record.
        select: SelectRecord,
    },
    /// Whether the compiler renders one mutation identically to what it mutates.
    Identical {
        /// The record.
        identical: IdenticalRecord,
    },
    /// A run kept one file an audit re-derives its proofs from.
    Evidence {
        /// The record.
        evidence: EvidenceRecord,
    },
    /// One mutant was executed against one target.
    MutantExec {
        /// The record.
        mutant: MutantExecRecord,
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
            Self::DiscoverFile { .. } => "discover-file",
            Self::Instrument { .. } => "instrument",
            Self::ValidateRound { .. } => "validate-round",
            Self::Bisect { .. } => "bisect",
            Self::Build { .. } => "build",
            Self::Verify { .. } => "verify",
            Self::Touch { .. } => "touch",
            Self::SkipClaim { .. } => "skip-claim",
            Self::Kept { .. } => "kept",
            Self::Witness { .. } => "witness",
            Self::Route { .. } => "route",
            Self::Cache { .. } => "cache",
            Self::Select { .. } => "select",
            Self::Identical { .. } => "identical",
            Self::Evidence { .. } => "evidence",
            Self::MutantExec { .. } => "mutant-exec",
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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ExecRecord {
    /// The command line, verbatim.
    pub argv: Vec<String>,
    /// The working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// The names of the environment variables set for the process. Never a value: the recorder strips `=value` from every entry.
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

/// Every decision syntactic discovery took in one file: each site with the guard form it got or the reason it was passed over, and the skip tallies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverFileRecord {
    /// The workspace-relative path.
    pub path: String,
    /// Candidates the file yielded.
    pub candidates: u32,
    /// Every site, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<SiteRecord>,
    /// The skip tallies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skips: Vec<SkipCount>,
}

/// One site discovery decided on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteRecord {
    /// The 1-based line of the edit.
    pub line: u32,
    /// The 1-based byte column of the edit.
    pub column: u32,
    /// The rule, or the skip reason for a site that is not a rule's (a macro invocation).
    pub rule: String,
    /// The guard form (`C`, `E`, `S`) of a candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    /// Why the site was passed over, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<String>,
    /// What the walk has to say about the decision beyond its reason: the text of a marker, or the shape a rule declined to edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// How many sites one reason passed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipCount {
    /// The reason's name.
    pub reason: String,
    /// The tally.
    pub count: u32,
}

/// One file rewritten to hold its mutants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentRecord {
    /// The workspace-relative path.
    pub path: String,
    /// How many guards were placed.
    pub guards: u32,
    /// The name the generated runtime module took.
    pub module: String,
    /// Lines before the rewrite.
    pub lines_before: u64,
    /// Lines after it, the appended runtime excluded: equal to `lines_before` or the rewrite moved something.
    pub lines_after: u64,
}

/// One validation round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateRoundRecord {
    /// Which round, from 1.
    pub round: u32,
    /// How many mutants were left out of this attempt.
    pub condemned: u32,
    /// Whether the tree compiled.
    pub success: bool,
    /// How many files this round had to write again, which is how many its condemnations changed.
    #[serde(default)]
    pub written: u32,
    /// The mutants an error was inside of, and what the compiler said.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributed: Vec<AttributionRecord>,
    /// The errors no branch accounts for, rendered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unattributed: Vec<String>,
}

/// One error attributed to one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionRecord {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The compiler's error code, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The first line of what it said.
    pub said: String,
}

/// One narrowing of suspects by halving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectRecord {
    /// How many mutants were still live when it started.
    pub suspects: u32,
    /// The mutants it named.
    pub offenders: Vec<u32>,
    /// How many compilations it cost.
    pub attempts: u32,
    /// How many of the offenders it could put the compiler's own words to.
    #[serde(default)]
    pub diagnosed: u32,
}

/// The test binaries a build produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRecord {
    /// The target ids, in order.
    pub targets: Vec<String>,
    /// What each target is, beyond its name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<TargetRecord>,
}

/// One target the build produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRecord {
    /// `package/kind/name`.
    pub id: String,
    /// What kind of target it is.
    pub kind: String,
    /// Whether it is built with the libtest harness, which decides how its silence is read.
    pub harness: bool,
    /// What a run could not establish about it, each named.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
}

/// What was established about one target with nothing active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyRecord {
    /// The target.
    pub target: String,
    /// What the run established, which for a baseline is what it must not have been.
    pub outcome: String,
    /// How many tests ran, when the harness said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_run: Option<u32>,
    /// How long it took, which is what a derived timeout is five times.
    pub duration_ms: u64,
    /// Whether this exact passing measurement was read back instead of running the target again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remembered: bool,
    /// Whether the target had to be run a second time, because the first run did not pass.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retried: bool,
}

/// What one target's guards recorded on the run that verified its baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchRecord {
    /// The target.
    pub target: String,
    /// How many of its tests reached at least one mutation.
    pub tests: u32,
    /// How many distinct mutations anything of it reached, which is the most mutants it can be asked about.
    pub sites: u32,
    /// How many of those were reached where nothing named a test, and so reach every test of the target.
    pub loose: u32,
    /// How many distinct mutations anything of it saw its guard's two branches differ over, which is what could have noticed them.
    #[serde(default)]
    pub infected: u32,
}

/// One branch claim put to the compiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WitnessRecord {
    /// The candidate's dense catalog index.
    pub index: u32,
    /// The witnesses the claim rests on, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub witnesses: Vec<String>,
    /// Whether the compiler accepted them, which is what makes the claim a proof.
    pub checked: bool,
    /// The first line of what refused it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

/// One directory a run was asked to keep rather than remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptRecord {
    /// The directory, as an absolute path.
    pub path: String,
    /// The run that kept it, which is what a reader looks it up by.
    pub run_id: String,
}

/// One `rust-mutants: skip` marker, and whether it hid anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkipClaimRecord {
    /// The workspace-relative path.
    pub path: String,
    /// The 1-based line the marker sits on.
    pub line: u32,
    /// The reason its author wrote.
    pub reason: String,
    /// Whether a place a rule targets starts inside what it speaks about.
    pub matched: bool,
}

/// One target a proof removed from what could have noticed a mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DischargeRecord {
    /// The target.
    pub target: String,
    /// The proof that removed it: `branch-never-taken` or `never-infected`.
    pub proof: String,
}

/// How one mutant's targets were chosen, and which of them ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRecord {
    /// The mutant's short identity.
    pub mutant: String,
    /// Its dense catalog index.
    pub index: u32,
    /// How the route was decided.
    pub granularity: crate::session::Granularity,
    /// What widened the route back to everything, when something did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<crate::session::Fallback>,
    /// The targets that could notice the mutation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reaching: Vec<String>,
    /// The targets that were measured, asked, and did not reach the mutation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub considered: Vec<String>,
    /// The targets a proof removed, each with the proof's name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discharged: Vec<DischargeRecord>,
    /// The targets that actually ran, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executed: Vec<String>,
    /// The run this outcome was read back from, when it was not established here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused: Option<String>,
}

/// One mutant executed against one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutantExecRecord {
    /// The mutant's full identity.
    pub id: String,
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The target that ran.
    pub target: String,
    /// What the execution established.
    pub outcome: String,
    /// The exit status, or the runner's stand-in.
    pub exit_code: i32,
    /// How long it took.
    pub duration_ms: u64,
    /// How many tests ran, when the harness said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_run: Option<u32>,
    /// The signal the process died from, on the platforms that have them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    /// Every test that failed, by name, which is what says who noticed the mutation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_tests: Vec<String>,
    /// How long this execution was given.
    #[serde(default)]
    pub timeout_ms: u64,
    /// Where that budget came from: `configured` or `derived`.
    #[serde(default)]
    pub timeout_source: String,
    /// Whether it ran with nothing else this run started running beside it, which is what a confirming retry does.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alone: bool,
}

/// What the outcome store was asked about one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheRecord {
    /// The mutant's display identity.
    pub mutant: String,
    /// The key the record was looked up under, which is what says two runs asked the same question.
    pub key: String,
    /// Whether a record answered.
    pub hit: bool,
    /// The run that established it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
}

/// Why one mutant was never executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectRecord {
    /// The mutant's display identity.
    pub mutant: String,
    /// `unreached`, `discharged`, or `interrupted`.
    pub reason: String,
}

/// What the equivalence layer said about one mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdenticalRecord {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// `identical`, `differs`, or `not-established`.
    pub identity: String,
    /// Why nothing was established, when nothing was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One file a run kept beside its report, so an audit can re-derive what the run decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    /// The path, relative to the run's own directory.
    pub file: String,
    /// How many bytes it holds.
    pub bytes: u64,
    /// The lowercase hex SHA-256 of those bytes.
    pub digest: String,
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
