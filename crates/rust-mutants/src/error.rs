// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Failure modes of the engine, each with a stable code documented in `docs/errors.md`.

/// The codes, and the one place an [`ErrorCode`] is made.
mod table {
    /// A stable, searchable identifier for one failure mode.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ErrorCode {
        /// The code, e.g. `RM0001`.
        pub code: &'static str,
        /// One line saying what the code means.
        pub summary: &'static str,
        /// What to do about it.
        /// Every code carries one.
        pub remedy: Option<&'static str>,
        sealed: Sealed,
    }

    /// What only this module can write, so only this module makes an [`ErrorCode`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct Sealed;

    /// Every failure mode, one variant per code, in code order.
    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants,
    )]
    pub enum RmCode {
        /// The caller cancelled before the operation finished, so nothing it saw says anything.
        Interrupted,
        /// A configuration file that could not be read.
        /// The command line reports it; the ledger of `RM` codes is one, so it lives here.
        ConfigUnreadable,
        /// A configuration file that is not the document this version understands.
        ConfigUnparsable,
        /// A configuration that parses but says something a run cannot honour.
        ConfigInvalid,
        /// A configuration whose `version` is not one this release understands.
        ConfigUnsupportedVersion,
        /// A process environment that already selects a mutant.
        EnvironmentReserved,
        /// A stored run report that is not there or cannot be read.
        ReportMissing,
        /// A file a command would write that is already there.
        FileExists,
        /// A directory a command has to write to and could not.
        WriteFailed,
        /// A change set that git could not be asked for.
        ChangeSetUnavailable,
        /// Reports that are not the parts of one whole.
        MergeRefused,
        /// A source a report names that the tree does not hold.
        SourceUnreadable,
        /// An outcome cache could not be enumerated completely.
        CacheUnreadable,
        /// A continuous integration host a command was asked to write for, which the environment does not provide.
        CiHostUnavailable,
        /// A workspace root outside the checkout a host places annotations in.
        CiRootOutsideCheckout,
        /// A file the host named for a step's summary or outputs, which could not be appended to.
        CiSinkUnwritable,
        /// Snapshot options that cannot be honoured, such as an escaping report directory.
        SnapshotInvalidOptions,
        /// A source root that is relative, cannot be read, or is not a directory.
        SnapshotSourceRoot,
        /// An operating system failure while reading a tree.
        SnapshotWalk,
        /// A symbolic link inside the source tree, which is refused rather than followed or skipped.
        SnapshotSymlink,
        /// A Windows reparse point (junction or mount point) inside the source tree.
        SnapshotReparsePoint,
        /// A file that is neither a directory nor a regular file: a device, a socket, a named pipe.
        SnapshotIrregular,
        /// A file name that cannot round-trip through a slash-separated relative path.
        SnapshotUnsupportedName,
        /// The snapshot directory could not be created or claimed.
        SnapshotDestination,
        /// A failure while copying the tree into the snapshot.
        SnapshotCopy,
        /// A cleanup refused because the recorded directory does not look like a snapshot directory.
        SnapshotCleanupRefused,
        /// A snapshot directory that survived every removal attempt.
        SnapshotCleanupFailed,
        /// The cargo or rustc executable could not be found.
        CargoToolchainNotFound,
        /// A -vV banner lacks its release or host line.
        CargoVersionUnreadable,
        /// A cargo command could not start or exited unsuccessfully.
        CargoCommandFailed,
        /// Cargo metadata printed something that is not its document.
        CargoMetadataUnparsable,
        /// A --message-format=json line is not a message.
        CargoMessageUnparsable,
        /// The workspace reads code from outside itself, which a copy of it does not hold.
        WorkspaceReachesOutside,
        /// The root is a member of a workspace rather than the workspace.
        RootIsNotTheWorkspace,
        /// A directory a run would copy has no place in the copy that keeps every path into it resolving.
        SnapshotLayout,
        /// A manifest a run has to read is there and could not be read.
        ManifestUnreadable,
        /// A name handed to a capability directory is not one path component.
        CapdirNameRefused,
        /// A target directory's record of what its members were built from could not be read or written, or a unit it names as stale could not be forgotten.
        BuildLedgerUnreadable,
        /// A bare `cargo` from the copy a run measures answers as no toolchain the run can put first on the tests' search path.
        TestsToolchainUnreachable,
        /// A dep-info file has no rule to read.
        DepInfoUnreadable,
        /// An artifact's dep-info file could not be read.
        DepInfoMissing,
        /// A source file a unit compiled could not be read.
        DiscoverFileUnreadable,
        /// A source file the compiler accepted does not parse as Rust for the engine.
        DiscoverParseFailed,
        /// A unit compiled a file outside the workspace root.
        DiscoverOutsideRoot,
        /// The candidates could not be assembled into a catalog.
        DiscoverCatalogFailed,
        /// A selected package is not a workspace member.
        DiscoverUnknownPackage,
        /// A rust-mutants: skip marker names no reason.
        DiscoverAnnotationWithoutReason,
        /// A rust-mutants marker names a directive this release does not know.
        DiscoverUnknownAnnotation,
        /// A candidate is not in the catalog being instrumented.
        InstrumentUnknownMutant,
        /// The source is not the one the candidates were discovered from.
        InstrumentSourceMismatch,
        /// Two rewrite sites partially overlap, which a syntax tree cannot produce.
        InstrumentSiteConflict,
        /// An alternative could not be folded onto one line.
        InstrumentFlattenFailed,
        /// The guards could not be applied to the file.
        InstrumentSpliceFailed,
        /// A guard would have moved a line.
        InstrumentLinesMoved,
        /// A mutant index makes the generated runtime's inclusive window overflow.
        InstrumentIndexReserved,
        /// The rewritten file does not read as Rust, down to what every identity macro holds.
        InstrumentUnparsable,
        /// The tree does not compile before any mutant is live.
        ValidateNotMutantInduced,
        /// The mutants a compilation failure came from could not be isolated.
        ValidateNotIsolated,
        /// An instrumented compilation could not be attempted at all.
        ValidateAttemptFailed,
        /// The workspace does not compile before anything is instrumented.
        SessionPristineBroken,
        /// The instrumented baseline fails a test the pristine tree passes.
        SessionVerifyFailed,
        /// No mutant of the catalog answers to the identity or prefix given.
        SessionUnknownMutant,
        /// No test target of the session answers to the name given.
        SessionUnknownTarget,
        /// The workspace builds no test target, so no mutant can be measured.
        SessionNoTargets,
        /// The instrumented tree could not be written.
        SessionWriteFailed,
        /// The crate planted for the routing layers could not be written.
        SentinelUnwritable,
        /// The crate planted for the routing layers was built by another compiler than the run's.
        SentinelOtherToolchain,
        /// A fault was asked to run beside something that is not a mutation, or what was named beside it is not a fault.
        SessionNotBeside,
        /// The system gave no randomness for the nonce that ties a crash's notice to its execution.
        SessionNonceUnavailable,
        /// A mutant's execution changed the test executables the run starts.
        SessionApparatusChanged,
        /// A coverage export that could not be read.
        CoverageUnreadable,
        /// The LLVM tools the toolchain ships, not installed.
        CoverageToolsMissing,
        /// One of the LLVM tools failed.
        CoverageToolFailed,
        /// A test process wrote no coverage profile at all.
        CoverageNothingWritten,
        /// An executable a successful build named could not be read back for equivalence comparison.
        EquivalenceArtifactUnreadable,
        /// A rule name the canonical registry does not know.
        RuleUnknown,
        /// A pattern that is not a pattern.
        GlobInvalid,
        /// A duration that is not a duration.
        DurationInvalid,
    }

    impl RmCode {
        /// The code, what it means, and what to do about it.
        #[must_use]
        #[expect(
            clippy::too_many_lines,
            reason = "one arm per code: the table is the function, and splitting it would split the one match that keeps it total"
        )]
        pub const fn error_code(self) -> ErrorCode {
            match self {
                Self::Interrupted => ErrorCode {
                    code: "RM0001",
                    summary: "the caller cancelled the operation before it completed",
                    remedy: Some("nothing was left half-done; run it again when you are ready"),
                    sealed: Sealed,
                },
                Self::ConfigUnreadable => ErrorCode {
                    code: "RM0002",
                    summary: "a configuration file that could not be read",
                    remedy: Some("check the file is readable by this user; the path names it"),
                    sealed: Sealed,
                },
                Self::ConfigUnparsable => ErrorCode {
                    code: "RM0003",
                    summary: "a configuration file that is not the document this version understands",
                    remedy: Some(
                        "`rust-mutants init` writes a file this release understands, with every key commented",
                    ),
                    sealed: Sealed,
                },
                Self::ConfigInvalid => ErrorCode {
                    code: "RM0004",
                    summary: "a configuration that parses but says something a run cannot honour",
                    remedy: Some(
                        "the message says which key and why; `rust-mutants init` writes one that is valid",
                    ),
                    sealed: Sealed,
                },
                Self::ConfigUnsupportedVersion => ErrorCode {
                    code: "RM0005",
                    summary: "a configuration whose version is not one this release understands",
                    remedy: Some(
                        "this release reads version 1; a newer file needs a newer release",
                    ),
                    sealed: Sealed,
                },
                Self::EnvironmentReserved => ErrorCode {
                    code: "RM0006",
                    summary: "a process environment that already selects a mutant or names a catalog",
                    remedy: Some(
                        "unset the RUST_MUTANTS_ variable the message names and run again",
                    ),
                    sealed: Sealed,
                },
                Self::ReportMissing => ErrorCode {
                    code: "RM0007",
                    summary: "a stored run report that is not there or cannot be read",
                    remedy: Some(
                        "every run is a directory named for it in the configured reports directory (`reports/mutation` by default); `rust-mutants report` with no `--run` reads the newest",
                    ),
                    sealed: Sealed,
                },
                Self::FileExists => ErrorCode {
                    code: "RM0008",
                    summary: "a file a command would write that is already there",
                    remedy: Some(
                        "remove the file, or name another path: a command here never writes over what it did not write",
                    ),
                    sealed: Sealed,
                },
                Self::WriteFailed => ErrorCode {
                    code: "RM0009",
                    summary: "a report or configuration file that could not be written",
                    remedy: Some(
                        "check the directory exists and this user may write in it; the path names the file",
                    ),
                    sealed: Sealed,
                },
                Self::ChangeSetUnavailable => ErrorCode {
                    code: "RM0010",
                    summary: "a change set git could not be asked for, which is never read as nothing changing",
                    remedy: Some(
                        "run inside a git working tree, or name what to measure with --include",
                    ),
                    sealed: Sealed,
                },
                Self::MergeRefused => ErrorCode {
                    code: "RM0011",
                    summary: "the reports given are not the parts of one catalog",
                    remedy: Some(
                        "merge the parts of one run: the same tree, the same catalog, and one report per --shard",
                    ),
                    sealed: Sealed,
                },
                Self::SourceUnreadable => ErrorCode {
                    code: "RM0012",
                    summary: "a source file a report names cannot be read from the root given",
                    remedy: Some(
                        "pass --root at the tree the run measured, or check the file out again",
                    ),
                    sealed: Sealed,
                },
                Self::CacheUnreadable => ErrorCode {
                    code: "RM0013",
                    summary: "an outcome cache could not be enumerated completely",
                    remedy: Some(
                        "check the cache directory is readable by this user, or pass --cache-dir at another one",
                    ),
                    sealed: Sealed,
                },
                Self::CiHostUnavailable => ErrorCode {
                    code: "RM0014",
                    summary: "a continuous integration host was asked for that the environment does not provide",
                    remedy: Some(
                        "run the step inside GitHub Actions, which names GITHUB_STEP_SUMMARY, GITHUB_OUTPUT and GITHUB_WORKSPACE, or pass --host plain",
                    ),
                    sealed: Sealed,
                },
                Self::CiRootOutsideCheckout => ErrorCode {
                    code: "RM0015",
                    summary: "a workspace root outside the checkout the host places annotations in",
                    remedy: Some(
                        "pass --root at the workspace inside the checkout GITHUB_WORKSPACE names",
                    ),
                    sealed: Sealed,
                },
                Self::CiSinkUnwritable => ErrorCode {
                    code: "RM0016",
                    summary: "a file the host named for a step's summary or outputs could not be appended to",
                    remedy: Some(
                        "the runner names the file for this step; check no earlier step removed it or its directory",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotInvalidOptions => ErrorCode {
                    code: "RM1001",
                    summary: "snapshot options that cannot be honoured, such as an escaping report directory",
                    remedy: Some(
                        "name a report directory inside the workspace; one that climbs out of it would have the run write where nothing sweeps",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotSourceRoot => ErrorCode {
                    code: "RM1002",
                    summary: "a source root that is relative, cannot be read, or is not a directory",
                    remedy: Some(
                        "pass --root at a directory that exists and holds the workspace manifest",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotWalk => ErrorCode {
                    code: "RM1003",
                    summary: "an operating system failure while reading a tree",
                    remedy: Some(
                        "this is what the operating system said; the path it names is the one to look at",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotSymlink => ErrorCode {
                    code: "RM1004",
                    summary: "a symbolic link inside the source tree, which is refused rather than followed or skipped",
                    remedy: Some(
                        "a copy cannot follow a link out of the tree and cannot leave it dangling, so remove it or name its directory in [snapshot] omit",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotReparsePoint => ErrorCode {
                    code: "RM1005",
                    summary: "a Windows reparse point (junction or mount point) inside the source tree",
                    remedy: Some(
                        "a copy cannot reproduce a junction, so remove it or name its directory in [snapshot] omit",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotIrregular => ErrorCode {
                    code: "RM1006",
                    summary: "a file that is neither a directory nor a regular file: a device, a socket, a named pipe",
                    remedy: Some(
                        "a device, socket, or pipe is not a file a copy can hold; name its directory in [snapshot] omit",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotUnsupportedName => ErrorCode {
                    code: "RM1007",
                    summary: "a file name that cannot round-trip through a slash-separated relative path",
                    remedy: Some(
                        "rename the file: a run says the same thing on every platform, and this name cannot",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotDestination => ErrorCode {
                    code: "RM1008",
                    summary: "the snapshot directory could not be created or claimed",
                    remedy: Some(
                        "check TMPDIR is a directory this user may write in, and that there is room under it",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotCopy => ErrorCode {
                    code: "RM1009",
                    summary: "a failure while copying the tree into the snapshot",
                    remedy: Some(
                        "check there is room under TMPDIR, and that nothing is writing the tree while it is copied",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotCleanupRefused => ErrorCode {
                    code: "RM1010",
                    summary: "a cleanup refused because the recorded directory does not look like a snapshot directory",
                    remedy: Some(
                        "the recorded path is not one this tool made; remove it yourself rather than having a tool remove a directory it cannot identify",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotCleanupFailed => ErrorCode {
                    code: "RM1011",
                    summary: "a snapshot directory that survived every removal attempt",
                    remedy: Some(
                        "something is holding it open, and a sweep cannot take it back; `rust-mutants cache` says where it is",
                    ),
                    sealed: Sealed,
                },
                Self::CargoToolchainNotFound => ErrorCode {
                    code: "RM1012",
                    summary: "the cargo or rustc executable could not be found",
                    remedy: Some(
                        "install the toolchain, or name cargo with --cargo, or put it on the PATH this process was given",
                    ),
                    sealed: Sealed,
                },
                Self::CargoVersionUnreadable => ErrorCode {
                    code: "RM1013",
                    summary: "a -vV banner lacks its release or host line",
                    remedy: Some(
                        "the toolchain answered something this release cannot read; `rustup update` and try again",
                    ),
                    sealed: Sealed,
                },
                Self::CargoCommandFailed => ErrorCode {
                    code: "RM1014",
                    summary: "a cargo command could not start or exited unsuccessfully",
                    remedy: Some(
                        "run the same cargo command yourself: what it says there is what it said here",
                    ),
                    sealed: Sealed,
                },
                Self::CargoMetadataUnparsable => ErrorCode {
                    code: "RM1015",
                    summary: "cargo metadata printed something that is not its document",
                    remedy: Some(
                        "run `cargo metadata` yourself on this tree; what it prints is what could not be read",
                    ),
                    sealed: Sealed,
                },
                Self::CargoMessageUnparsable => ErrorCode {
                    code: "RM1016",
                    summary: "a line of `cargo … --message-format=json` output is not a message",
                    remedy: Some(
                        "run the same `cargo … --message-format=json` yourself; what it prints is what could not be read",
                    ),
                    sealed: Sealed,
                },
                Self::WorkspaceReachesOutside => ErrorCode {
                    code: "RM1017",
                    summary: "the workspace reads code from outside itself, which a copy of it does not hold",
                    remedy: Some(
                        "--allow-outside DIR copies that directory into the copy where the tree reaches it, or [project] allow_outside does",
                    ),
                    sealed: Sealed,
                },
                Self::RootIsNotTheWorkspace => ErrorCode {
                    code: "RM1018",
                    summary: "the root is a member of a workspace rather than the workspace",
                    remedy: Some(
                        "run with --root at the workspace root the message names, and --package to narrow it",
                    ),
                    sealed: Sealed,
                },
                Self::SnapshotLayout => ErrorCode {
                    code: "RM1019",
                    summary: "a directory a run would copy has no place in the copy that keeps every path into it resolving",
                    remedy: Some(
                        "--allow-outside takes an existing absolute directory outside the tree and on the same filesystem root as it; a copy reproduces the shape of what it copies, and cannot hold a directory that is the tree, holds it, or lies across a volume",
                    ),
                    sealed: Sealed,
                },
                Self::ManifestUnreadable => ErrorCode {
                    code: "RM1020",
                    summary: "a manifest a run has to read is there and could not be read",
                    remedy: Some(
                        "read the manifest the message names yourself: a run decides what it may copy, which targets carry a harness, and which lints a crate forbids from it, and an empty answer to any of those is a different run rather than a missing one",
                    ),
                    sealed: Sealed,
                },
                Self::CapdirNameRefused => ErrorCode {
                    code: "RM1021",
                    summary: "a name handed to a capability directory is not one path component",
                    remedy: Some(
                        "a store names its entries itself; a name that could reach a parent, a stream or a device is a defect in the caller, so report it",
                    ),
                    sealed: Sealed,
                },
                Self::BuildLedgerUnreadable => ErrorCode {
                    code: "RM1022",
                    summary: "a target directory's record of what its members were built from could not be read or written, or a unit it names as stale could not be forgotten",
                    remedy: Some(
                        "remove the target directory the message names: a run compiles again what it cannot vouch for, and a record it cannot read is one it cannot vouch by",
                    ),
                    sealed: Sealed,
                },
                Self::TestsToolchainUnreachable => ErrorCode {
                    code: "RM1023",
                    summary: "a bare `cargo` from the copy a run measures answers as no toolchain the run can put first on the tests' search path",
                    remedy: Some(
                        "run `cargo -vV` from the directory the message names with the environment the run was given: a shim that chooses a toolchain by the directory it runs in has to answer there, or the toolchain rustc names has to hold a cargo",
                    ),
                    sealed: Sealed,
                },
                Self::DepInfoUnreadable => ErrorCode {
                    code: "RM2001",
                    summary: "a dep-info file has no rule to read",
                    remedy: Some(
                        "run `cargo test --no-run` yourself, then try again: a build that did not finish leaves this behind",
                    ),
                    sealed: Sealed,
                },
                Self::DepInfoMissing => ErrorCode {
                    code: "RM2002",
                    summary: "an artifact's dep-info file could not be read",
                    remedy: Some(
                        "run `cargo clean` and try again; a dep-info file from an interrupted build cannot be read",
                    ),
                    sealed: Sealed,
                },
                Self::DiscoverFileUnreadable => ErrorCode {
                    code: "RM2003",
                    summary: "a source file a unit compiled could not be read",
                    remedy: Some(
                        "the file a unit compiled is not readable from the copy; check it is not written while the run reads it",
                    ),
                    sealed: Sealed,
                },
                Self::DiscoverParseFailed => ErrorCode {
                    code: "RM2004",
                    summary: "a source file the compiler accepted does not parse as Rust for the engine",
                    remedy: Some(
                        "this release parses the edition the manifest declares; a file the compiler accepts and this does not is a defect in this tool, and the path names it",
                    ),
                    sealed: Sealed,
                },
                Self::DiscoverOutsideRoot => ErrorCode {
                    code: "RM2005",
                    summary: "a unit compiled a file outside the workspace root",
                    remedy: Some(
                        "name the directory in allow_outside, or move the file into the workspace: a run measures a copy, and what is outside it is not in the copy",
                    ),
                    sealed: Sealed,
                },
                Self::DiscoverCatalogFailed => ErrorCode {
                    code: "RM2006",
                    summary: "the candidates could not be assembled into a catalog",
                    remedy: Some(
                        "this is a defect in this tool: no candidate the walk produces should be one the catalog refuses",
                    ),
                    sealed: Sealed,
                },
                Self::DiscoverUnknownPackage => ErrorCode {
                    code: "RM2007",
                    summary: "a selected package is not a workspace member",
                    remedy: Some(
                        "name a package `cargo metadata` lists for this workspace; a name no member has narrows nothing",
                    ),
                    sealed: Sealed,
                },
                Self::DiscoverAnnotationWithoutReason => ErrorCode {
                    code: "RM2008",
                    summary: "a rust-mutants: skip marker names no reason",
                    remedy: Some(
                        "write the marker as `rust-mutants: skip <why this place is not worth measuring>`",
                    ),
                    sealed: Sealed,
                },
                Self::DiscoverUnknownAnnotation => ErrorCode {
                    code: "RM2009",
                    summary: "a rust-mutants marker names a directive this release does not know",
                    remedy: Some("`skip` is the only directive this release knows"),
                    sealed: Sealed,
                },
                Self::InstrumentUnknownMutant => ErrorCode {
                    code: "RM3001",
                    summary: "a candidate is not in the catalog being instrumented",
                    remedy: Some(
                        "this is a defect in this tool: the catalog and the instrumentation disagree about which mutants exist",
                    ),
                    sealed: Sealed,
                },
                Self::InstrumentSourceMismatch => ErrorCode {
                    code: "RM3002",
                    summary: "the source is not the one the candidates were discovered from",
                    remedy: Some(
                        "the file changed between being read and being instrumented; make sure nothing writes the tree while a run is preparing",
                    ),
                    sealed: Sealed,
                },
                Self::InstrumentSiteConflict => ErrorCode {
                    code: "RM3003",
                    summary: "two rewrite sites partially overlap, which a syntax tree cannot produce",
                    remedy: Some(
                        "this is a defect in this tool: two rules claimed overlapping bytes, which a syntax tree cannot produce",
                    ),
                    sealed: Sealed,
                },
                Self::InstrumentFlattenFailed => ErrorCode {
                    code: "RM3004",
                    summary: "an alternative could not be folded onto one line",
                    remedy: Some(
                        "this is a defect in this tool: a guard has to fit on the line it replaces, and this one did not",
                    ),
                    sealed: Sealed,
                },
                Self::InstrumentSpliceFailed => ErrorCode {
                    code: "RM3005",
                    summary: "the guards could not be applied to the file",
                    remedy: Some(
                        "this is a defect in this tool: the guards could not be written back over the file they were cut from",
                    ),
                    sealed: Sealed,
                },
                Self::InstrumentLinesMoved => ErrorCode {
                    code: "RM3006",
                    summary: "a guard would have moved a line",
                    remedy: Some(
                        "this is a defect in this tool: a guard moved a line, and every position a run reports is relative to lines that did not move",
                    ),
                    sealed: Sealed,
                },
                Self::InstrumentIndexReserved => ErrorCode {
                    code: "RM3007",
                    summary: "a mutant index makes the generated runtime's inclusive window overflow",
                    remedy: Some(
                        "this is a defect in this tool: the catalog outgrew the u32 window the generated runtime can represent",
                    ),
                    sealed: Sealed,
                },
                Self::InstrumentUnparsable => ErrorCode {
                    code: "RM3008",
                    summary: "the rewritten file does not read as Rust",
                    remedy: Some(
                        "this is a defect in this tool: a guard changed how the syntax around it reads; the line is named, and the source there is the case to report",
                    ),
                    sealed: Sealed,
                },
                Self::ValidateNotMutantInduced => ErrorCode {
                    code: "RM4001",
                    summary: "the tree does not compile before any mutant is live",
                    remedy: Some(
                        "make `cargo test --no-run` pass on the tree as committed, then run again",
                    ),
                    sealed: Sealed,
                },
                Self::ValidateNotIsolated => ErrorCode {
                    code: "RM4002",
                    summary: "the mutants a compilation failure came from could not be isolated",
                    remedy: Some(
                        "run `cargo test --no-run` on the tree yourself; the compilation failed for a reason this tool could not attribute to one mutant",
                    ),
                    sealed: Sealed,
                },
                Self::ValidateAttemptFailed => ErrorCode {
                    code: "RM4003",
                    summary: "an instrumented compilation could not be attempted at all",
                    remedy: Some(
                        "the compilation could not be started at all: check cargo runs on this tree and that there is room under TMPDIR",
                    ),
                    sealed: Sealed,
                },
                Self::SessionPristineBroken => ErrorCode {
                    code: "RM5001",
                    summary: "the workspace does not compile before anything is instrumented",
                    remedy: Some(
                        "make `cargo test --no-run` pass on the tree as committed, then run again",
                    ),
                    sealed: Sealed,
                },
                Self::SessionVerifyFailed => ErrorCode {
                    code: "RM5002",
                    summary: "the instrumented baseline fails a test the pristine tree passes",
                    remedy: Some(
                        "[execution] skip_targets leaves that target out; --no-verify makes every result a result about instrumentation",
                    ),
                    sealed: Sealed,
                },
                Self::SessionUnknownMutant => ErrorCode {
                    code: "RM5003",
                    summary: "no mutant of the catalog answers to the identity or prefix given",
                    remedy: Some(
                        "`rust-mutants catalog` lists what this run holds; an identity is re-minted whenever its file changes, so name the mutation by `path:item:rule` instead",
                    ),
                    sealed: Sealed,
                },
                Self::SessionUnknownTarget => ErrorCode {
                    code: "RM5004",
                    summary: "no test target of the session answers to the name given",
                    remedy: Some(
                        "`rust-mutants catalog` names every target this session built; a name no target has starts nothing",
                    ),
                    sealed: Sealed,
                },
                Self::SessionNoTargets => ErrorCode {
                    code: "RM5005",
                    summary: "the workspace builds no test target, so no mutant can be measured",
                    remedy: Some(
                        "the workspace has nothing that tests, so there is nothing a mutation could be put to; write a test, or point --root at the workspace that has them",
                    ),
                    sealed: Sealed,
                },
                Self::SessionWriteFailed => ErrorCode {
                    code: "RM5006",
                    summary: "the instrumented tree could not be written",
                    remedy: Some(
                        "check there is room under TMPDIR and that nothing is removing the run's directory while it writes",
                    ),
                    sealed: Sealed,
                },
                Self::SentinelUnwritable => ErrorCode {
                    code: "RM5007",
                    summary: "the crate planted for the routing layers could not be written",
                    remedy: Some(
                        "check the run's scratch directory is one this user may write in and that there is room under it",
                    ),
                    sealed: Sealed,
                },
                Self::SentinelOtherToolchain => ErrorCode {
                    code: "RM5008",
                    summary: "the crate planted for the routing layers was built by another compiler than the run's",
                    remedy: Some(
                        "the planted crate is built by the binaries in the run's own sysroot, so they answered with another version than the run's rustc: the toolchain directory is broken or mixed; reinstall it, or, where the run's rustc names no sysroot, make the cargo on PATH the one the tree resolves to",
                    ),
                    sealed: Sealed,
                },
                Self::SessionNotBeside => ErrorCode {
                    code: "RM5009",
                    summary: "a fault was asked to run beside something that is not a mutation, or what was named beside it is not a fault",
                    remedy: Some(
                        "a fault is put beside a mutation of the same session: name an `inject-error` fault beside a mutant of any other rule",
                    ),
                    sealed: Sealed,
                },
                Self::SessionNonceUnavailable => ErrorCode {
                    code: "RM5010",
                    summary: "the system gave no randomness for the nonce that ties a crash's notice to its execution",
                    remedy: Some(
                        "the operating system's random source failed; nothing the run could do stands in for it, so check the machine rather than the tree",
                    ),
                    sealed: Sealed,
                },
                Self::SessionApparatusChanged => ErrorCode {
                    code: "RM5011",
                    summary: "a mutant's execution changed the test executables the run starts, so no answer after it would be about the tests",
                    remedy: Some(
                        "run the mutant it names alone with `rust-mutants run --mutant <id> --jobs 1` to confirm, then keep its tests from writing where the test binaries live, or skip it with a reason",
                    ),
                    sealed: Sealed,
                },
                Self::CoverageUnreadable => ErrorCode {
                    code: "RM6001",
                    summary: "a coverage export that could not be read",
                    remedy: Some(
                        "run again without --coverage to measure without it, or check llvm-tools-preview is installed",
                    ),
                    sealed: Sealed,
                },
                Self::CoverageToolsMissing => ErrorCode {
                    code: "RM6002",
                    summary: "the LLVM tools the toolchain ships are not installed",
                    remedy: Some("rustup component add llvm-tools, or run with --no-coverage"),
                    sealed: Sealed,
                },
                Self::CoverageToolFailed => ErrorCode {
                    code: "RM6003",
                    summary: "llvm-profdata or llvm-cov failed",
                    remedy: Some(
                        "`rustup component add llvm-tools-preview`, and check the versions match the toolchain in use",
                    ),
                    sealed: Sealed,
                },
                Self::CoverageNothingWritten => ErrorCode {
                    code: "RM6004",
                    summary: "a test process wrote no coverage profile at all",
                    remedy: Some(
                        "the test process wrote no profile: check nothing in the suite sets LLVM_PROFILE_FILE for itself",
                    ),
                    sealed: Sealed,
                },
                Self::EquivalenceArtifactUnreadable => ErrorCode {
                    code: "RM7001",
                    summary: "an executable a successful build named could not be read back",
                    remedy: Some(
                        "run again after checking nothing removes or rewrites target files while the build is being measured",
                    ),
                    sealed: Sealed,
                },
                Self::RuleUnknown => ErrorCode {
                    code: "RM9001",
                    summary: "a rule name the canonical registry does not know",
                    remedy: Some("`rust-mutants rules` lists every rule this release knows"),
                    sealed: Sealed,
                },
                Self::GlobInvalid => ErrorCode {
                    code: "RM9002",
                    summary: "a pattern the caller gave is not a pattern",
                    remedy: Some(
                        "a pattern is workspace-relative with forward slashes: `src/**/*.rs`, never a leading or trailing slash",
                    ),
                    sealed: Sealed,
                },
                Self::DurationInvalid => ErrorCode {
                    code: "RM9003",
                    summary: "a duration the caller gave is not a duration",
                    remedy: Some("write a duration as 30s, 5m, or 1h30m"),
                    sealed: Sealed,
                },
            }
        }
    }
}

pub use table::{ErrorCode, RmCode};

/// A rule name the canonical registry does not know.
const RULE_UNKNOWN: ErrorCode = RmCode::RuleUnknown.error_code();

/// A pattern that is not a pattern.
const GLOB_INVALID: ErrorCode = RmCode::GlobInvalid.error_code();

/// A duration that is not a duration.
const DURATION_INVALID: ErrorCode = RmCode::DurationInvalid.error_code();

/// The caller cancelled before the operation finished, so nothing it saw says anything.
pub const INTERRUPTED: ErrorCode = RmCode::Interrupted.error_code();

/// A configuration file that could not be read.
/// The command line reports it; the ledger of `RM` codes is one, so it lives here.
pub const CONFIG_UNREADABLE: ErrorCode = RmCode::ConfigUnreadable.error_code();

/// A configuration file that is not the document this version understands.
pub const CONFIG_UNPARSABLE: ErrorCode = RmCode::ConfigUnparsable.error_code();

/// A configuration that parses but says something a run cannot honour.
pub const CONFIG_INVALID: ErrorCode = RmCode::ConfigInvalid.error_code();

/// A configuration whose `version` is not one this release understands.
pub const CONFIG_UNSUPPORTED_VERSION: ErrorCode = RmCode::ConfigUnsupportedVersion.error_code();

/// A process environment that already selects a mutant.
pub const ENVIRONMENT_RESERVED: ErrorCode = RmCode::EnvironmentReserved.error_code();

/// A stored run report that is not there or cannot be read.
pub const REPORT_MISSING: ErrorCode = RmCode::ReportMissing.error_code();

/// A file a command would write that is already there.
pub const FILE_EXISTS: ErrorCode = RmCode::FileExists.error_code();

/// A directory a command has to write to and could not.
pub const WRITE_FAILED: ErrorCode = RmCode::WriteFailed.error_code();

/// A coverage export that could not be read.
pub const COVERAGE_UNREADABLE: ErrorCode = RmCode::CoverageUnreadable.error_code();

/// The LLVM tools the toolchain ships, not installed.
pub const COVERAGE_TOOLS_MISSING: ErrorCode = RmCode::CoverageToolsMissing.error_code();

/// One of the LLVM tools failed.
pub const COVERAGE_TOOL_FAILED: ErrorCode = RmCode::CoverageToolFailed.error_code();

/// A test process wrote no coverage profile at all.
pub const COVERAGE_NOTHING_WRITTEN: ErrorCode = RmCode::CoverageNothingWritten.error_code();

/// An executable a successful build named could not be read back for equivalence comparison.
pub const EQUIVALENCE_ARTIFACT_UNREADABLE: ErrorCode =
    RmCode::EquivalenceArtifactUnreadable.error_code();

/// A change set that git could not be asked for.
pub const CHANGE_SET_UNAVAILABLE: ErrorCode = RmCode::ChangeSetUnavailable.error_code();

/// Reports that are not the parts of one whole.
pub const MERGE_REFUSED: ErrorCode = RmCode::MergeRefused.error_code();

/// A source a report names that the tree does not hold.
pub const SOURCE_UNREADABLE: ErrorCode = RmCode::SourceUnreadable.error_code();

/// An outcome cache could not be enumerated completely.
pub const CACHE_UNREADABLE: ErrorCode = RmCode::CacheUnreadable.error_code();

/// A continuous integration host a command was asked to write for, which the environment does not provide.
pub const CI_HOST_UNAVAILABLE: ErrorCode = RmCode::CiHostUnavailable.error_code();

/// A workspace root outside the checkout a host places annotations in.
pub const CI_ROOT_OUTSIDE_CHECKOUT: ErrorCode = RmCode::CiRootOutsideCheckout.error_code();

/// A file the host named for a step's summary or outputs, which could not be appended to.
pub const CI_SINK_UNWRITABLE: ErrorCode = RmCode::CiSinkUnwritable.error_code();

pub(crate) const SNAPSHOT_INVALID_OPTIONS: ErrorCode = RmCode::SnapshotInvalidOptions.error_code();
pub(crate) const SNAPSHOT_SOURCE_ROOT: ErrorCode = RmCode::SnapshotSourceRoot.error_code();
pub(crate) const SNAPSHOT_WALK: ErrorCode = RmCode::SnapshotWalk.error_code();
pub(crate) const SNAPSHOT_SYMLINK: ErrorCode = RmCode::SnapshotSymlink.error_code();
pub(crate) const SNAPSHOT_REPARSE_POINT: ErrorCode = RmCode::SnapshotReparsePoint.error_code();
pub(crate) const SNAPSHOT_IRREGULAR: ErrorCode = RmCode::SnapshotIrregular.error_code();
pub(crate) const SNAPSHOT_UNSUPPORTED_NAME: ErrorCode =
    RmCode::SnapshotUnsupportedName.error_code();
pub(crate) const SNAPSHOT_DESTINATION: ErrorCode = RmCode::SnapshotDestination.error_code();
pub(crate) const SNAPSHOT_COPY: ErrorCode = RmCode::SnapshotCopy.error_code();
pub(crate) const SNAPSHOT_CLEANUP_REFUSED: ErrorCode = RmCode::SnapshotCleanupRefused.error_code();
pub(crate) const SNAPSHOT_CLEANUP_FAILED: ErrorCode = RmCode::SnapshotCleanupFailed.error_code();
pub(crate) const CARGO_TOOLCHAIN_NOT_FOUND: ErrorCode = RmCode::CargoToolchainNotFound.error_code();
pub(crate) const CARGO_VERSION_UNREADABLE: ErrorCode = RmCode::CargoVersionUnreadable.error_code();
pub(crate) const CARGO_COMMAND_FAILED: ErrorCode = RmCode::CargoCommandFailed.error_code();
pub(crate) const CARGO_METADATA_UNPARSABLE: ErrorCode =
    RmCode::CargoMetadataUnparsable.error_code();
pub(crate) const CARGO_MESSAGE_UNPARSABLE: ErrorCode = RmCode::CargoMessageUnparsable.error_code();
pub(crate) const WORKSPACE_REACHES_OUTSIDE: ErrorCode =
    RmCode::WorkspaceReachesOutside.error_code();
pub(crate) const ROOT_IS_NOT_THE_WORKSPACE: ErrorCode = RmCode::RootIsNotTheWorkspace.error_code();
pub(crate) const SNAPSHOT_LAYOUT: ErrorCode = RmCode::SnapshotLayout.error_code();
pub(crate) const MANIFEST_UNREADABLE: ErrorCode = RmCode::ManifestUnreadable.error_code();
pub(crate) const CAPDIR_NAME_REFUSED: ErrorCode = RmCode::CapdirNameRefused.error_code();
pub(crate) const BUILD_LEDGER_UNREADABLE: ErrorCode = RmCode::BuildLedgerUnreadable.error_code();
pub(crate) const TESTS_TOOLCHAIN_UNREACHABLE: ErrorCode =
    RmCode::TestsToolchainUnreachable.error_code();
pub(crate) const DEP_INFO_UNREADABLE: ErrorCode = RmCode::DepInfoUnreadable.error_code();
pub(crate) const DEP_INFO_MISSING: ErrorCode = RmCode::DepInfoMissing.error_code();
pub(crate) const DISCOVER_FILE_UNREADABLE: ErrorCode = RmCode::DiscoverFileUnreadable.error_code();
pub(crate) const DISCOVER_PARSE_FAILED: ErrorCode = RmCode::DiscoverParseFailed.error_code();
pub(crate) const DISCOVER_OUTSIDE_ROOT: ErrorCode = RmCode::DiscoverOutsideRoot.error_code();
pub(crate) const DISCOVER_CATALOG_FAILED: ErrorCode = RmCode::DiscoverCatalogFailed.error_code();

/// A discovered candidate broke an identity invariant before it could be displayed.
pub const CANDIDATE_INVALID: ErrorCode = DISCOVER_CATALOG_FAILED;
pub(crate) const DISCOVER_UNKNOWN_PACKAGE: ErrorCode = RmCode::DiscoverUnknownPackage.error_code();
pub(crate) const DISCOVER_ANNOTATION_WITHOUT_REASON: ErrorCode =
    RmCode::DiscoverAnnotationWithoutReason.error_code();
pub(crate) const DISCOVER_UNKNOWN_ANNOTATION: ErrorCode =
    RmCode::DiscoverUnknownAnnotation.error_code();
pub(crate) const INSTRUMENT_UNKNOWN_MUTANT: ErrorCode =
    RmCode::InstrumentUnknownMutant.error_code();
pub(crate) const INSTRUMENT_SOURCE_MISMATCH: ErrorCode =
    RmCode::InstrumentSourceMismatch.error_code();
pub(crate) const INSTRUMENT_SITE_CONFLICT: ErrorCode = RmCode::InstrumentSiteConflict.error_code();
pub(crate) const INSTRUMENT_FLATTEN_FAILED: ErrorCode =
    RmCode::InstrumentFlattenFailed.error_code();
pub(crate) const INSTRUMENT_SPLICE_FAILED: ErrorCode = RmCode::InstrumentSpliceFailed.error_code();
pub(crate) const INSTRUMENT_LINES_MOVED: ErrorCode = RmCode::InstrumentLinesMoved.error_code();
pub(crate) const INSTRUMENT_INDEX_RESERVED: ErrorCode =
    RmCode::InstrumentIndexReserved.error_code();
pub(crate) const INSTRUMENT_UNPARSABLE: ErrorCode = RmCode::InstrumentUnparsable.error_code();
pub(crate) const VALIDATE_NOT_MUTANT_INDUCED: ErrorCode =
    RmCode::ValidateNotMutantInduced.error_code();
pub(crate) const VALIDATE_NOT_ISOLATED: ErrorCode = RmCode::ValidateNotIsolated.error_code();
pub(crate) const VALIDATE_ATTEMPT_FAILED: ErrorCode = RmCode::ValidateAttemptFailed.error_code();
pub(crate) const SESSION_PRISTINE_BROKEN: ErrorCode = RmCode::SessionPristineBroken.error_code();
pub(crate) const SESSION_VERIFY_FAILED: ErrorCode = RmCode::SessionVerifyFailed.error_code();
pub(crate) const SESSION_UNKNOWN_MUTANT: ErrorCode = RmCode::SessionUnknownMutant.error_code();
pub(crate) const SESSION_UNKNOWN_TARGET: ErrorCode = RmCode::SessionUnknownTarget.error_code();
pub(crate) const SESSION_NO_TARGETS: ErrorCode = RmCode::SessionNoTargets.error_code();
pub(crate) const SESSION_WRITE_FAILED: ErrorCode = RmCode::SessionWriteFailed.error_code();
pub(crate) const SESSION_APPARATUS_CHANGED: ErrorCode =
    RmCode::SessionApparatusChanged.error_code();
pub(crate) const SENTINEL_UNWRITABLE: ErrorCode = RmCode::SentinelUnwritable.error_code();
pub(crate) const SENTINEL_OTHER_TOOLCHAIN: ErrorCode = RmCode::SentinelOtherToolchain.error_code();
pub(crate) const SESSION_NOT_BESIDE: ErrorCode = RmCode::SessionNotBeside.error_code();
pub(crate) const SESSION_NONCE_UNAVAILABLE: ErrorCode =
    RmCode::SessionNonceUnavailable.error_code();

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
    /// The crate planted for the routing layers could not be put where a session can open it.
    #[error(transparent)]
    Sentinel(#[from] crate::sentinel::SentinelError),
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
            Self::Sentinel(error) => error.code(),
        }
    }
}

/// Every code the rust-mutants product reports, the engine's and the command line's alike, in code order.
#[must_use]
pub const fn error_codes() -> &'static [ErrorCode] {
    &ERROR_CODES
}

/// Every code, made once from [`RmCode::ALL`].
#[expect(
    clippy::indexing_slicing,
    reason = "a const loop cannot iterate, and the bound is the length of the array it indexes"
)]
const ERROR_CODES: [ErrorCode; RmCode::ALL.len()] = {
    let mut codes = [RmCode::ALL[0].error_code(); RmCode::ALL.len()];
    let mut at = 0;
    while at < codes.len() {
        codes[at] = RmCode::ALL[at].error_code();
        at += 1;
    }
    codes
};
