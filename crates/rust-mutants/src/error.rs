// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failure modes of the engine, each with a stable code documented in `docs/errors.md`.

/// A stable, searchable identifier for one failure mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ErrorCode {
    /// The code, e.g. `RM0001`.
    pub code: &'static str,
    /// One line saying what the code means.
    pub summary: &'static str,
    /// What to do about it. Every code carries one.
    pub remedy: Option<&'static str>,
}

/// A rule name the canonical registry does not know.
const RULE_UNKNOWN: ErrorCode = ErrorCode {
    code: "RM9001",
    summary: "a rule name the canonical registry does not know",
    remedy: Some("`rust-mutants rules` lists every rule this release knows"),
};

/// A pattern that is not a pattern.
const GLOB_INVALID: ErrorCode = ErrorCode {
    code: "RM9002",
    summary: "a pattern the caller gave is not a pattern",
    remedy: Some(
        "a pattern is workspace-relative with forward slashes: `src/**/*.rs`, never a leading or trailing slash",
    ),
};

/// A duration that is not a duration.
const DURATION_INVALID: ErrorCode = ErrorCode {
    code: "RM9003",
    summary: "a duration the caller gave is not a duration",
    remedy: Some("write a duration as 30s, 5m, or 1h30m"),
};

/// The caller cancelled before the operation finished, so nothing it saw says anything.
pub const INTERRUPTED: ErrorCode = ErrorCode {
    code: "RM0001",
    summary: "the caller cancelled the operation before it completed",
    remedy: Some("nothing was left half-done; run it again when you are ready"),
};

/// A configuration file that could not be read. The command line reports it; the ledger of `RM` codes is one, so it lives here.
pub const CONFIG_UNREADABLE: ErrorCode = ErrorCode {
    code: "RM0002",
    summary: "a configuration file that could not be read",
    remedy: Some("check the file is readable by this user; the path names it"),
};

/// A configuration file that is not the document this version understands.
pub const CONFIG_UNPARSABLE: ErrorCode = ErrorCode {
    code: "RM0003",
    summary: "a configuration file that is not the document this version understands",
    remedy: Some(
        "`rust-mutants init` writes a file this release understands, with every key commented",
    ),
};

/// A configuration that parses but says something a run cannot honour.
pub const CONFIG_INVALID: ErrorCode = ErrorCode {
    code: "RM0004",
    summary: "a configuration that parses but says something a run cannot honour",
    remedy: Some(
        "the message says which key and why; `rust-mutants init` writes one that is valid",
    ),
};

/// A configuration whose `version` is not one this release understands.
pub const CONFIG_UNSUPPORTED_VERSION: ErrorCode = ErrorCode {
    code: "RM0005",
    summary: "a configuration whose version is not one this release understands",
    remedy: Some("this release reads version 1; a newer file needs a newer release"),
};

/// A process environment that already selects a mutant.
pub const ENVIRONMENT_RESERVED: ErrorCode = ErrorCode {
    code: "RM0006",
    summary: "a process environment that already selects a mutant or names a catalog",
    remedy: Some("unset the RUST_MUTANTS_ variable the message names and run again"),
};

/// A stored run report that is not there or cannot be read.
pub const REPORT_MISSING: ErrorCode = ErrorCode {
    code: "RM0007",
    summary: "a stored run report that is not there or cannot be read",
    remedy: Some("`rust-mutants report --list` names the runs that are stored under this root"),
};

/// A file a command would write that is already there.
pub const FILE_EXISTS: ErrorCode = ErrorCode {
    code: "RM0008",
    summary: "a file a command would write that is already there",
    remedy: Some(
        "remove the file, or name another path: a command here never writes over what it did not write",
    ),
};

/// A directory a command has to write to and could not.
pub const WRITE_FAILED: ErrorCode = ErrorCode {
    code: "RM0009",
    summary: "a report or configuration file that could not be written",
    remedy: Some(
        "check the directory exists and this user may write in it; the path names the file",
    ),
};

/// A coverage export that could not be read.
pub const COVERAGE_UNREADABLE: ErrorCode = ErrorCode {
    code: "RM6001",
    summary: "a coverage export that could not be read",
    remedy: Some(
        "run again without --coverage to measure without it, or check llvm-tools-preview is installed",
    ),
};

/// The LLVM tools the toolchain ships, not installed.
pub const COVERAGE_TOOLS_MISSING: ErrorCode = ErrorCode {
    code: "RM6002",
    summary: "the LLVM tools the toolchain ships are not installed",
    remedy: Some("rustup component add llvm-tools, or run with --no-coverage"),
};

/// One of the LLVM tools failed.
pub const COVERAGE_TOOL_FAILED: ErrorCode = ErrorCode {
    code: "RM6003",
    summary: "llvm-profdata or llvm-cov failed",
    remedy: Some(
        "`rustup component add llvm-tools-preview`, and check the versions match the toolchain in use",
    ),
};

/// A test process wrote no coverage profile at all.
pub const COVERAGE_NOTHING_WRITTEN: ErrorCode = ErrorCode {
    code: "RM6004",
    summary: "a test process wrote no coverage profile at all",
    remedy: Some(
        "the test process wrote no profile: check nothing in the suite sets LLVM_PROFILE_FILE for itself",
    ),
};

/// An executable a successful build named could not be read back for equivalence comparison.
pub const EQUIVALENCE_ARTIFACT_UNREADABLE: ErrorCode = ErrorCode {
    code: "RM7001",
    summary: "an executable a successful build named could not be read back",
    remedy: Some(
        "run again after checking nothing removes or rewrites target files while the build is being measured",
    ),
};

/// A change set that git could not be asked for.
pub const CHANGE_SET_UNAVAILABLE: ErrorCode = ErrorCode {
    code: "RM0010",
    summary: "a change set git could not be asked for, which is never read as nothing changing",
    remedy: Some("run inside a git working tree, or name what to measure with --include"),
};

/// Reports that are not the parts of one whole.
pub const MERGE_REFUSED: ErrorCode = ErrorCode {
    code: "RM0011",
    summary: "the reports given are not the parts of one catalog",
    remedy: Some(
        "merge the parts of one run: the same tree, the same catalog, and one report per --shard",
    ),
};

/// A source a report names that the tree does not hold.
pub const SOURCE_UNREADABLE: ErrorCode = ErrorCode {
    code: "RM0012",
    summary: "a source file a report names cannot be read from the root given",
    remedy: Some("pass --root at the tree the run measured, or check the file out again"),
};

/// An outcome cache could not be enumerated completely.
pub const CACHE_UNREADABLE: ErrorCode = ErrorCode {
    code: "RM0013",
    summary: "an outcome cache could not be enumerated completely",
    remedy: Some(
        "check the cache directory is readable by this user, or pass --cache-dir at another one",
    ),
};

/// Declares one error code. There is no form without a remedy, on purpose.
macro_rules! snapshot_code {
    ($name:ident, $code:literal, $summary:literal, $remedy:literal) => {
        pub(crate) const $name: ErrorCode = ErrorCode {
            code: $code,
            summary: $summary,
            remedy: Some($remedy),
        };
    };
}

snapshot_code!(
    SNAPSHOT_INVALID_OPTIONS,
    "RM1001",
    "snapshot options that cannot be honoured, such as an escaping report directory",
    "name a report directory inside the workspace; one that climbs out of it would have the run write where nothing sweeps"
);
snapshot_code!(
    SNAPSHOT_SOURCE_ROOT,
    "RM1002",
    "a source root that is relative, cannot be read, or is not a directory",
    "pass --root at a directory that exists and holds the workspace manifest"
);
snapshot_code!(
    SNAPSHOT_WALK,
    "RM1003",
    "an operating system failure while reading a tree",
    "this is what the operating system said; the path it names is the one to look at"
);
snapshot_code!(
    SNAPSHOT_SYMLINK,
    "RM1004",
    "a symbolic link inside the source tree, which is refused rather than followed or skipped",
    "a copy cannot follow a link out of the tree and cannot leave it dangling, so remove it or name its directory in [snapshot] omit"
);
snapshot_code!(
    SNAPSHOT_REPARSE_POINT,
    "RM1005",
    "a Windows reparse point (junction or mount point) inside the source tree",
    "a copy cannot reproduce a junction, so remove it or name its directory in [snapshot] omit"
);
snapshot_code!(
    SNAPSHOT_IRREGULAR,
    "RM1006",
    "a file that is neither a directory nor a regular file: a device, a socket, a named pipe",
    "a device, socket, or pipe is not a file a copy can hold; name its directory in [snapshot] omit"
);
snapshot_code!(
    SNAPSHOT_UNSUPPORTED_NAME,
    "RM1007",
    "a file name that cannot round-trip through a slash-separated relative path",
    "rename the file: a run says the same thing on every platform, and this name cannot"
);
snapshot_code!(
    SNAPSHOT_DESTINATION,
    "RM1008",
    "the snapshot directory could not be created or claimed",
    "check TMPDIR is a directory this user may write in, and that there is room under it"
);
snapshot_code!(
    SNAPSHOT_COPY,
    "RM1009",
    "a failure while copying the tree into the snapshot",
    "check there is room under TMPDIR, and that nothing is writing the tree while it is copied"
);
snapshot_code!(
    SNAPSHOT_CLEANUP_REFUSED,
    "RM1010",
    "a cleanup refused because the recorded directory does not look like a snapshot directory",
    "the recorded path is not one this tool made; remove it yourself rather than having a tool remove a directory it cannot identify"
);
snapshot_code!(
    SNAPSHOT_CLEANUP_FAILED,
    "RM1011",
    "a snapshot directory that survived every removal attempt",
    "something is holding it open, and a sweep cannot take it back; `rust-mutants cache` says where it is"
);
snapshot_code!(
    CARGO_TOOLCHAIN_NOT_FOUND,
    "RM1012",
    "the cargo or rustc executable could not be found",
    "install the toolchain, or name cargo with --cargo, or put it on the PATH this process was given"
);
snapshot_code!(
    CARGO_VERSION_UNREADABLE,
    "RM1013",
    "a -vV banner lacks its release or host line",
    "the toolchain answered something this release cannot read; `rustup update` and try again"
);
snapshot_code!(
    CARGO_COMMAND_FAILED,
    "RM1014",
    "a cargo command could not start or exited unsuccessfully",
    "run the same cargo command yourself: what it says there is what it said here"
);
snapshot_code!(
    CARGO_METADATA_UNPARSABLE,
    "RM1015",
    "cargo metadata printed something that is not its document",
    "run `cargo metadata` yourself on this tree; what it prints is what could not be read"
);
snapshot_code!(
    CARGO_MESSAGE_UNPARSABLE,
    "RM1016",
    "a --message-format=json line is not a message",
    "run the same cargo command with --message-format=json yourself; what it prints is what could not be read"
);
snapshot_code!(
    WORKSPACE_REACHES_OUTSIDE,
    "RM1017",
    "the workspace reads code from outside itself, which a copy of it does not hold",
    "--allow-outside DIR copies that directory into the copy where the tree reaches it, or [project] allow_outside does"
);
snapshot_code!(
    ROOT_IS_NOT_THE_WORKSPACE,
    "RM1018",
    "the root is a member of a workspace rather than the workspace",
    "run with --root at the workspace root the message names, and --package to narrow it"
);
snapshot_code!(
    SNAPSHOT_LAYOUT,
    "RM1019",
    "a directory a run would copy has no place in the copy that keeps every path into it resolving",
    "--allow-outside takes an existing absolute directory outside the tree and on the same filesystem root as it; a copy reproduces the shape of what it copies, and cannot hold a directory that is the tree, holds it, or lies across a volume"
);
snapshot_code!(
    MANIFEST_UNREADABLE,
    "RM1020",
    "a manifest a run has to read is there and could not be read",
    "read the manifest the message names yourself: a run decides what it may copy, which targets carry a harness, and which lints a crate forbids from it, and an empty answer to any of those is a different run rather than a missing one"
);
snapshot_code!(
    DEP_INFO_UNREADABLE,
    "RM2001",
    "a dep-info file has no rule to read",
    "run `cargo test --no-run` yourself, then try again: a build that did not finish leaves this behind"
);
snapshot_code!(
    DEP_INFO_MISSING,
    "RM2002",
    "an artifact's dep-info file could not be read",
    "run `cargo clean` and try again; a dep-info file from an interrupted build cannot be read"
);
snapshot_code!(
    DISCOVER_FILE_UNREADABLE,
    "RM2003",
    "a source file a unit compiled could not be read",
    "the file a unit compiled is not readable from the copy; check it is not written while the run reads it"
);
snapshot_code!(
    DISCOVER_PARSE_FAILED,
    "RM2004",
    "a source file the compiler accepted does not parse as Rust for the engine",
    "this release parses the edition the manifest declares; a file the compiler accepts and this does not is a defect in this tool, and the path names it"
);
snapshot_code!(
    DISCOVER_OUTSIDE_ROOT,
    "RM2005",
    "a unit compiled a file outside the workspace root",
    "name the directory in allow_outside, or move the file into the workspace: a run measures a copy, and what is outside it is not in the copy"
);
snapshot_code!(
    DISCOVER_CATALOG_FAILED,
    "RM2006",
    "the candidates could not be assembled into a catalog",
    "this is a defect in this tool: no candidate the walk produces should be one the catalog refuses"
);

/// A discovered candidate broke an identity invariant before it could be displayed.
pub const CANDIDATE_INVALID: ErrorCode = DISCOVER_CATALOG_FAILED;
snapshot_code!(
    DISCOVER_UNKNOWN_PACKAGE,
    "RM2007",
    "a selected package is not a workspace member",
    "name a package `cargo metadata` lists for this workspace; a name no member has narrows nothing"
);
snapshot_code!(
    DISCOVER_ANNOTATION_WITHOUT_REASON,
    "RM2008",
    "a rust-mutants: skip marker names no reason",
    "write the marker as `rust-mutants: skip <why this place is not worth measuring>`"
);
snapshot_code!(
    DISCOVER_UNKNOWN_ANNOTATION,
    "RM2009",
    "a rust-mutants marker names a directive this release does not know",
    "`skip` is the only directive this release knows"
);
snapshot_code!(
    INSTRUMENT_UNKNOWN_MUTANT,
    "RM3001",
    "a candidate is not in the catalog being instrumented",
    "this is a defect in this tool: the catalog and the instrumentation disagree about which mutants exist"
);
snapshot_code!(
    INSTRUMENT_SOURCE_MISMATCH,
    "RM3002",
    "the source is not the one the candidates were discovered from",
    "the file changed between being read and being instrumented; make sure nothing writes the tree while a run is preparing"
);
snapshot_code!(
    INSTRUMENT_SITE_CONFLICT,
    "RM3003",
    "two rewrite sites partially overlap, which a syntax tree cannot produce",
    "this is a defect in this tool: two rules claimed overlapping bytes, which a syntax tree cannot produce"
);
snapshot_code!(
    INSTRUMENT_FLATTEN_FAILED,
    "RM3004",
    "an alternative could not be folded onto one line",
    "this is a defect in this tool: a guard has to fit on the line it replaces, and this one did not"
);
snapshot_code!(
    INSTRUMENT_SPLICE_FAILED,
    "RM3005",
    "the guards could not be applied to the file",
    "this is a defect in this tool: the guards could not be written back over the file they were cut from"
);
snapshot_code!(
    INSTRUMENT_LINES_MOVED,
    "RM3006",
    "a guard would have moved a line",
    "this is a defect in this tool: a guard moved a line, and every position a run reports is relative to lines that did not move"
);
snapshot_code!(
    INSTRUMENT_INDEX_RESERVED,
    "RM3007",
    "a mutant index makes the generated runtime's inclusive window overflow",
    "this is a defect in this tool: the catalog outgrew the u32 window the generated runtime can represent"
);
snapshot_code!(
    VALIDATE_NOT_MUTANT_INDUCED,
    "RM4001",
    "the tree does not compile before any mutant is live",
    "make `cargo test --no-run` pass on the tree as committed, then run again"
);
snapshot_code!(
    VALIDATE_NOT_ISOLATED,
    "RM4002",
    "the mutants a compilation failure came from could not be isolated",
    "run `cargo test --no-run` on the tree yourself; the compilation failed for a reason this tool could not attribute to one mutant"
);
snapshot_code!(
    VALIDATE_ATTEMPT_FAILED,
    "RM4003",
    "an instrumented compilation could not be attempted at all",
    "the compilation could not be started at all: check cargo runs on this tree and that there is room under TMPDIR"
);
snapshot_code!(
    SESSION_PRISTINE_BROKEN,
    "RM5001",
    "the workspace does not compile before anything is instrumented",
    "make `cargo test --no-run` pass on the tree as committed, then run again"
);
snapshot_code!(
    SESSION_VERIFY_FAILED,
    "RM5002",
    "the instrumented baseline fails a test the pristine tree passes",
    "[execution] skip_targets leaves that target out; --no-verify makes every result a result about instrumentation"
);
snapshot_code!(
    SESSION_UNKNOWN_MUTANT,
    "RM5003",
    "no mutant of the catalog answers to the identity or prefix given",
    "`rust-mutants catalog` lists what this run holds; an identity is re-minted whenever its file changes, so name the mutation by `path:item:rule` instead"
);
snapshot_code!(
    SESSION_UNKNOWN_TARGET,
    "RM5004",
    "no test target of the session answers to the name given",
    "`rust-mutants catalog` names every target this session built; a name no target has starts nothing"
);
snapshot_code!(
    SESSION_NO_TARGETS,
    "RM5005",
    "the workspace builds no test target, so no mutant can be measured",
    "the workspace has nothing that tests, so there is nothing a mutation could be put to; write a test, or point --root at the workspace that has them"
);
snapshot_code!(
    SESSION_WRITE_FAILED,
    "RM5006",
    "the instrumented tree could not be written",
    "check there is room under TMPDIR and that nothing is removing the run's directory while it writes"
);

/// Every failure the engine reports.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The caller cancelled the operation before it completed.
    #[error("the operation was interrupted before it completed")]
    Interrupted,
    /// The source tree could not be copied into a disposable snapshot, or the snapshot could not be removed.
    #[error(transparent)]
    Snapshot(#[from] crate::snapshot::SnapshotError),
    /// The toolchain could not be located or driven, or what it printed could not be read.
    #[error(transparent)]
    Cargo(#[from] crate::cargo::CargoError),
    /// Compiler flags from the environment could not be preserved exactly.
    #[error(transparent)]
    CargoConfig(#[from] crate::cargo::config::ConfigError),
    /// The workspace's files could not be turned into a catalog.
    #[error(transparent)]
    Discover(#[from] crate::discover::DiscoverError),
    /// A file could not be rewritten to hold its mutants.
    #[error(transparent)]
    Instrument(#[from] crate::instrument::InstrumentError),
    /// Which candidates are real mutants could not be established.
    #[error(transparent)]
    Validate(#[from] crate::validate::ValidateError),
    /// The workspace could not be prepared, or a request against a prepared one could not be answered.
    #[error(transparent)]
    Session(#[from] crate::workspace::SessionError),
    /// The rules asked for are not the registry's.
    #[error(transparent)]
    Rule(#[from] crate::rule::RuleError),
    /// A pattern the caller gave is not a pattern.
    #[error(transparent)]
    Glob(#[from] crate::glob::GlobError),
    /// A duration the caller gave is not a duration.
    #[error(transparent)]
    Duration(#[from] crate::duration::DurationError),
    /// A successful build named an executable whose bytes could not be read back for equivalence comparison.
    #[error(transparent)]
    Equivalence(#[from] crate::equivalence::artifacts::ArtifactError),
    /// A durable outcome identity was not canonical.
    #[error(transparent)]
    OutcomeIdentity(#[from] crate::id::HexDigestError),
    /// A durable outcome could not be read or written exactly.
    #[error(transparent)]
    Outcomes(#[from] crate::outcomes::StoreError),
    /// A remembered passing baseline could not be read or checked exactly.
    #[error(transparent)]
    BaselineCache(#[from] crate::session::BaselineCacheError),
}

impl EngineError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Interrupted => INTERRUPTED,
            Self::Snapshot(error) => error.code(),
            Self::Cargo(error) => error.code(),
            Self::CargoConfig(_) => CONFIG_UNREADABLE,
            Self::Discover(error) => error.code(),
            Self::Instrument(error) => error.code(),
            Self::Validate(error) => error.code(),
            Self::Session(error) => error.code(),
            Self::Rule(_) => RULE_UNKNOWN,
            Self::Glob(_) => GLOB_INVALID,
            Self::Duration(_) => DURATION_INVALID,
            Self::Equivalence(_) => EQUIVALENCE_ARTIFACT_UNREADABLE,
            Self::OutcomeIdentity(_) | Self::Outcomes(_) | Self::BaselineCache(_) => {
                CACHE_UNREADABLE
            }
        }
    }
}

/// Every code the rust-mutants product reports, the engine's and the command line's alike, in code order.
#[must_use]
pub const fn error_codes() -> &'static [ErrorCode] {
    &[
        INTERRUPTED,
        CONFIG_UNREADABLE,
        CONFIG_UNPARSABLE,
        CONFIG_INVALID,
        CONFIG_UNSUPPORTED_VERSION,
        ENVIRONMENT_RESERVED,
        REPORT_MISSING,
        FILE_EXISTS,
        WRITE_FAILED,
        CHANGE_SET_UNAVAILABLE,
        MERGE_REFUSED,
        SOURCE_UNREADABLE,
        CACHE_UNREADABLE,
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
        WORKSPACE_REACHES_OUTSIDE,
        ROOT_IS_NOT_THE_WORKSPACE,
        SNAPSHOT_LAYOUT,
        MANIFEST_UNREADABLE,
        DEP_INFO_UNREADABLE,
        DEP_INFO_MISSING,
        DISCOVER_FILE_UNREADABLE,
        DISCOVER_PARSE_FAILED,
        DISCOVER_OUTSIDE_ROOT,
        DISCOVER_CATALOG_FAILED,
        DISCOVER_UNKNOWN_PACKAGE,
        DISCOVER_ANNOTATION_WITHOUT_REASON,
        DISCOVER_UNKNOWN_ANNOTATION,
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
        COVERAGE_UNREADABLE,
        COVERAGE_TOOLS_MISSING,
        COVERAGE_TOOL_FAILED,
        COVERAGE_NOTHING_WRITTEN,
        EQUIVALENCE_ARTIFACT_UNREADABLE,
        RULE_UNKNOWN,
        GLOB_INVALID,
        DURATION_INVALID,
    ]
}
