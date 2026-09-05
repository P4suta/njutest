// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failure modes of the engine, each with a stable code documented in
//! `docs/errors.md`.
//!
//! A code is the searchable name of a failure: what a person greps the
//! documentation and the issue tracker for. The test `errors_doc` keeps the
//! table in `docs/errors.md` and [`error_codes`] equal in both directions.

/// A stable, searchable identifier for one failure mode.
///
/// Codes are `RM` followed by four digits. The first digit names the area:
/// `0` the engine's own contract, `1` workspace and snapshot, `2` discovery,
/// `3` instrumentation, `4` validation, `5` execution, `6` probing,
/// `7` process supervision, `9` internal invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ErrorCode {
    /// The code, e.g. `RM0001`.
    pub code: &'static str,
    /// One line saying what the code means.
    pub summary: &'static str,
}

const INTERRUPTED: ErrorCode = ErrorCode {
    code: "RM0001",
    summary: "the caller cancelled the operation before it completed",
};

/// Every failure the engine reports.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The caller cancelled the operation before it completed.
    #[error("the operation was interrupted before it completed")]
    Interrupted,
}

impl EngineError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
        }
    }
}

/// Every code the engine can report, in code order.
#[must_use]
pub const fn error_codes() -> &'static [ErrorCode] {
    &[INTERRUPTED]
}
