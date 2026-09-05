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

/// A rule name the canonical registry does not know.
const RULE_UNKNOWN: ErrorCode = ErrorCode {
    code: "RM9001",
    summary: "a rule name the canonical registry does not know",
};

const INTERRUPTED: ErrorCode = ErrorCode {
    code: "RM0001",
    summary: "the caller cancelled the operation before it completed",
};

macro_rules! snapshot_code {
    // Named for the first area it served; every subsystem code is minted with it.
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
snapshot_code!(
    CARGO_TOOLCHAIN_NOT_FOUND,
    "RM1012",
    "the cargo or rustc executable could not be found"
);
snapshot_code!(
    CARGO_VERSION_UNREADABLE,
    "RM1013",
    "a -vV banner lacks its release or host line"
);
snapshot_code!(
    CARGO_COMMAND_FAILED,
    "RM1014",
    "a cargo command could not start or exited unsuccessfully"
);
snapshot_code!(
    CARGO_METADATA_UNPARSABLE,
    "RM1015",
    "cargo metadata printed something that is not its document"
);
snapshot_code!(
    CARGO_MESSAGE_UNPARSABLE,
    "RM1016",
    "a --message-format=json line is not a message"
);
snapshot_code!(
    DEP_INFO_UNREADABLE,
    "RM2001",
    "a dep-info file has no rule to read"
);
snapshot_code!(
    DEP_INFO_MISSING,
    "RM2002",
    "an artifact's dep-info file could not be read"
);
snapshot_code!(
    DISCOVER_FILE_UNREADABLE,
    "RM2003",
    "a source file a unit compiled could not be read"
);
snapshot_code!(
    DISCOVER_PARSE_FAILED,
    "RM2004",
    "a source file the compiler accepted does not parse as Rust for the engine"
);
snapshot_code!(
    DISCOVER_OUTSIDE_ROOT,
    "RM2005",
    "a unit compiled a file outside the workspace root"
);
snapshot_code!(
    DISCOVER_CATALOG_FAILED,
    "RM2006",
    "the candidates could not be assembled into a catalog"
);
snapshot_code!(
    DISCOVER_UNKNOWN_PACKAGE,
    "RM2007",
    "a selected package is not a workspace member"
);
snapshot_code!(
    INSTRUMENT_UNKNOWN_MUTANT,
    "RM3001",
    "a candidate is not in the catalog being instrumented"
);
snapshot_code!(
    INSTRUMENT_SOURCE_MISMATCH,
    "RM3002",
    "the source is not the one the candidates were discovered from"
);
snapshot_code!(
    INSTRUMENT_SITE_CONFLICT,
    "RM3003",
    "two rewrite sites partially overlap, which a syntax tree cannot produce"
);
snapshot_code!(
    INSTRUMENT_FLATTEN_FAILED,
    "RM3004",
    "an alternative could not be folded onto one line"
);
snapshot_code!(
    INSTRUMENT_SPLICE_FAILED,
    "RM3005",
    "the guards could not be applied to the file"
);
snapshot_code!(
    INSTRUMENT_LINES_MOVED,
    "RM3006",
    "a guard would have moved a line"
);
snapshot_code!(
    INSTRUMENT_INDEX_RESERVED,
    "RM3007",
    "a mutant index collides with the runtime's sentinel values"
);
snapshot_code!(
    VALIDATE_NOT_MUTANT_INDUCED,
    "RM4001",
    "the tree does not compile before any mutant is live"
);
snapshot_code!(
    VALIDATE_NOT_ISOLATED,
    "RM4002",
    "the mutants a compilation failure came from could not be isolated"
);
snapshot_code!(
    VALIDATE_ATTEMPT_FAILED,
    "RM4003",
    "an instrumented compilation could not be attempted at all"
);
snapshot_code!(
    SESSION_PRISTINE_BROKEN,
    "RM5001",
    "the workspace does not compile before anything is instrumented"
);
snapshot_code!(
    SESSION_VERIFY_FAILED,
    "RM5002",
    "the instrumented baseline fails a test the pristine tree passes"
);
snapshot_code!(
    SESSION_UNKNOWN_MUTANT,
    "RM5003",
    "no mutant of the catalog answers to the identity or prefix given"
);
snapshot_code!(
    SESSION_UNKNOWN_TARGET,
    "RM5004",
    "no test target of the session answers to the name given"
);
snapshot_code!(
    SESSION_NO_TARGETS,
    "RM5005",
    "the workspace builds no test target, so no mutant can be measured"
);
snapshot_code!(
    SESSION_WRITE_FAILED,
    "RM5006",
    "the instrumented tree could not be written"
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
    /// The toolchain could not be located or driven, or what it printed
    /// could not be read.
    #[error(transparent)]
    Cargo(#[from] crate::cargo::CargoError),
    /// The workspace's files could not be turned into a catalog.
    #[error(transparent)]
    Discover(#[from] crate::discover::DiscoverError),
    /// A file could not be rewritten to hold its mutants.
    #[error(transparent)]
    Instrument(#[from] crate::instrument::InstrumentError),
    /// Which candidates are real mutants could not be established.
    #[error(transparent)]
    Validate(#[from] crate::validate::ValidateError),
    /// The workspace could not be prepared, or a request against a prepared
    /// one could not be answered.
    #[error(transparent)]
    Session(#[from] crate::workspace::SessionError),
    /// The rules asked for are not the registry's.
    #[error(transparent)]
    Rule(#[from] crate::rule::RuleError),
}

impl EngineError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
            Self::Snapshot(error) => error.code(),
            Self::Cargo(error) => error.code(),
            Self::Discover(error) => error.code(),
            Self::Instrument(error) => error.code(),
            Self::Validate(error) => error.code(),
            Self::Session(error) => error.code(),
            Self::Rule(_) => RULE_UNKNOWN,
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
        CARGO_TOOLCHAIN_NOT_FOUND,
        CARGO_VERSION_UNREADABLE,
        CARGO_COMMAND_FAILED,
        CARGO_METADATA_UNPARSABLE,
        CARGO_MESSAGE_UNPARSABLE,
        DEP_INFO_UNREADABLE,
        DEP_INFO_MISSING,
        DISCOVER_FILE_UNREADABLE,
        DISCOVER_PARSE_FAILED,
        DISCOVER_OUTSIDE_ROOT,
        DISCOVER_CATALOG_FAILED,
        DISCOVER_UNKNOWN_PACKAGE,
        INSTRUMENT_UNKNOWN_MUTANT,
        INSTRUMENT_SOURCE_MISMATCH,
        INSTRUMENT_SITE_CONFLICT,
        INSTRUMENT_FLATTEN_FAILED,
        INSTRUMENT_SPLICE_FAILED,
        INSTRUMENT_LINES_MOVED,
        INSTRUMENT_INDEX_RESERVED,
        VALIDATE_NOT_MUTANT_INDUCED,
        VALIDATE_NOT_ISOLATED,
        VALIDATE_ATTEMPT_FAILED,
        SESSION_PRISTINE_BROKEN,
        SESSION_VERIFY_FAILED,
        SESSION_UNKNOWN_MUTANT,
        SESSION_UNKNOWN_TARGET,
        SESSION_NO_TARGETS,
        SESSION_WRITE_FAILED,
        RULE_UNKNOWN,
    ]
}
