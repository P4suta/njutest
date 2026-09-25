// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The wire shape of one trace event: an envelope of sequence number and moment around a typed record.
//! The vocabulary grows with every phase of the engine; the envelope and the run-start / run-end pair are frozen.

use serde::{Deserialize, Serialize};

use crate::cargo::{BuildSelection, BuildSelectionDigest};
use crate::id::RunId;

/// The schema name carried by every `run-start` event.
/// It names the recipe version; a future incompatible shape takes a new version.
pub const SCHEMA: &str = "rust-mutants-trace-v1";

/// Every type a recording can hold, in the order [`Payload::type_name`] answers with.
pub const EVERY_TYPE: [&str; 25] = [
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
    "perturbed-control",
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
/// What one event of a recording says.
///
/// Closed, because both CLIs render it and one of them had a waiver in `xtask/wildcard_allowlist.txt` for the arm this type once forced on it.
/// A ledger entry is what paying the cost looks like (ADR 0023).
#[serde(deny_unknown_fields)]
pub enum Payload {
    /// The first event of every recording.
    RunStart {
        /// [`SCHEMA`].
        schema: String,
        /// The engine version that recorded.
        engine: String,
        /// Whether the engine was invoked directly or as one bound build of an `njutest` run.
        context: TraceContext,
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
    /// One control started under a perturbation came to an end, kept apart from every other control so that nothing comparing a control with its baseline under equal conditions reads it.
    PerturbedControl {
        /// The record.
        perturbed: PerturbedRecord,
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

/// The invocation boundary a recording belongs to.
///
/// This is a closed union rather than optional `njutest` fields: a standalone engine run and a runner-owned configured build cannot be confused by partially populated JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub enum TraceContext {
    /// `rust-mutants` was invoked as the top-level tool.
    Standalone {
        /// The canonical run directory identity.
        run_id: RunId,
        /// The canonical digest of the Cargo options selected for this run.
        build_selection: BuildSelectionDigest,
    },
    /// `njutest` invoked the engine for exactly one configured build.
    Njutest {
        /// The complete nested-build binding.
        build: NjutestBuild,
    },
}

/// The immutable binding between one nested engine trace and its configured `njutest` build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NjutestBuild {
    final_run_id: RunId,
    internal_run_id: RunId,
    ordinal: u32,
    name: String,
    build_selection: BuildSelectionDigest,
}

impl NjutestBuild {
    /// Constructs a nested trace binding after proving that the internal run id is the canonical id for this final run and ordinal, and that the report-visible build name is nonempty.
    ///
    /// # Errors
    /// Returns the violated binding invariant.
    /// No partial nested context is representable.
    pub fn new(
        final_run_id: RunId,
        ordinal: u32,
        name: String,
        build: &BuildSelection,
    ) -> Result<Self, NjutestBuildError> {
        let internal_run_id = nested_run_id(&final_run_id, ordinal)?;
        let candidate = Self {
            final_run_id,
            internal_run_id,
            ordinal,
            name,
            build_selection: build.digest().clone(),
        };
        candidate.validate()?;
        Ok(candidate)
    }

    /// The final, report-visible run id shared by all configured builds.
    #[must_use]
    pub const fn final_run_id(&self) -> &RunId {
        &self.final_run_id
    }

    /// The internal run id used for this build's scratch and engine work.
    #[must_use]
    pub const fn internal_run_id(&self) -> &RunId {
        &self.internal_run_id
    }

    /// The zero-based position in the configured build ledger.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// The exact report-visible build name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The canonical digest of the Cargo options selected for this build.
    #[must_use]
    pub const fn build_selection(&self) -> &BuildSelectionDigest {
        &self.build_selection
    }

    fn validate(&self) -> Result<(), NjutestBuildError> {
        if self.name.trim().is_empty() || self.name.trim() != self.name {
            return Err(NjutestBuildError::InvalidName {
                name: self.name.clone(),
            });
        }
        let expected = nested_run_id(&self.final_run_id, self.ordinal)?;
        if self.internal_run_id != expected {
            return Err(NjutestBuildError::InternalRunId {
                expected,
                actual: self.internal_run_id.clone(),
            });
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for NjutestBuild {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = NjutestBuildWire::deserialize(deserializer)?;
        Self::try_from(wire).map_err(serde::de::Error::custom)
    }
}

/// Why a nested engine trace binding cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NjutestBuildError {
    /// A report-visible build name must contain non-whitespace and carry its canonical unpadded spelling.
    #[error("configured build name {name:?} must be nonempty and have no surrounding whitespace")]
    InvalidName {
        /// The refused name.
        name: String,
    },
    /// Appending the ordinal did not produce a canonical writable run id.
    #[error("configured build {ordinal} cannot be named under final run {final_run_id}: {source}")]
    RunId {
        /// The final run.
        final_run_id: RunId,
        /// The build ordinal.
        ordinal: u32,
        /// Why the composed id was invalid.
        source: crate::id::RunIdError,
    },
    /// The supplied internal id is not the one derived from the final run and ordinal.
    #[error("nested trace internal run id {actual} must be {expected}")]
    InternalRunId {
        /// The only canonical internal id for the binding.
        expected: RunId,
        /// The refused id.
        actual: RunId,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NjutestBuildWire {
    final_run_id: RunId,
    internal_run_id: RunId,
    ordinal: u32,
    name: String,
    build_selection: BuildSelectionDigest,
}

impl TryFrom<NjutestBuildWire> for NjutestBuild {
    type Error = NjutestBuildError;

    fn try_from(wire: NjutestBuildWire) -> Result<Self, Self::Error> {
        let NjutestBuildWire {
            final_run_id,
            internal_run_id,
            ordinal,
            name,
            build_selection,
        } = wire;
        let candidate = Self {
            final_run_id,
            internal_run_id,
            ordinal,
            name,
            build_selection,
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

fn nested_run_id(final_run_id: &RunId, ordinal: u32) -> Result<RunId, NjutestBuildError> {
    RunId::try_from(format!("{final_run_id}-b{ordinal:010}")).map_err(|source| {
        NjutestBuildError::RunId {
            final_run_id: final_run_id.clone(),
            ordinal,
            source,
        }
    })
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
            Self::PerturbedControl { .. } => "perturbed-control",
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
#[serde(deny_unknown_fields)]
pub struct PhaseRecord {
    /// The phase's name.
    pub name: String,
    /// How long it took; only on `phase-end`.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub duration_ms: Option<u64>,
}

/// What a sweep of the temporary area did on the way in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct OpenRecord {
    /// The absolute source root.
    pub root: String,
    /// The snapshot directory.
    pub snapshot_dir: String,
    /// Whether the snapshot got its stable name.
    pub stable_dir: bool,
    /// The sweep that ran first, if one did.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub sweep: Option<SweepRecord>,
}

/// A snapshot was taken, or refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub workspace_digest: Option<String>,
    /// How long the copy took.
    pub duration_ms: u64,
    /// The refusal, rendered, when there was one.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub error: Option<String>,
}

/// One process execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRecord {
    /// The command line, verbatim.
    pub argv: Vec<String>,
    /// The working directory.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub dir: Option<String>,
    /// The names of the environment variables set for the process.
    /// Never a value: the recorder strips `=value` from every entry.
    pub env_names: Vec<String>,
    /// The bound on the whole execution, if one applied; for one watched for progress, the ceiling.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub timeout_ms: Option<u64>,
    /// How long the process could go without raising its step count, where it was watched for that.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub quiet_ms: Option<u64>,
    /// How the process came to an end.
    pub stopped: crate::execute::Stopped,
    /// How long the process ran.
    pub duration_ms: u64,
    /// Bytes of output captured.
    pub output_bytes: u64,
    /// The SHA-256 of the whole capture, when there was any.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub output_sha256: Option<String>,
    /// Whether the preserved copy was cut at [`super::OUTPUT_FILE_LIMIT`].
    pub output_truncated: bool,
    /// Where a sink preserved the output, relative to its directory.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub output_path: Option<String>,
    /// The failure to start or wait, rendered.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub error: Option<String>,
    /// The raw capture, for a sink that preserves it.
    /// Never serialized.
    #[serde(skip)]
    pub output: Vec<u8>,
}

/// Every decision syntactic discovery took in one file: each site with the guard form it got or the reason it was passed over, and the skip tallies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverFileRecord {
    /// The workspace-relative path.
    pub path: String,
    /// Candidates the file yielded.
    pub candidates: u32,
    /// Every site, in source order.
    pub sites: Vec<SiteRecord>,
    /// The skip tallies.
    pub skips: Vec<SkipCount>,
}

/// One site discovery decided on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteRecord {
    /// The 1-based line of the edit.
    pub line: u32,
    /// The 1-based byte column of the edit.
    pub column: u32,
    /// The rule, or the skip reason for a site that is not a rule's (a macro invocation).
    pub rule: String,
    /// The guard form (`C`, `E`, `S`) of a candidate.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub form: Option<String>,
    /// Why the site was passed over, when it was.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub skip: Option<String>,
    /// What the walk has to say about the decision beyond its reason: the text of a marker, or the shape a rule declined to edit.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub note: Option<String>,
}

/// How many sites one reason passed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkipCount {
    /// The reason's name.
    pub reason: String,
    /// The tally.
    pub count: u32,
}

/// One file rewritten to hold its mutants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ValidateRoundRecord {
    /// Which round, from 1.
    pub round: u32,
    /// How many mutants were left out of this attempt.
    pub condemned: u32,
    /// Whether the tree compiled.
    pub success: bool,
    /// How many files this round had to write again, which is how many its condemnations changed.
    pub written: u32,
    /// The mutants an error was inside of, and what the compiler said.
    pub attributed: Vec<AttributionRecord>,
    /// The errors no branch accounts for, rendered.
    pub unattributed: Vec<String>,
}

/// One error attributed to one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributionRecord {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The compiler's error code, when it had one.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub code: Option<String>,
    /// The first line of what it said.
    pub said: String,
}

/// One narrowing of suspects by halving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BisectRecord {
    /// How many mutants were still live when it started.
    pub suspects: u32,
    /// The mutants it named.
    pub offenders: Vec<u32>,
    /// How many compilations it cost.
    pub attempts: u32,
    /// How many of the offenders it could put the compiler's own words to.
    pub diagnosed: u32,
}

/// The test binaries a build produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildRecord {
    /// The target ids, in order.
    pub targets: Vec<String>,
    /// What each target is, beyond its name.
    pub details: Vec<TargetRecord>,
}

/// One target the build produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRecord {
    /// `package/kind/name`.
    pub id: String,
    /// What kind of target it is.
    pub kind: String,
    /// Whether it is built with the libtest harness, which decides how its silence is read.
    pub harness: bool,
    /// What a run could not establish about it, each named.
    pub limitations: Vec<String>,
}

/// What was established about one target with nothing active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyRecord {
    /// The target.
    pub target: String,
    /// What the run established, which for a baseline is what it must not have been.
    pub outcome: String,
    /// How many tests ran, when the harness said.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub tests_run: Option<u32>,
    /// How long it took, which is what a derived timeout is five times.
    pub duration_ms: u64,
    /// Whether this exact passing measurement was read back instead of running the target again.
    pub remembered: bool,
    /// Whether the target had to be run a second time, because the first run did not pass.
    pub retried: bool,
}

/// Which whole-target run with nothing active one touch record was measured on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Measurement {
    /// The one run of every target that verifies the baseline, which routing rests on.
    Baseline,
    /// An original-code control of the whole target, run to confirm a kill.
    Control,
    /// A run of one mutation against a target whose reach moved, whose record says whether it reached the mutation's site (ADR 0036).
    Repair,
}

/// One variable a perturbed control was started with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetRecord {
    /// The name a process reads it by.
    pub name: String,
    /// Its value, or nothing where the value is not text.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub value: Option<String>,
}

/// What a control was started with beyond what its baseline was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerturbationRecord {
    /// Each variable set over the environment.
    pub environment: Vec<SetRecord>,
    /// What the shell it was started through ran before it became the test binary, or nothing.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub launcher: Option<String>,
    /// The harness arguments its schedule added after the baseline's.
    pub arguments: Vec<String>,
}

/// What one control started under a perturbation came to, and what became of the reach it could have recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerturbedRecord {
    /// The target.
    pub target: String,
    /// What it was started with beyond what its baseline was.
    pub perturbation: PerturbationRecord,
    /// What it came to, by the outcome's name.
    pub outcome: String,
    /// The tests that failed, as the harness named them.
    pub failed_tests: Vec<String>,
    /// How long it ran.
    pub duration_ms: u64,
    /// What became of the reach it could have recorded.
    pub reach: ReachRecord,
}

/// What became of the reach one perturbed control could have recorded, each case said where it was decided, so that a reader tells a control never asked from one whose record failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReachRecord {
    /// It was not asked to record: it observed nothing, ran one test, or is a target whose guards cannot be asked, a doctest or one run through cargo.
    NotAsked,
    /// It did not pass, so what it reached is a failing run's and was not read.
    NotRead,
    /// Its process could not write what its guards reached.
    Unrecorded,
    /// What it wrote did not read back.
    Unreadable,
    /// What its guards recorded.
    Recorded {
        /// The record, whose `measured` is `control`.
        touch: TouchRecord,
    },
}

/// What one target's guards recorded on one whole run of it with nothing active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TouchRecord {
    /// The target.
    pub target: String,
    /// Which run it was measured on.
    pub measured: Measurement,
    /// The mutation a repair ran, in full; nothing on a baseline or a control, which run with nothing active.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub mutant: Option<String>,
    /// The tests that run passed, which is what everything below is the reach of.
    pub passed: Vec<String>,
    /// What the run's own summary said, in the protocol it answered in; under libtest a comparison stands only where its count equals the length of `passed`.
    pub summary: SummaryRecord,
    /// Every mutant site anything of it reached, in index order.
    pub reached_sites: Vec<u32>,
    /// Every branch body anything of it entered, by the marker at the body's first statement, in index order.
    pub entered_bodies: Vec<u32>,
    /// Every mutation anything of it saw its guard's two branches differ over, in index order.
    pub infected_sites: Vec<u32>,
    /// How many of its tests reached at least one mutation.
    pub tests: u32,
    /// How many distinct mutations anything of it reached, which is the most mutants it can be asked about.
    pub sites: u32,
    /// How many of those were reached where nothing named a test, and so reach every test of the target.
    pub loose: u32,
    /// How many distinct mutations anything of it saw its guard's two branches differ over, which is what could have noticed them.
    pub infected: u32,
    /// How many distinct items anything of it entered the body of, which is the union a change to one of them is routed by.
    pub entered: u32,
    /// Every item anything of it entered the body of, in index order, which is what a comparison of two runs holds against each other.
    pub entered_items: Vec<u32>,
}

/// What a run's own summary said, in the protocol it answered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SummaryRecord {
    /// libtest, with how many tests its summary said ran, or nothing where it printed none.
    Libtest {
        /// The summary's count.
        tests_run: Option<u32>,
    },
    /// A harness that answers by exit code, names no test, and prints no summary.
    Custom,
    /// No process answered.
    Unanswered,
    /// A baseline remembered from an earlier session, whose run is not here to say.
    Remembered,
}

impl SummaryRecord {
    /// What `result` said, in the protocol it answered in.
    #[must_use]
    pub fn of(result: &crate::execute::MutantResult) -> Self {
        match result.protocol {
            crate::execute::Protocol::Libtest => Self::Libtest {
                tests_run: result.tests_run(),
            },
            crate::execute::Protocol::Custom => Self::Custom,
            crate::execute::Protocol::Unanswered => Self::Unanswered,
        }
    }
}

/// One branch claim put to the compiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WitnessRecord {
    /// The candidate's dense catalog index.
    pub index: u32,
    /// The witnesses the claim rests on, by name.
    pub witnesses: Vec<String>,
    /// Whether the compiler accepted them, which is what makes the claim a proof.
    pub checked: bool,
    /// The first line of what refused it, when one did.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub diagnostic: Option<String>,
}

/// One directory a run was asked to keep rather than remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptRecord {
    /// The directory, as an absolute path.
    pub path: String,
    /// The run that kept it, which is what a reader looks it up by.
    pub run_id: String,
}

/// One `rust-mutants: skip` marker, and whether it hid anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct DischargeRecord {
    /// The target.
    pub target: String,
    /// The proof that removed it: `branch-never-taken` or `never-infected`.
    pub proof: String,
}

/// How one mutant's targets were chosen, and which of them ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRecord {
    /// The mutant's short identity.
    pub mutant: String,
    /// Its dense catalog index.
    pub index: u32,
    /// How the route was decided.
    pub granularity: crate::session::Granularity,
    /// What widened the route back to everything, when something did.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub fallback: Option<crate::session::Fallback>,
    /// The targets that could notice the mutation.
    pub reaching: Vec<String>,
    /// The targets that were measured, asked, and did not reach the mutation.
    pub considered: Vec<String>,
    /// The targets a proof removed, each with the proof's name.
    pub discharged: Vec<DischargeRecord>,
    /// The targets that actually ran, in order.
    pub executed: Vec<String>,
    /// The run this outcome was read back from, when it was not established here.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub reused: Option<String>,
}

/// One mutant executed against one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutantExecRecord {
    /// The mutant's full identity.
    pub id: String,
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The target that ran.
    pub target: String,
    /// What the execution established.
    pub outcome: String,
    /// The verified runtime notice when this execution reached its step limit.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub step_notice: Option<crate::execute::StepLimitNotice>,
    /// The exit status, or the runner's stand-in.
    pub exit_code: i32,
    /// How long it took.
    pub duration_ms: u64,
    /// How many tests ran, when the harness said.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub tests_run: Option<u32>,
    /// The signal the process died from, on the platforms that have them.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub signal: Option<i32>,
    /// Every test that failed, by name, which is what says who noticed the mutation.
    pub failed_tests: Vec<String>,
    /// How long this execution was given.
    pub timeout_ms: u64,
    /// Where that budget came from: `configured` or `derived`.
    pub timeout_source: String,
    /// Whether it ran with nothing else this run started running beside it, which is what a confirming retry does.
    pub alone: bool,
}

/// What the outcome store was asked about one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheRecord {
    /// The mutant's display identity.
    pub mutant: String,
    /// The key the record was looked up under, which is what says two runs asked the same question.
    pub key: String,
    /// Whether a record answered.
    pub hit: bool,
    /// The run that established it, when one did.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub source_run_id: Option<String>,
}

/// Why one mutant was never executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectRecord {
    /// The mutant's display identity.
    pub mutant: String,
    /// `unreached`, `discharged`, or `interrupted`.
    pub reason: String,
}

/// What the equivalence layer said about one mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdenticalRecord {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// `identical`, `differs`, or `not-established`.
    pub identity: String,
    /// Why nothing was established, when nothing was.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub detail: Option<String>,
}

/// One file a run kept beside its report, so an audit can re-derive what the run decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct NoteRecord {
    /// What kind of note.
    pub kind: String,
    /// The note.
    pub detail: String,
}

/// How a recorded run ended.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum RunOutcome {
    /// Every mutation the run judged was noticed.
    Detected,
    /// The run reported a finding a person has to act on: a survivor, a stale claim, or something it could not decide.
    Found,
    /// The run was cancelled before it finished.
    Interrupted,
    /// The run stopped on an error.
    Failed,
    /// The run finished and left judging what it measured to the program that embedded it.
    Completed,
}

impl RunOutcome {
    /// The name a recording spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Found => "found",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }
}

/// The accounting that closes a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    /// How the run ended.
    pub outcome: RunOutcome,
    /// The error that ended it, rendered, if one did.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub error: Option<String>,
    /// Events the sink kept before this one.
    pub events_emitted: u64,
    /// Events the sink could not keep before this one.
    pub events_dropped: u64,
}
