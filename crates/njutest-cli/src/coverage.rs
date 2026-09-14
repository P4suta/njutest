// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which regions of which files one test reached, in this product's error ledger.
//!
//! The reading itself is the engine's, so both products measure coverage the
//! same way. Only the codes differ: a `njutest` report answers in `NJ`.

use crate::error::{self, ErrorCode};

pub use rust_mutants::coverage::{
    Block, FileRegions, INSTRUMENT_FLAG, PROFILE_ENV, Point, REGION_KIND_CODE, Region, Tools,
    covered, instrumented, parse_export, profile_pattern, written_profiles,
};

/// The failure modes of coverage, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverageErrorKind {
    /// A coverage export could not be read.
    Unreadable,
    /// The LLVM tools the toolchain ships are not installed.
    ToolsMissing,
    /// One of them failed.
    ToolFailed,
    /// A test process wrote no profile at all.
    NothingWritten,
}

impl CoverageErrorKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 4] = [
        Self::Unreadable,
        Self::ToolsMissing,
        Self::ToolFailed,
        Self::NothingWritten,
    ];

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::Unreadable => error::COVERAGE_UNREADABLE,
            Self::ToolsMissing => error::COVERAGE_TOOLS_MISSING,
            Self::ToolFailed => error::COVERAGE_TOOL_FAILED,
            Self::NothingWritten => error::COVERAGE_NOTHING_WRITTEN,
        }
    }
}

impl From<rust_mutants::coverage::CoverageErrorKind> for CoverageErrorKind {
    fn from(kind: rust_mutants::coverage::CoverageErrorKind) -> Self {
        match kind {
            rust_mutants::coverage::CoverageErrorKind::Unreadable => Self::Unreadable,
            rust_mutants::coverage::CoverageErrorKind::ToolsMissing => Self::ToolsMissing,
            rust_mutants::coverage::CoverageErrorKind::ToolFailed => Self::ToolFailed,
            rust_mutants::coverage::CoverageErrorKind::NothingWritten => Self::NothingWritten,
        }
    }
}

/// Why coverage could not be read.
#[derive(Debug, thiserror::Error)]
#[error("{}: {}", kind.code().code, source.message())]
pub struct CoverageError {
    kind: CoverageErrorKind,
    #[source]
    source: rust_mutants::coverage::CoverageError,
}

impl CoverageError {
    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> CoverageErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }
}

impl From<rust_mutants::coverage::CoverageError> for CoverageError {
    fn from(source: rust_mutants::coverage::CoverageError) -> Self {
        Self {
            kind: source.kind().into(),
            source,
        }
    }
}
