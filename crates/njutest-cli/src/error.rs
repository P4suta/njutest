// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failure modes of the runner, each with a stable code documented in `docs/errors.md`.

/// A stable, searchable identifier for one failure mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ErrorCode {
    /// The code, e.g. `NJ0001`.
    pub code: &'static str,
    /// One line saying what the code means.
    pub summary: &'static str,
}

const INTERRUPTED: ErrorCode = ErrorCode {
    code: "NJ0001",
    summary: "the caller cancelled the operation before it completed",
};

macro_rules! code {
    ($name:ident, $code:literal, $summary:literal) => {
        pub(crate) const $name: ErrorCode = ErrorCode {
            code: $code,
            summary: $summary,
        };
    };
}

code!(
    CONFIG_UNREADABLE,
    "NJ1001",
    "the configuration file could not be read"
);
code!(
    CONFIG_UNPARSABLE,
    "NJ1002",
    "the configuration file is not the document this version understands"
);
code!(
    CONFIG_INVALID,
    "NJ1003",
    "the configuration says something a run cannot honour"
);
code!(
    CONFIG_UNSUPPORTED_VERSION,
    "NJ1004",
    "the configuration names a version this release does not understand"
);
code!(
    CONFIG_EXISTS,
    "NJ1005",
    "a configuration file is already there"
);
code!(BUILD_FAILED, "NJ3002", "the build could not be run");
code!(
    BUILD_UNREADABLE,
    "NJ3003",
    "the build's output could not be read"
);
code!(
    TARGET_LIST_FAILED,
    "NJ3001",
    "a test binary could not be asked what tests it holds"
);
code!(
    COVERAGE_UNREADABLE,
    "NJ4001",
    "a coverage export could not be read"
);
code!(
    COVERAGE_TOOLS_MISSING,
    "NJ4002",
    "the LLVM tools the toolchain ships are not installed"
);
code!(
    COVERAGE_TOOL_FAILED,
    "NJ4003",
    "llvm-profdata or llvm-cov failed"
);
code!(
    COVERAGE_NOTHING_WRITTEN,
    "NJ4004",
    "a test process wrote no coverage profile at all"
);
code!(
    REPORT_UNSERIALIZABLE,
    "NJ6001",
    "the report could not be written as JSON"
);
code!(
    REPORT_UNREADABLE,
    "NJ6002",
    "a document is not the assurance report this version understands"
);
code!(
    RUN_NOT_FOUND,
    "NJ6005",
    "there is no such run to answer about"
);
code!(
    REPORT_NOT_KEPT,
    "NJ6004",
    "the report could not be written where a reader will look for it"
);
code!(
    EVIDENCE_UNREADABLE,
    "NJ2001",
    "the tree a run is about could not be read"
);
code!(
    CACHE_UNUSABLE,
    "NJ8003",
    "the store of earlier answers could not be used"
);
code!(
    CACHE_CORRUPT,
    "NJ8004",
    "a stored answer is not the answer it claims to be"
);
code!(SCRATCH_UNUSABLE, "NJ8001", "the run has nowhere to work");
code!(
    MERGE_REFUSED,
    "NJ9001",
    "the reports offered are not the parts of one catalog"
);
code!(
    PROVIDER_UNSTARTABLE,
    "NJ5001",
    "a provider could not be started"
);
code!(
    PROVIDER_TIMEOUT,
    "NJ5002",
    "a provider said nothing in the time it was given"
);
code!(
    PROVIDER_PROTOCOL,
    "NJ5003",
    "a provider said something this version does not understand"
);
code!(
    PROVIDER_REFUSED,
    "NJ5004",
    "a provider said it could not do what it was asked"
);
code!(
    RESOURCE_ENVIRONMENT_REFUSED,
    "NJ5005",
    "a provider offered an environment variable a run composes itself"
);
code!(
    MIRI_MISSING,
    "NJ7001",
    "the toolchain has no miri, and this contract promises interpretation"
);
code!(
    GENERATION_PROTOCOL,
    "NJ5006",
    "a generation provider said something this version does not understand"
);
code!(
    GENERATION_PATH_REFUSED,
    "NJ5007",
    "a generation provider would write where it may not"
);
code!(
    GENERATION_PREIMAGE_MOVED,
    "NJ5008",
    "the file a candidate patches is not the file the provider saw"
);
code!(
    REPORT_UNSOUND,
    "NJ6003",
    "the report contradicts itself and was not written"
);

/// Every failure the runner reports.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunnerError {
    /// The caller cancelled the operation before it completed.
    #[error("the operation was interrupted before it completed")]
    Interrupted,
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
    /// Coverage could not be read.
    #[error(transparent)]
    Coverage(#[from] crate::coverage::CoverageError),
    /// A provider could not be used.
    #[error(transparent)]
    Provider(#[from] crate::provider::ProviderError),
    /// The toolchain has no Miri, and the contract promises interpretation.
    #[error("{}: {message}", MIRI_MISSING.code)]
    MiriMissing {
        /// What the toolchain said.
        message: String,
    },
    /// A resource could not be leased.
    #[error(transparent)]
    Resource(#[from] crate::resource::ResourceError),
    /// A report could not be written or read.
    #[error(transparent)]
    Report(#[from] crate::report::json::ReportError),
    /// The run has nowhere to work.
    #[error(transparent)]
    Scratch(#[from] crate::scratch::ScratchError),
    /// The workspace could not be built.
    #[error(transparent)]
    Build(#[from] crate::build::BuildError),
    /// The engine refused. Its codes are `RM`-prefixed and live in the engine's half of `docs/errors.md`; a runner that renamed them would make a user's report unsearchable.
    #[error(transparent)]
    Engine(#[from] rust_mutants::EngineError),
}

impl RunnerError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
            Self::Config(error) => error.code(),
            Self::Target(error) => error.code(),
            Self::Evidence(error) => error.code(),
            Self::Cache(error) => error.code(),
            Self::Coverage(error) => error.code(),
            Self::Provider(error) => error.code(),
            Self::MiriMissing { .. } => MIRI_MISSING,
            Self::Resource(error) => error.code(),
            Self::Report(error) => error.code(),
            Self::Scratch(error) => error.code(),
            Self::Build(error) => error.code(),
            Self::Engine(error) => {
                let engine = error.code();
                ErrorCode {
                    code: engine.code,
                    summary: engine.summary,
                }
            }
        }
    }
}

/// Every code the runner can report, in code order.
#[must_use]
pub const fn error_codes() -> &'static [ErrorCode] {
    &[
        INTERRUPTED,
        CONFIG_UNREADABLE,
        CONFIG_UNPARSABLE,
        CONFIG_INVALID,
        CONFIG_UNSUPPORTED_VERSION,
        CONFIG_EXISTS,
        EVIDENCE_UNREADABLE,
        TARGET_LIST_FAILED,
        BUILD_FAILED,
        BUILD_UNREADABLE,
        COVERAGE_UNREADABLE,
        COVERAGE_TOOLS_MISSING,
        COVERAGE_TOOL_FAILED,
        COVERAGE_NOTHING_WRITTEN,
        PROVIDER_UNSTARTABLE,
        PROVIDER_TIMEOUT,
        PROVIDER_PROTOCOL,
        PROVIDER_REFUSED,
        RESOURCE_ENVIRONMENT_REFUSED,
        GENERATION_PROTOCOL,
        GENERATION_PATH_REFUSED,
        GENERATION_PREIMAGE_MOVED,
        REPORT_UNSERIALIZABLE,
        REPORT_UNREADABLE,
        REPORT_UNSOUND,
        REPORT_NOT_KEPT,
        RUN_NOT_FOUND,
        MIRI_MISSING,
        SCRATCH_UNUSABLE,
        CACHE_UNUSABLE,
        CACHE_CORRUPT,
        MERGE_REFUSED,
    ]
}

impl From<rust_mutants::coverage::CoverageError> for RunnerError {
    fn from(source: rust_mutants::coverage::CoverageError) -> Self {
        Self::Coverage(source.into())
    }
}
