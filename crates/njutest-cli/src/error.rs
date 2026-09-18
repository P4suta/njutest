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
    /// What to do about it. Every code carries one.
    pub remedy: &'static str,
}

const INTERRUPTED: ErrorCode = ErrorCode {
    code: "NJ0001",
    summary: "the caller cancelled the operation before it completed",
    remedy: "nothing was left half-done; run it again when you are ready",
};

/// Declares one error code. There is no form without a remedy, on purpose.
macro_rules! code {
    ($name:ident, $code:literal, $summary:literal, $remedy:literal) => {
        pub(crate) const $name: ErrorCode = ErrorCode {
            code: $code,
            summary: $summary,
            remedy: $remedy,
        };
    };
}

code!(
    CONFIG_UNREADABLE,
    "NJ1001",
    "the configuration file could not be read",
    "check the file is readable by this user; the path names it"
);
code!(
    CONFIG_UNPARSABLE,
    "NJ1002",
    "the configuration file is not the document this version understands",
    "`njutest init` writes a file this release understands, with every key commented"
);
code!(
    CONFIG_INVALID,
    "NJ1003",
    "the configuration says something a run cannot honour",
    "the message says which key and why; `njutest init` writes one that is valid"
);
code!(
    CONFIG_UNSUPPORTED_VERSION,
    "NJ1004",
    "the configuration names a version this release does not understand",
    "this release reads version 1; a newer file needs a newer release"
);
code!(
    CONFIG_EXISTS,
    "NJ1005",
    "a configuration file is already there",
    "remove the file first, or edit the one already there: init never writes over a configuration somebody wrote"
);
code!(
    BUILD_FAILED,
    "NJ3002",
    "the build could not be run",
    "run the same cargo command yourself; what it says there is what it said here"
);
code!(
    BUILD_UNREADABLE,
    "NJ3003",
    "the build's output could not be read",
    "run `cargo clean` and try again; output from an interrupted build cannot be read"
);
code!(
    TARGET_LIST_FAILED,
    "NJ3001",
    "a test binary could not be asked what tests it holds",
    "run `cargo test --no-run` yourself: a binary that will not list its tests is one the build did not finish"
);
code!(
    COVERAGE_UNREADABLE,
    "NJ4001",
    "a coverage export could not be read",
    "run again without coverage to verify without it, or check llvm-tools-preview is installed"
);
code!(
    COVERAGE_TOOLS_MISSING,
    "NJ4002",
    "the LLVM tools the toolchain ships are not installed",
    "`rustup component add llvm-tools-preview`"
);
code!(
    COVERAGE_TOOL_FAILED,
    "NJ4003",
    "llvm-profdata or llvm-cov failed",
    "`rustup component add llvm-tools-preview`, and check the versions match the toolchain in use"
);
code!(
    COVERAGE_NOTHING_WRITTEN,
    "NJ4004",
    "a test process wrote no coverage profile at all",
    "check nothing in the suite sets LLVM_PROFILE_FILE for itself; a run composes it and an inherited one sends the profile elsewhere"
);
code!(
    REPORT_UNSERIALIZABLE,
    "NJ6001",
    "the report could not be written as JSON",
    "this is a defect in this tool: a report it built could not be written as JSON"
);
code!(
    REPORT_UNREADABLE,
    "NJ6002",
    "a document is not the assurance report this version understands",
    "the document is from another release or another tool; `njutest verify` writes one this release reads"
);
code!(
    RUN_NOT_FOUND,
    "NJ6005",
    "there is no such run to answer about",
    "`njutest report --list` names the runs that are stored under this root"
);
code!(
    REPORT_NOT_KEPT,
    "NJ6004",
    "the report could not be written where a reader will look for it",
    "check the report directory exists and this user may write in it; the path names the file"
);
code!(
    EVIDENCE_UNREADABLE,
    "NJ2001",
    "the tree a run is about could not be read",
    "run this inside the tree you mean to verify, or pass --root at it"
);
code!(
    CACHE_UNUSABLE,
    "NJ8003",
    "the store of earlier answers could not be used",
    "remove the store and let it be rebuilt: what is in it is read-only evidence and nothing is lost"
);
code!(
    CACHE_CORRUPT,
    "NJ8004",
    "a stored answer is not the answer it claims to be",
    "remove the store and let it be rebuilt: a stored answer that is not what it claims is never used"
);
code!(
    WIRE_CANNOT_LISTEN,
    "NJ8005",
    "no port could be listened on in front of a seam, so nothing could be recorded about it",
    "check this machine allows a listener on the loopback interface, and that nothing has taken every port"
);
code!(
    SCRATCH_UNUSABLE,
    "NJ8001",
    "the run has nowhere to work",
    "check TMPDIR is a directory this user may write in, and that there is room under it"
);
code!(
    MERGE_REFUSED,
    "NJ9001",
    "the reports offered are not the parts of one catalog",
    "every part of one catalog has the same catalog digest; the parts offered do not, so they are not parts of one run"
);
code!(
    PROVIDER_UNSTARTABLE,
    "NJ5001",
    "a provider could not be started",
    "run the provider's command yourself: it could not be started, and the message names it"
);
code!(
    PROVIDER_TIMEOUT,
    "NJ5002",
    "a provider said nothing in the time it was given",
    "raise the provider's timeout, or check the command it runs answers at all"
);
code!(
    PROVIDER_PROTOCOL,
    "NJ5003",
    "a provider said something this version does not understand",
    "this is a defect in the provider, not in this tool: what it printed is not the document the contract asks for"
);
code!(
    PROVIDER_REFUSED,
    "NJ5004",
    "a provider said it could not do what it was asked",
    "the provider refused and said why; nothing here can answer for it"
);
code!(
    RESOURCE_ENVIRONMENT_REFUSED,
    "NJ5005",
    "a provider offered an environment variable a run composes itself",
    "a provider may not set a variable a run composes; remove it from what the provider offers"
);
code!(
    MIRI_MISSING,
    "NJ7001",
    "the toolchain has no miri, and this contract promises interpretation",
    "`rustup +nightly component add miri`, or ask for a contract that does not promise interpretation"
);
code!(
    GENERATION_PROTOCOL,
    "NJ5006",
    "a generation provider said something this version does not understand",
    "this is a defect in the provider, not in this tool: what it printed is not the document the contract asks for"
);
code!(
    GENERATION_PATH_REFUSED,
    "NJ5007",
    "a generation provider would write where it may not",
    "a generated candidate is stored beside the tree and never written into it; the provider named a path outside what it may write"
);
code!(
    GENERATION_PREIMAGE_MOVED,
    "NJ5008",
    "the file a candidate patches is not the file the provider saw",
    "the file changed after the provider read it; run again on a tree nothing else is writing"
);
code!(
    REPORT_UNSOUND,
    "NJ6003",
    "the report contradicts itself and was not written",
    "this is a defect in this tool: it refused to write a report whose parts disagree, rather than store one a reader could not trust"
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
                    remedy: match engine.remedy {
                        Some(said) => said,
                        None => {
                            "the engine reported this; `rust-mutants` on the same tree says \
                             the same thing with more of its own context"
                        }
                    },
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
        WIRE_CANNOT_LISTEN,
        MERGE_REFUSED,
    ]
}

impl From<rust_mutants::coverage::CoverageError> for RunnerError {
    fn from(source: rust_mutants::coverage::CoverageError) -> Self {
        Self::Coverage(source.into())
    }
}
