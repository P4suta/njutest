// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failure modes of the runner, each with a stable code documented in `docs/errors.md`.

/// The codes, and the one place an [`ErrorCode`] is made.
mod table {
    /// A stable, searchable identifier for one failure mode.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ErrorCode {
        /// The code, e.g. `NJ0001`.
        pub code: &'static str,
        /// One line saying what the code means.
        pub summary: &'static str,
        /// What to do about it.
        /// Every code carries one.
        pub remedy: &'static str,
        sealed: Sealed,
    }

    /// What only this module can write, so only this module makes an [`ErrorCode`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Sealed;

    /// Every failure mode, one variant per code, in code order.
    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants,
    )]
    pub enum NjCode {
        /// The caller cancelled the operation before it completed.
        Interrupted,
        /// The command's output stream could not be written.
        OutputUnwritable,
        /// The configuration file could not be read.
        ConfigUnreadable,
        /// The configuration file is not the document this version understands.
        ConfigUnparsable,
        /// The configuration says something a run cannot honour.
        ConfigInvalid,
        /// The configuration names a version this release does not understand.
        ConfigUnsupportedVersion,
        /// A configuration file is already there.
        ConfigExists,
        /// The tree a run is about could not be read.
        EvidenceUnreadable,
        /// The tree changed while it was being measured.
        TreeWrittenDuringMeasurement,
        /// A test binary could not be asked what tests it holds.
        TargetListFailed,
        /// The build could not be run.
        BuildFailed,
        /// The build's output could not be read.
        BuildUnreadable,
        /// A coverage export could not be read.
        CoverageUnreadable,
        /// The LLVM tools the toolchain ships are not installed.
        CoverageToolsMissing,
        /// Llvm-profdata or llvm-cov failed.
        CoverageToolFailed,
        /// A test process wrote no coverage profile at all.
        CoverageNothingWritten,
        /// A provider could not be started.
        ProviderUnstartable,
        /// A provider said nothing in the time it was given.
        ProviderTimeout,
        /// A provider said something this version does not understand.
        ProviderProtocol,
        /// A provider said it could not do what it was asked.
        ProviderRefused,
        /// A provider offered an environment variable a run composes itself.
        ResourceEnvironmentRefused,
        /// A generation provider said something this version does not understand.
        GenerationProtocol,
        /// A generation provider would write where it may not.
        GenerationPathRefused,
        /// The file a candidate patches is not the file the provider saw.
        GenerationPreimageMoved,
        /// A routing layer did not route the mutant planted for it.
        SentinelBlind,
        /// The report could not be written as JSON.
        ReportUnserializable,
        /// A document is not the assurance report this version understands.
        ReportUnreadable,
        /// The report contradicts itself and was not written.
        ReportUnsound,
        /// The report could not be written where a reader will look for it.
        ReportNotKept,
        /// There is no such run to answer about.
        RunNotFound,
        /// The run made no change in anything the subject names.
        SubjectNotCataloged,
        /// The measurement a selection reads could not be written.
        MeasurementUnwritable,
        /// There is no measurement to select by, or it is not one this release reads.
        MeasurementUnreadable,
        /// The toolchain has no miri, and this contract promises interpretation.
        MiriMissing,
        /// The verified model-checking phase could not preserve its own evidence.
        ModelPhaseFailed,
        /// The run could not schedule measurements without trusting interrupted state.
        SchedulerUnusable,
        /// An assurance phase printed output that is not valid UTF-8.
        PhaseOutputUnreadable,
        /// The run ran out of descriptors or memory while reading the sources a proof rests on.
        SourcesUnreadable,
        /// The run has nowhere to work.
        ScratchUnusable,
        /// The store of earlier answers could not be used.
        CacheUnusable,
        /// A stored answer is not the answer it claims to be.
        CacheCorrupt,
        /// No port could be listened on in front of a seam, so nothing could be recorded about it.
        WireCannotListen,
        /// The reports offered are not the parts of one catalog.
        MergeRefused,
    }

    impl ErrorCode {
        /// An engine code, carried under its own name so a person can search for what the engine said.
        #[must_use]
        pub const fn carried(engine: rust_mutants::error::ErrorCode) -> Self {
            Self {
                code: engine.code,
                summary: engine.summary,
                remedy: match engine.remedy {
                    Some(said) => said,
                    None => {
                        "the engine reported this; `rust-mutants` on the same tree says \
                         the same thing with more of its own context"
                    }
                },
                sealed: Sealed,
            }
        }
    }

    impl NjCode {
        /// The code, what it means, and what to do about it.
        #[must_use]
        #[expect(
            clippy::too_many_lines,
            reason = "one arm per code: the table is the function, and splitting it would split the one match that keeps it total"
        )]
        pub const fn error_code(self) -> ErrorCode {
            match self {
                Self::Interrupted => ErrorCode {
                    code: "NJ0001",
                    summary: "the caller cancelled the operation before it completed",
                    remedy: "nothing was left half-done; run it again when you are ready",
                    sealed: Sealed,
                },
                Self::OutputUnwritable => ErrorCode {
                    code: "NJ0002",
                    summary: "the command's output stream could not be written",
                    remedy: "check the destination is writable and has space; a composition root may treat a deliberately closed pipe as success",
                    sealed: Sealed,
                },
                Self::ConfigUnreadable => ErrorCode {
                    code: "NJ1001",
                    summary: "the configuration file could not be read",
                    remedy: "check the file is readable by this user; the path names it",
                    sealed: Sealed,
                },
                Self::ConfigUnparsable => ErrorCode {
                    code: "NJ1002",
                    summary: "the configuration file is not the document this version understands",
                    remedy: "`njutest init` writes a file this release understands, with every key commented",
                    sealed: Sealed,
                },
                Self::ConfigInvalid => ErrorCode {
                    code: "NJ1003",
                    summary: "the configuration says something a run cannot honour",
                    remedy: "the message says which key and why; `njutest init` writes one that is valid",
                    sealed: Sealed,
                },
                Self::ConfigUnsupportedVersion => ErrorCode {
                    code: "NJ1004",
                    summary: "the configuration names a version this release does not understand",
                    remedy: "this release reads version 1; a newer file needs a newer release",
                    sealed: Sealed,
                },
                Self::ConfigExists => ErrorCode {
                    code: "NJ1005",
                    summary: "a configuration file is already there",
                    remedy: "remove the file first, or edit the one already there: init never writes over a configuration somebody wrote",
                    sealed: Sealed,
                },
                Self::EvidenceUnreadable => ErrorCode {
                    code: "NJ2001",
                    summary: "the tree a run is about could not be read",
                    remedy: "run this inside the tree you mean to verify, or pass --root at it",
                    sealed: Sealed,
                },
                Self::TreeWrittenDuringMeasurement => ErrorCode {
                    code: "NJ2002",
                    summary: "the tree changed while it was being measured, so the measurement would describe files it did not read",
                    remedy: "run again on a tree nothing else is writing: a measurement is kept only of the bytes it read",
                    sealed: Sealed,
                },
                Self::TargetListFailed => ErrorCode {
                    code: "NJ3001",
                    summary: "a test binary could not be asked what tests it holds",
                    remedy: "run `cargo test --no-run` yourself: a binary that will not list its tests is one the build did not finish",
                    sealed: Sealed,
                },
                Self::BuildFailed => ErrorCode {
                    code: "NJ3002",
                    summary: "the build could not be run",
                    remedy: "run the same cargo command yourself; what it says there is what it said here",
                    sealed: Sealed,
                },
                Self::BuildUnreadable => ErrorCode {
                    code: "NJ3003",
                    summary: "the build's output could not be read",
                    remedy: "run `cargo clean` and try again; output from an interrupted build cannot be read",
                    sealed: Sealed,
                },
                Self::CoverageUnreadable => ErrorCode {
                    code: "NJ4001",
                    summary: "a coverage export could not be read",
                    remedy: "run again without coverage to verify without it, or check llvm-tools-preview is installed",
                    sealed: Sealed,
                },
                Self::CoverageToolsMissing => ErrorCode {
                    code: "NJ4002",
                    summary: "the LLVM tools the toolchain ships are not installed",
                    remedy: "`rustup component add llvm-tools-preview`",
                    sealed: Sealed,
                },
                Self::CoverageToolFailed => ErrorCode {
                    code: "NJ4003",
                    summary: "llvm-profdata or llvm-cov failed",
                    remedy: "`rustup component add llvm-tools-preview`, and check the versions match the toolchain in use",
                    sealed: Sealed,
                },
                Self::CoverageNothingWritten => ErrorCode {
                    code: "NJ4004",
                    summary: "a test process wrote no coverage profile at all",
                    remedy: "check nothing in the suite sets LLVM_PROFILE_FILE for itself; a run composes it and an inherited one sends the profile elsewhere",
                    sealed: Sealed,
                },
                Self::ProviderUnstartable => ErrorCode {
                    code: "NJ5001",
                    summary: "a provider could not be started",
                    remedy: "run the provider's command yourself: it could not be started, and the message names it",
                    sealed: Sealed,
                },
                Self::ProviderTimeout => ErrorCode {
                    code: "NJ5002",
                    summary: "a provider said nothing in the time it was given",
                    remedy: "raise the provider's timeout, or check the command it runs answers at all",
                    sealed: Sealed,
                },
                Self::ProviderProtocol => ErrorCode {
                    code: "NJ5003",
                    summary: "a provider said something this version does not understand",
                    remedy: "this is a defect in the provider, not in this tool: what it printed is not the document the contract asks for",
                    sealed: Sealed,
                },
                Self::ProviderRefused => ErrorCode {
                    code: "NJ5004",
                    summary: "a provider said it could not do what it was asked",
                    remedy: "the provider refused and said why; nothing here can answer for it",
                    sealed: Sealed,
                },
                Self::ResourceEnvironmentRefused => ErrorCode {
                    code: "NJ5005",
                    summary: "a provider offered an environment variable a run composes itself",
                    remedy: "a provider may not set a variable a run composes; remove it from what the provider offers",
                    sealed: Sealed,
                },
                Self::GenerationProtocol => ErrorCode {
                    code: "NJ5006",
                    summary: "a generation provider said something this version does not understand",
                    remedy: "this is a defect in the provider, not in this tool: what it printed is not the document the contract asks for",
                    sealed: Sealed,
                },
                Self::GenerationPathRefused => ErrorCode {
                    code: "NJ5007",
                    summary: "a generation provider would write where it may not",
                    remedy: "a generated candidate is stored beside the tree and never written into it; the provider named a path outside what it may write",
                    sealed: Sealed,
                },
                Self::GenerationPreimageMoved => ErrorCode {
                    code: "NJ5008",
                    summary: "the file a candidate patches is not the file the provider saw",
                    remedy: "the file changed after the provider read it; run again on a tree nothing else is writing",
                    sealed: Sealed,
                },
                Self::SentinelBlind => ErrorCode {
                    code: "NJ5009",
                    summary: "a routing layer did not route the mutant planted for it",
                    remedy: "this is a defect in the engine, not in the code under test; no setting skips a sentinel, because a layer that fails one would be deciding which of your mutants never run",
                    sealed: Sealed,
                },
                Self::ReportUnserializable => ErrorCode {
                    code: "NJ6001",
                    summary: "the report could not be written as JSON",
                    remedy: "this is a defect in this tool: a report it built could not be written as JSON",
                    sealed: Sealed,
                },
                Self::ReportUnreadable => ErrorCode {
                    code: "NJ6002",
                    summary: "a document is not the assurance report this version understands",
                    remedy: "the document is from another release or another tool; `njutest verify` writes one this release reads",
                    sealed: Sealed,
                },
                Self::ReportUnsound => ErrorCode {
                    code: "NJ6003",
                    summary: "the report contradicts itself and was not written",
                    remedy: "this is a defect in this tool: it refused to write a report whose parts disagree, rather than store one a reader could not trust",
                    sealed: Sealed,
                },
                Self::ReportNotKept => ErrorCode {
                    code: "NJ6004",
                    summary: "the report could not be written where a reader will look for it",
                    remedy: "check the report directory exists and this user may write in it; the path names the file",
                    sealed: Sealed,
                },
                Self::RunNotFound => ErrorCode {
                    code: "NJ6005",
                    summary: "there is no such run to answer about",
                    remedy: "every run is a directory under `runs/` in the configured reports directory (`reports/runs` by default); `njutest report` with no run reads the newest",
                    sealed: Sealed,
                },
                Self::SubjectNotCataloged => ErrorCode {
                    code: "NJ6006",
                    summary: "the run made no change in anything the subject names",
                    remedy: "`njutest report` lists what the run changed; name a file, `PATH:ITEM`, or an item as the source names it",
                    sealed: Sealed,
                },
                Self::MeasurementUnwritable => ErrorCode {
                    code: "NJ6020",
                    summary: "the measurement a selection reads could not be written",
                    remedy: "check the report directory exists and this user may write in it; the path names the file",
                    sealed: Sealed,
                },
                Self::MeasurementUnreadable => ErrorCode {
                    code: "NJ6021",
                    summary: "there is no measurement to select by, or it is not one this release reads",
                    remedy: "`njutest measure` writes one; a selection with nothing measured to stand on selects nothing",
                    sealed: Sealed,
                },
                Self::MiriMissing => ErrorCode {
                    code: "NJ7001",
                    summary: "the toolchain has no miri, and this contract promises interpretation",
                    remedy: "`rustup +nightly component add miri`, or ask for a contract that does not promise interpretation",
                    sealed: Sealed,
                },
                Self::ModelPhaseFailed => ErrorCode {
                    code: "NJ7002",
                    summary: "the verified model-checking phase could not preserve its own evidence",
                    remedy: "the message names the internal source or artifact boundary that failed; fix its permissions or report the invariant failure",
                    sealed: Sealed,
                },
                Self::SchedulerUnusable => ErrorCode {
                    code: "NJ7003",
                    summary: "the run could not schedule measurements without trusting interrupted state",
                    remedy: "run it again; a worker panic or poisoned coordination lock is never recovered as ordinary state",
                    sealed: Sealed,
                },
                Self::PhaseOutputUnreadable => ErrorCode {
                    code: "NJ7004",
                    summary: "an assurance phase printed output that is not valid UTF-8",
                    remedy: "the named tool violated its text-output contract; fix or replace that tool before trusting its result",
                    sealed: Sealed,
                },
                Self::SourcesUnreadable => ErrorCode {
                    code: "NJ7005",
                    summary: "the run ran out of descriptors or memory while reading the sources a proof rests on",
                    remedy: "raise the open-file limit or free memory and run it again; what could not be opened is not known to be unreadable",
                    sealed: Sealed,
                },
                Self::ScratchUnusable => ErrorCode {
                    code: "NJ8001",
                    summary: "the run has nowhere to work",
                    remedy: "check TMPDIR is a directory this user may write in, and that there is room under it",
                    sealed: Sealed,
                },
                Self::CacheUnusable => ErrorCode {
                    code: "NJ8003",
                    summary: "the store of earlier answers could not be used",
                    remedy: "remove the store and let it be rebuilt: what is in it is read-only evidence and nothing is lost",
                    sealed: Sealed,
                },
                Self::CacheCorrupt => ErrorCode {
                    code: "NJ8004",
                    summary: "a stored answer is not the answer it claims to be",
                    remedy: "remove the store and let it be rebuilt: a stored answer that is not what it claims is never used",
                    sealed: Sealed,
                },
                Self::WireCannotListen => ErrorCode {
                    code: "NJ8005",
                    summary: "no port could be listened on in front of a seam, so nothing could be recorded about it",
                    remedy: "check this machine allows a listener on the loopback interface, and that nothing has taken every port",
                    sealed: Sealed,
                },
                Self::MergeRefused => ErrorCode {
                    code: "NJ9001",
                    summary: "the reports offered are not the parts of one catalog",
                    remedy: "every part of one catalog has the same catalog digest; the parts offered do not, so they are not parts of one run",
                    sealed: Sealed,
                },
            }
        }
    }
}

pub use table::{ErrorCode, NjCode};

const INTERRUPTED: ErrorCode = NjCode::Interrupted.error_code();

const OUTPUT_UNWRITABLE: ErrorCode = NjCode::OutputUnwritable.error_code();

pub(crate) const CONFIG_UNREADABLE: ErrorCode = NjCode::ConfigUnreadable.error_code();
pub(crate) const CONFIG_UNPARSABLE: ErrorCode = NjCode::ConfigUnparsable.error_code();
pub(crate) const CONFIG_INVALID: ErrorCode = NjCode::ConfigInvalid.error_code();
pub(crate) const CONFIG_UNSUPPORTED_VERSION: ErrorCode =
    NjCode::ConfigUnsupportedVersion.error_code();
pub(crate) const CONFIG_EXISTS: ErrorCode = NjCode::ConfigExists.error_code();
pub(crate) const BUILD_FAILED: ErrorCode = NjCode::BuildFailed.error_code();
pub(crate) const BUILD_UNREADABLE: ErrorCode = NjCode::BuildUnreadable.error_code();
pub(crate) const TARGET_LIST_FAILED: ErrorCode = NjCode::TargetListFailed.error_code();
pub(crate) const COVERAGE_UNREADABLE: ErrorCode = NjCode::CoverageUnreadable.error_code();
pub(crate) const COVERAGE_TOOLS_MISSING: ErrorCode = NjCode::CoverageToolsMissing.error_code();
pub(crate) const COVERAGE_TOOL_FAILED: ErrorCode = NjCode::CoverageToolFailed.error_code();
pub(crate) const COVERAGE_NOTHING_WRITTEN: ErrorCode = NjCode::CoverageNothingWritten.error_code();
pub(crate) const REPORT_UNSERIALIZABLE: ErrorCode = NjCode::ReportUnserializable.error_code();
pub(crate) const REPORT_UNREADABLE: ErrorCode = NjCode::ReportUnreadable.error_code();
pub(crate) const RUN_NOT_FOUND: ErrorCode = NjCode::RunNotFound.error_code();
pub(crate) const SUBJECT_NOT_CATALOGED: ErrorCode = NjCode::SubjectNotCataloged.error_code();
pub(crate) const REPORT_NOT_KEPT: ErrorCode = NjCode::ReportNotKept.error_code();
pub(crate) const EVIDENCE_UNREADABLE: ErrorCode = NjCode::EvidenceUnreadable.error_code();
pub(crate) const TREE_WRITTEN_DURING_MEASUREMENT: ErrorCode =
    NjCode::TreeWrittenDuringMeasurement.error_code();
pub(crate) const MEASUREMENT_UNWRITABLE: ErrorCode = NjCode::MeasurementUnwritable.error_code();
pub(crate) const MEASUREMENT_UNREADABLE: ErrorCode = NjCode::MeasurementUnreadable.error_code();
pub(crate) const CACHE_UNUSABLE: ErrorCode = NjCode::CacheUnusable.error_code();
pub(crate) const CACHE_CORRUPT: ErrorCode = NjCode::CacheCorrupt.error_code();
pub(crate) const WIRE_CANNOT_LISTEN: ErrorCode = NjCode::WireCannotListen.error_code();
pub(crate) const SCRATCH_UNUSABLE: ErrorCode = NjCode::ScratchUnusable.error_code();
pub(crate) const MERGE_REFUSED: ErrorCode = NjCode::MergeRefused.error_code();
pub(crate) const PROVIDER_UNSTARTABLE: ErrorCode = NjCode::ProviderUnstartable.error_code();
pub(crate) const PROVIDER_TIMEOUT: ErrorCode = NjCode::ProviderTimeout.error_code();
pub(crate) const PROVIDER_PROTOCOL: ErrorCode = NjCode::ProviderProtocol.error_code();
pub(crate) const PROVIDER_REFUSED: ErrorCode = NjCode::ProviderRefused.error_code();
pub(crate) const RESOURCE_ENVIRONMENT_REFUSED: ErrorCode =
    NjCode::ResourceEnvironmentRefused.error_code();
pub(crate) const MIRI_MISSING: ErrorCode = NjCode::MiriMissing.error_code();
pub(crate) const MODEL_PHASE_FAILED: ErrorCode = NjCode::ModelPhaseFailed.error_code();
pub(crate) const SCHEDULER_UNUSABLE: ErrorCode = NjCode::SchedulerUnusable.error_code();
pub(crate) const PHASE_OUTPUT_UNREADABLE: ErrorCode = NjCode::PhaseOutputUnreadable.error_code();
pub(crate) const SOURCES_UNREADABLE: ErrorCode = NjCode::SourcesUnreadable.error_code();
pub(crate) const GENERATION_PROTOCOL: ErrorCode = NjCode::GenerationProtocol.error_code();
pub(crate) const GENERATION_PATH_REFUSED: ErrorCode = NjCode::GenerationPathRefused.error_code();
pub(crate) const GENERATION_PREIMAGE_MOVED: ErrorCode =
    NjCode::GenerationPreimageMoved.error_code();
pub(crate) const SENTINEL_BLIND: ErrorCode = NjCode::SentinelBlind.error_code();
pub(crate) const REPORT_UNSOUND: ErrorCode = NjCode::ReportUnsound.error_code();

/// Every failure the runner reports.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunnerError {
    /// The caller cancelled the operation before it completed.
    #[error("the operation was interrupted before it completed")]
    Interrupted,
    /// The command's output stream stopped accepting its result.
    #[error("{}: cannot write command output: {source}", OUTPUT_UNWRITABLE.code)]
    Output {
        /// The write or flush failure.
        #[from]
        source: std::io::Error,
    },
    /// The configuration could not be used.
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    /// A unit's tests could not be named.
    #[error(transparent)]
    Target(#[from] crate::targets::TargetError),
    /// The tree a run is about could not be read.
    #[error(transparent)]
    Evidence(#[from] crate::evidence::tree::ScanError),
    /// The store of earlier answers could not be used.
    #[error(transparent)]
    Cache(#[from] crate::cache::store::CacheError),
    /// Scheduling state from an interrupted run could not be preserved or trusted.
    #[error(transparent)]
    Checkpoint(#[from] crate::checkpoint::CheckpointError),
    /// Per-mutant evidence could not be preserved.
    #[error(transparent)]
    MutationEvidence(#[from] crate::evidence::store::StoreError),
    /// Coverage could not be read.
    #[error(transparent)]
    Coverage(#[from] crate::coverage::CoverageError),
    /// A provider could not be used.
    #[error(transparent)]
    Provider(#[from] crate::provider::ProviderError),
    /// A tree could not be measured for a selection.
    #[error(transparent)]
    Measure(#[from] crate::assure::measure::MeasureError),
    /// A measurement could not be kept or read back.
    #[error(transparent)]
    Reach(#[from] crate::reach::ReachError),
    /// A selected environment entry cannot be represented in the run identity.
    #[error("{}: {source}", CONFIG_INVALID.code)]
    IdentityEnvironment {
        /// The exact environment encoding refusal.
        #[source]
        source: crate::assure::identity::EnvironmentError,
    },
    /// A mutation's source bytes cannot be represented as report text.
    #[error("{}: {source}", REPORT_UNSOUND.code)]
    MutationText {
        /// Which source half violated the UTF-8 invariant.
        #[from]
        source: crate::assure::mutation::MutationTextError,
    },
    /// The toolchain has no Miri, and the contract promises interpretation.
    #[error("{}: {message}", MIRI_MISSING.code)]
    MiriMissing {
        /// What the toolchain said.
        message: String,
    },
    /// An assurance phase printed bytes that its text protocol cannot represent.
    #[error("{}: {phase} output is not valid UTF-8: {source}", PHASE_OUTPUT_UNREADABLE.code)]
    PhaseOutput {
        /// The phase whose process printed the bytes.
        phase: &'static str,
        /// Why the bytes cannot be decoded exactly.
        #[source]
        source: std::str::Utf8Error,
    },
    /// The model phase could not preserve the source or artifact it must audit.
    #[error("{}: {message}", MODEL_PHASE_FAILED.code)]
    Model {
        /// The typed internal failure, rendered only at this outer error boundary.
        message: String,
    },
    /// Measurements could not be scheduled without trusting state interrupted by a panic.
    #[error(transparent)]
    Schedule(#[from] crate::assure::schedule::ScheduleError),
    /// The sources a proof rests on could not be read just now.
    #[error(transparent)]
    Sources(#[from] crate::observe::SourceReadError),
    /// Equivalence answers could not be correlated without ambiguity.
    #[error("{}: {source}", REPORT_UNSOUND.code)]
    Equivalence {
        /// The exact ledger inconsistency.
        #[from]
        source: crate::assure::equivalence::EquivalenceError,
    },
    /// A resource could not be leased.
    #[error(transparent)]
    Resource(#[from] crate::resource::ResourceError),
    /// A report could not be written or read.
    #[error(transparent)]
    Report(#[from] crate::report::json::ReportError),
    /// A derived report projection exceeded the v1 exact counter range.
    #[error("{}: {source}", REPORT_UNSOUND.code)]
    ReportCount {
        /// The exact counter relation that could not be represented.
        #[from]
        source: crate::report::CountError,
    },
    /// A measured run fact could not be represented exactly.
    #[error("{}: {source}", REPORT_UNSOUND.code)]
    RunInvariant {
        /// The exact failed invariant.
        #[from]
        source: crate::assure::run::RunInvariantError,
    },
    /// An observed seam could not be given the exact v1 fault identity.
    #[error("{}: {source}", REPORT_UNSOUND.code)]
    WireIdentity {
        /// The closed identity-recipe failure.
        #[from]
        source: crate::wire::derive::DeriveError,
    },
    /// The run has nowhere to work.
    #[error(transparent)]
    Scratch(#[from] crate::scratch::ScratchError),
    /// The workspace could not be built.
    #[error(transparent)]
    Build(#[from] crate::build::BuildError),
    /// A routing layer did not route the mutant planted for it, so nothing it removes from this run is believed.
    #[error(
        "{}: the {layer} layer did not route the mutant planted for it: {mutant} was to be \
         {expected}, and the engine routed it `{routed}`. Nothing the {layer} layer would \
         remove from this run is believed, so the run stops before its baseline",
        SENTINEL_BLIND.code
    )]
    Blind {
        /// The layer the mutant was planted for.
        layer: rust_mutants::sentinel::Planted,
        /// The planted mutant, by its locator.
        mutant: String,
        /// How the layer must have routed it, as a reader is told.
        expected: String,
        /// How the engine routed it.
        routed: String,
    },
    /// The engine refused.
    /// Its codes are `RM`-prefixed and live in the engine's half of `docs/errors.md`; a runner that renamed them would make a user's report unsearchable.
    #[error(transparent)]
    Engine(#[from] rust_mutants::EngineError),
}

impl RunnerError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
            Self::Output { .. } => OUTPUT_UNWRITABLE,
            Self::Config(error) => error.code(),
            Self::Target(error) => error.code(),
            Self::Evidence(error) => error.code(),
            Self::Cache(error) => error.code(),
            Self::Checkpoint(error) => error.code(),
            Self::MutationEvidence(error) => error.code(),
            Self::Coverage(error) => error.code(),
            Self::Provider(error) => error.code(),
            Self::IdentityEnvironment { .. } => CONFIG_INVALID,
            Self::MutationText { .. }
            | Self::Equivalence { .. }
            | Self::ReportCount { .. }
            | Self::RunInvariant { .. }
            | Self::WireIdentity { .. } => REPORT_UNSOUND,
            Self::MiriMissing { .. } => MIRI_MISSING,
            Self::PhaseOutput { .. } => PHASE_OUTPUT_UNREADABLE,
            Self::Model { .. } => MODEL_PHASE_FAILED,
            Self::Schedule(_) => SCHEDULER_UNUSABLE,
            Self::Sources(error) => error.code(),
            Self::Blind { .. } => SENTINEL_BLIND,
            Self::Resource(error) => error.code(),
            Self::Report(error) => error.code(),
            Self::Scratch(error) => error.code(),
            Self::Build(error) => error.code(),
            Self::Measure(error) => error.code(),
            Self::Reach(error) => error.code(),
            Self::Engine(error) => ErrorCode::carried(error.code()),
        }
    }
}

/// Every code the runner can report, in code order.
#[must_use]
#[cfg(feature = "testkit")]
pub const fn error_codes() -> &'static [ErrorCode] {
    &ERROR_CODES
}

/// Every code, made once from [`NjCode::ALL`].
#[cfg(feature = "testkit")]
#[expect(
    clippy::indexing_slicing,
    reason = "a const loop cannot iterate, and the bound is the length of the array it indexes"
)]
const ERROR_CODES: [ErrorCode; NjCode::ALL.len()] = {
    let mut codes = [NjCode::ALL[0].error_code(); NjCode::ALL.len()];
    let mut at = 0;
    while at < codes.len() {
        codes[at] = NjCode::ALL[at].error_code();
        at += 1;
    }
    codes
};

impl From<crate::assure::model::ModelError> for RunnerError {
    fn from(source: crate::assure::model::ModelError) -> Self {
        Self::Model {
            message: source.to_string(),
        }
    }
}

impl From<rust_mutants::coverage::CoverageError> for RunnerError {
    fn from(source: rust_mutants::coverage::CoverageError) -> Self {
        Self::Coverage(source.into())
    }
}
