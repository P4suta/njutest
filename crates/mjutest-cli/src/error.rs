// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failure modes of the runner, each with a stable code documented in
//! `docs/errors.md`.

/// A stable, searchable identifier for one failure mode.
///
/// Codes are `MJ` followed by four digits. The first digit names the area:
/// `0` the command line and its contract, `1` configuration, `2` repository
/// and evidence identity, `3` targets and baseline, `4` coverage,
/// `5` mutation, `6` reports and stores, `7` providers, `8` caches and
/// temporary directories, `9` internal invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ErrorCode {
    /// The code, e.g. `MJ0001`.
    pub code: &'static str,
    /// One line saying what the code means.
    pub summary: &'static str,
}

const INTERRUPTED: ErrorCode = ErrorCode {
    code: "MJ0001",
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
    "MJ1001",
    "the configuration file could not be read"
);
code!(
    CONFIG_UNPARSABLE,
    "MJ1002",
    "the configuration file is not the document this version understands"
);
code!(
    CONFIG_INVALID,
    "MJ1003",
    "the configuration says something a run cannot honour"
);
code!(
    CONFIG_UNSUPPORTED_VERSION,
    "MJ1004",
    "the configuration names a version this release does not understand"
);
code!(
    TARGET_LIST_FAILED,
    "MJ3001",
    "a test binary could not be asked what tests it holds"
);
code!(
    COVERAGE_UNREADABLE,
    "MJ4001",
    "a coverage export could not be read"
);
code!(
    COVERAGE_TOOLS_MISSING,
    "MJ4002",
    "the LLVM tools the toolchain ships are not installed"
);
code!(
    COVERAGE_TOOL_FAILED,
    "MJ4003",
    "llvm-profdata or llvm-cov failed"
);
code!(
    COVERAGE_NOTHING_WRITTEN,
    "MJ4004",
    "a test process wrote no coverage profile at all"
);
code!(
    REPORT_UNSERIALIZABLE,
    "MJ6001",
    "the report could not be written as JSON"
);
code!(
    REPORT_UNREADABLE,
    "MJ6002",
    "a document is not the assurance report this version understands"
);
code!(SCRATCH_UNUSABLE, "MJ8001", "the run has nowhere to work");
code!(
    REPORT_UNSOUND,
    "MJ6003",
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
    /// Coverage could not be read.
    #[error(transparent)]
    Coverage(#[from] crate::coverage::CoverageError),
    /// A report could not be written or read.
    #[error(transparent)]
    Report(#[from] crate::report::json::ReportError),
    /// The run has nowhere to work.
    #[error(transparent)]
    Scratch(#[from] crate::scratch::ScratchError),
}

impl RunnerError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
            Self::Config(error) => error.code(),
            Self::Target(error) => error.code(),
            Self::Coverage(error) => error.code(),
            Self::Report(error) => error.code(),
            Self::Scratch(error) => error.code(),
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
        TARGET_LIST_FAILED,
        COVERAGE_UNREADABLE,
        COVERAGE_TOOLS_MISSING,
        COVERAGE_TOOL_FAILED,
        COVERAGE_NOTHING_WRITTEN,
        REPORT_UNSERIALIZABLE,
        REPORT_UNREADABLE,
        REPORT_UNSOUND,
        SCRATCH_UNUSABLE,
    ]
}
