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

macro_rules! snapshot_code {
    ($name:ident, $code:literal, $summary:literal) => {
        pub(crate) const $name: ErrorCode = ErrorCode {
            code: $code,
            summary: $summary,
        };
    };
}

snapshot_code!(
    SNAPSHOT_INVALID_OPTIONS,
    "RM1001",
    "snapshot options that cannot be honoured, such as an escaping report directory"
);
snapshot_code!(
    SNAPSHOT_SOURCE_ROOT,
    "RM1002",
    "a source root that is relative, cannot be read, or is not a directory"
);
snapshot_code!(
    SNAPSHOT_WALK,
    "RM1003",
    "an operating system failure while reading a tree"
);
snapshot_code!(
    SNAPSHOT_SYMLINK,
    "RM1004",
    "a symbolic link inside the source tree, which is refused rather than followed or skipped"
);
snapshot_code!(
    SNAPSHOT_REPARSE_POINT,
    "RM1005",
    "a Windows reparse point (junction or mount point) inside the source tree"
);
snapshot_code!(
    SNAPSHOT_IRREGULAR,
    "RM1006",
    "a file that is neither a directory nor a regular file: a device, a socket, a named pipe"
);
snapshot_code!(
    SNAPSHOT_UNSUPPORTED_NAME,
    "RM1007",
    "a file name that cannot round-trip through a slash-separated relative path"
);
snapshot_code!(
    SNAPSHOT_DESTINATION,
    "RM1008",
    "the snapshot directory could not be created or claimed"
);
snapshot_code!(
    SNAPSHOT_COPY,
    "RM1009",
    "a failure while copying the tree into the snapshot"
);
snapshot_code!(
    SNAPSHOT_CLEANUP_REFUSED,
    "RM1010",
    "a cleanup refused because the recorded directory does not look like a snapshot directory"
);
snapshot_code!(
    SNAPSHOT_CLEANUP_FAILED,
    "RM1011",
    "a snapshot directory that survived every removal attempt"
);

/// Every failure the engine reports.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The caller cancelled the operation before it completed.
    #[error("the operation was interrupted before it completed")]
    Interrupted,
    /// The source tree could not be copied into a disposable snapshot, or
    /// the snapshot could not be removed.
    #[error(transparent)]
    Snapshot(#[from] crate::snapshot::SnapshotError),
}

impl EngineError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
            Self::Snapshot(error) => error.code(),
        }
    }
}

/// Every code the engine can report, in code order.
#[must_use]
pub const fn error_codes() -> &'static [ErrorCode] {
    &[
        INTERRUPTED,
        SNAPSHOT_INVALID_OPTIONS,
        SNAPSHOT_SOURCE_ROOT,
        SNAPSHOT_WALK,
        SNAPSHOT_SYMLINK,
        SNAPSHOT_REPARSE_POINT,
        SNAPSHOT_IRREGULAR,
        SNAPSHOT_UNSUPPORTED_NAME,
        SNAPSHOT_DESTINATION,
        SNAPSHOT_COPY,
        SNAPSHOT_CLEANUP_REFUSED,
        SNAPSHOT_CLEANUP_FAILED,
    ]
}
