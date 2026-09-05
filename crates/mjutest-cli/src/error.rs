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

/// Every failure the runner reports.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunnerError {
    /// The caller cancelled the operation before it completed.
    #[error("the operation was interrupted before it completed")]
    Interrupted,
}

impl RunnerError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
        }
    }
}

/// Every code the runner can report, in code order.
#[must_use]
pub const fn error_codes() -> &'static [ErrorCode] {
    &[INTERRUPTED]
}
