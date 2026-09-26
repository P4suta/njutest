// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The public entry point: a read-only source tree, copied.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cargo::{Driver, LocateOptions, Metadata, MetadataOptions, Toolchain};
use crate::error::{self, ErrorCode};
use crate::glob::Pattern;
use crate::runner::Cancel;
use crate::session::{PrepareOptions, Session, prepare};
use crate::snapshot::{self, DIR_PREFIX, Options as SnapshotOptions, Snapshot};
use crate::tempowner::{self, SweepResult};
use crate::trace::{OpenRecord, Recorder, SnapshotRecord, SweepRecord};

/// Where the engine puts the target directory of a run, under the temporary root: one per source root, so successive runs share cargo's incremental state.
pub const TARGET_DIR_PREFIX: &str = "rust-mutants-target-";

/// The schema a target directory's owner marker names, so a reader can tell a build cache from a run's scratch tree.
pub const TARGET_OWNER_SCHEMA: &str = "rust-mutants-target-owner-v1";

/// Where a run's test processes work, under the temporary root: one per run, beside the target directory rather than inside it.
pub const SCRATCH_DIR_PREFIX: &str = "rm-scratch-";

/// How many scratch directories the engine will look at before naming one after the process instead.
const SCRATCH_ATTEMPTS: u32 = 1024;

/// The schema a scratch directory's owner marker names, so a reader can tell a run's working area from a build cache.
pub const SCRATCH_OWNER_SCHEMA: &str = "rust-mutants-scratch-owner-v1";

/// One physical spelling of the existing directory that owns every disposable artifact of a workspace.
///
/// Cargo canonicalizes paths in compiler and metadata messages.
/// Resolving the temporary root once at the input boundary keeps the snapshot, build cache,
/// scratch directory, and those emitted paths in the same identity domain.
#[derive(Debug)]
struct TemporaryRoot {
    path: PathBuf,
}

impl TemporaryRoot {
    fn open(path: &Path) -> Result<Self, SessionError> {
        crate::canonical::canonical(path)
            .map(|path| Self { path })
            .map_err(|source| SessionError::TemporaryRootUnavailable {
                path: path.to_path_buf(),
                source,
            })
    }

    fn path(&self) -> &Path {
        self.path.as_path()
    }
}

/// Every prefix the engine names a temporary directory with, which is what a sweep collects.
pub const SWEPT_PREFIXES: [&str; 3] = [DIR_PREFIX, TARGET_DIR_PREFIX, SCRATCH_DIR_PREFIX];

/// The part of a stable name that identifies the source root.
fn keyed(root: &Path) -> String {
    snapshot::stable_key(root)
}

/// Where cargo builds a run against `root`, under `parent`.
#[must_use]
pub fn target_of(parent: &Path, root: &Path) -> PathBuf {
    parent.join(format!("{TARGET_DIR_PREFIX}{}", keyed(root)))
}

/// The `at`th scratch directory under `parent`, where a run's test processes work.
#[must_use]
pub fn scratch_of(parent: &Path, at: u32) -> PathBuf {
    parent.join(format!("{SCRATCH_DIR_PREFIX}{at}"))
}

/// Takes the lowest-numbered scratch directory no other run holds, so the name stays short however many runs share a temporary root.
///
/// A claim is what keeps the next run's sweep off this one's working directory, so a run that could not take one is refused rather than started: the sweep would remove the directory the test processes are working in, every mutation in flight would come back killed, and the run would report that as what the tests establish.
///
/// # Errors
/// The scratch directory could not be created or claimed under any of its names.
fn claim_scratch(
    parent: &Path,
    now: jiff::Timestamp,
) -> Result<(PathBuf, tempowner::Owner), SessionError> {
    for at in 0..SCRATCH_ATTEMPTS {
        let dir = scratch_of(parent, at);
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        match tempowner::claim_as(&dir, now, SCRATCH_OWNER_SCHEMA) {
            Ok(owner) => match owner.empty() {
                Ok(()) => return Ok((dir, owner)),
                Err(_left_by_a_dead_run_and_not_removable) => drop(owner),
            },
            Err(_already_claimed_or_unusable) => {}
        }
    }
    let dir = parent.join(format!("{SCRATCH_DIR_PREFIX}p{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|source| SessionError::ScratchCreateFailed {
        path: dir.clone(),
        source,
    })?;
    tempowner::claim_as(&dir, now, SCRATCH_OWNER_SCHEMA)
        .map(|owner| (dir.clone(), owner))
        .map_err(|source| SessionError::ScratchUnclaimed {
            path: dir,
            source: std::io::Error::other(source),
        })
}

/// Configures [`Workspace::open`].
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// The cargo to use: a path, or a bare name to find on `search_path`.
    pub cargo: Option<PathBuf>,
    /// The `PATH` a bare cargo name is searched on.
    /// The composition root reads the process environment; the engine never does.
    pub search_path: Option<OsString>,
    /// The complete environment every command and test process runs with.
    pub env: Vec<(OsString, OsString)>,
    /// The existing absolute directory snapshots and target directories are created in.
    /// An argument for the same reason `env` is; the composition root creates and names the operating system's temporary directory, and this layer binds its physical identity before minting any child path.
    pub temp_directory: PathBuf,
    /// The configured report directory as a source-root-relative path, so the snapshot excludes it.
    pub report_directory: Option<String>,
    /// Patterns removing paths from the snapshot entirely.
    pub exclude: Vec<Pattern>,
    /// Preserve the snapshot and the target directory instead of removing them, for a person who has to look at what a run produced.
    pub keep_temp: bool,
    /// Pass `--offline` to every cargo command.
    pub offline: bool,
    /// Pass `--locked` to every cargo command.
    pub locked: bool,
    /// Directories outside the root the workspace may read code from, each copied into the snapshot at the position it has relative to the root.
    pub allow_outside: Vec<PathBuf>,
    /// Where the run records what it did.
    /// [`Recorder::disabled`] by default.
    pub trace: Recorder,
}

/// `path` as a `/`-separated path under `root`, or nothing when it is not under it.
fn within(root: &Path, path: &Path) -> Result<Option<String>, crate::id::SlashedPathError> {
    let resolved = match crate::canonical::canonical(path) {
        Ok(resolved) => resolved,
        Err(_target_directory_does_not_exist_yet) => path.to_path_buf(),
    };
    let Ok(relative) = resolved.strip_prefix(root) else {
        return Ok(None);
    };
    let named = crate::id::slashed(relative)?;
    Ok((!named.is_empty()).then_some(named))
}

/// Whether a file of the copy lies under a member's directory, where a member whose directory cannot be named holds every file.
fn member_holds(under: Option<&str>, rel_path: &str) -> bool {
    match under {
        None | Some("") => true,
        Some(dir) => rel_path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/')),
    }
}

/// The trace note saying which processes a test left running outside its execution were ended when the run closed.
const ESCAPED_PROCESSES: &str = "escaped-processes";

/// Claims the build cache for the life of this workspace, so a sweep elsewhere leaves it alone while cargo is writing into it.
/// A cache that cannot be claimed is one another run is already using, which is not this run's business and not a reason to fail: cargo takes its own lock.
fn claim_target(dir: &Path, now: jiff::Timestamp, root: &Path) -> Option<tempowner::Owner> {
    if std::fs::create_dir_all(dir).is_err() {
        return None;
    }
    match tempowner::claim_cache_of(dir, now, TARGET_OWNER_SCHEMA, root) {
        Ok(owner) => Some(owner),
        Err(_) => None,
    }
}

/// The name of the directory beside the copy a process that lost the run's environment says so in.
const WATCHED_NAME: &str = "watched";

/// A read-only source tree and the disposable copy of it this run works in.
#[derive(Debug)]
pub struct Workspace {
    pub(crate) snapshot: Snapshot,
    /// The rules the snapshot was copied under, which a survey of the source must follow to say what a copy would hold.
    pub(crate) rules: SnapshotOptions,
    /// The directory a process of the instrumented tree that lost the run's environment says so in: beside the copy, which no other run shares.
    pub(crate) watched: String,
    pub(crate) toolchain: Toolchain,
    pub(crate) metadata: Metadata,
    pub(crate) target_dir: PathBuf,
    /// The claim on that directory: held for the life of the workspace so a concurrent sweep leaves it alone, and released without removing anything.
    pub(crate) target_owner: Option<tempowner::Owner>,
    /// Where this run's test processes work: a sibling of the target directory, not a child, because its name has to stay inside `sun_path`.
    pub(crate) scratch_dir: PathBuf,
    /// The claim on that directory, held and released exactly as [`Workspace::target_owner`] is.
    /// The claim on the scratch directory, taken when the run opened and `None` only once it has been released or kept.
    pub(crate) scratch_owner: Option<tempowner::Owner>,
    pub(crate) base_env: Vec<(OsString, OsString)>,
    pub(crate) swept: SweepResult,
    pub(crate) keep_temp: bool,
    pub(crate) offline: bool,
    pub(crate) locked: bool,
    pub(crate) trace: Recorder,
}

/// Why the workspace layer could not do what it was asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// The tree does not compile before anything was instrumented.
    #[error("{}: the workspace does not compile before anything is instrumented: {first}", error::SESSION_PRISTINE_BROKEN.code)]
    PristineBroken {
        /// The first error the compiler reported, rendered.
        first: String,
    },
    /// The workspace reads code from outside itself, which the copy does not hold.
    #[error(
        "{}: {name} is read from {}, which is outside {} and so is not in the copy a run \
         measures. {} declares it. Allow it with --allow-outside, or vendor it inside the tree",
        error::WORKSPACE_REACHES_OUTSIDE.code,
        path.display(),
        root.display(),
        manifest.display()
    )]
    ReachesOutside {
        /// The dependency or patched crate.
        name: String,
        /// The manifest that declares it.
        manifest: PathBuf,
        /// Where it reads it from.
        path: PathBuf,
        /// The tree the run was given.
        root: PathBuf,
    },
    /// The root names a member of a workspace rather than the workspace.
    #[error(
        "{}: {} is a member of the workspace at {}, and a member on its own is not a tree a \
         run can build. Pass --root {}",
        error::ROOT_IS_NOT_THE_WORKSPACE.code,
        root.display(),
        workspace_root.display(),
        workspace_root.display()
    )]
    RootIsNotTheWorkspace {
        /// The tree the run was given.
        root: PathBuf,
        /// The workspace it belongs to.
        workspace_root: PathBuf,
    },
    /// The instrumented baseline does not pass its own tests.
    #[error(
        "{}: {} fails with nothing active, so no outcome under a mutation would be about the \
         mutation. Fix the test, leave the target out with {}, or pass --no-verify and read \
         every result as being about the instrumentation as much as about the mutation:\n{output}",
        error::SESSION_VERIFY_FAILED.code,
        targets.join(", "),
        targets
            .iter()
            .map(|target| format!("--skip-target {target}"))
            .collect::<Vec<String>>()
            .join(" ")
    )]
    VerifyFailed {
        /// Every target that failed, in identity order.
        targets: Vec<String>,
        /// The tail of what the first of them said.
        output: String,
    },
    /// No prefix of the catalog matches, or several do.
    #[error("{}: {message}", error::SESSION_UNKNOWN_MUTANT.code)]
    UnknownMutant {
        /// What was wrong with the prefix.
        message: String,
    },
    /// A fault was asked to run beside something that is not a mutation, or what was named beside it is not a fault.
    #[error("{}: {fault} cannot run beside {mutant}: {why}", error::SESSION_NOT_BESIDE.code)]
    NotBeside {
        /// What was asked to run.
        mutant: String,
        /// What was named beside it.
        fault: String,
        /// Which half is not what it has to be.
        why: &'static str,
    },
    /// The named target is not one this session built.
    #[error(
        "{}: no test target is named {name:?}; this session built {}",
        error::SESSION_UNKNOWN_TARGET.code,
        if available.is_empty() { String::from("none") } else { available.join(", ") }
    )]
    UnknownTarget {
        /// The name that was asked for.
        name: String,
        /// The targets there are, which is what a reader has to choose between.
        available: Vec<String>,
    },
    /// A target a run was told to leave out is not one the workspace declares.
    #[error(
        "{}: no test target is named {name:?}, so leaving it out leaves nothing out; this \
         workspace declares {}",
        error::SESSION_UNKNOWN_TARGET.code,
        if available.is_empty() { String::from("none") } else { available.join(", ") }
    )]
    SkippedTargetUnknown {
        /// The name that was asked to be left out.
        name: String,
        /// Every target the workspace declares, which is more than a narrowed run builds.
        available: Vec<String>,
    },
    /// The workspace has no test target at all, so nothing can be measured.
    #[error(
        "{}: the workspace builds no test target, so no mutant can be measured; {} {} selected",
        error::SESSION_NO_TARGETS.code,
        if packages.is_empty() { String::from("every package was") } else { packages.join(", ") },
        if packages.len() == 1 { "was" } else { "were" }
    )]
    NoTargets {
        /// The packages the run was about, which is where a reader looks for a test to write.
        packages: Vec<String>,
    },
    /// A mutant's execution changed the test executables the run starts.
    #[error(
        "{}: the test executables changed while {mutant} ran{} ({}); every answer after it would be read against a harness this run did not build, so the run stops here",
        error::SESSION_APPARATUS_CHANGED.code,
        crate::apparatus::alongside(beside),
        crate::apparatus::summary(changes)
    )]
    ApparatusChanged {
        /// The mutant whose execution the change was found after.
        mutant: String,
        /// Every other mutant whose execution ran at any moment that one did, any of which may have made the change.
        beside: Vec<String>,
        /// What changed.
        changes: Vec<crate::apparatus::Change>,
    },
    /// The instrumented tree could not be written.
    #[error("{}: cannot write {path} into the snapshot: {source}", error::SESSION_WRITE_FAILED.code)]
    WriteFailed {
        /// The workspace-relative path.
        path: String,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
    /// The temporary root could not be bound to one physical directory spelling before snapshot paths were minted.
    #[error(
        "{}: cannot resolve temporary root {} to one physical directory: {source}",
        error::SESSION_WRITE_FAILED.code,
        path.display()
    )]
    TemporaryRootUnavailable {
        /// The configured temporary root.
        path: PathBuf,
        /// Why its physical identity could not be established.
        #[source]
        source: std::io::Error,
    },
    /// A source selected from the catalog was absent from the immutable plan.
    #[error(
        "{}: {path} is in the catalog but absent from the source plan",
        error::INSTRUMENT_SOURCE_MISMATCH.code
    )]
    SelectionSourceMissing {
        /// The workspace-relative source path.
        path: String,
    },
    /// A source changed from valid Rust text into non-UTF-8 bytes before planning.
    #[error(
        "{}: {path} is not valid UTF-8 while selecting catalog entries: {source}",
        error::INSTRUMENT_SOURCE_MISMATCH.code
    )]
    SelectionSourceNotUtf8 {
        /// The workspace-relative source path.
        path: String,
        /// The encoding failure.
        #[source]
        source: std::str::Utf8Error,
    },
    /// A selected source could not carry an exact line or column.
    #[error(
        "{}: {path} cannot be indexed exactly while selecting catalog entries: {source}",
        error::INSTRUMENT_SOURCE_MISMATCH.code
    )]
    SelectionPositionInvalid {
        /// The workspace-relative source path.
        path: String,
        /// The exact position invariant that failed.
        #[source]
        source: crate::syntax::PositionError,
    },
    /// A file used to bind retained build evidence was outside the copied tree.
    #[error(
        "{}: evidence path {} is outside copied tree {}",
        error::INSTRUMENT_SOURCE_MISMATCH.code,
        path.display(),
        root.display()
    )]
    EvidencePathOutside {
        /// The path emitted by the compiler or metadata.
        path: PathBuf,
        /// The copied tree it was required to belong to.
        root: PathBuf,
    },
    /// A file used to bind retained build evidence had no exact UTF-8 relative spelling.
    #[error(
        "{}: evidence path {} has no exact UTF-8 spelling",
        error::INSTRUMENT_SOURCE_MISMATCH.code,
        path.display()
    )]
    EvidencePathNotUtf8 {
        /// The unrepresentable path.
        path: PathBuf,
    },
    /// A file used to bind retained build evidence had a noncanonical relative path.
    #[error(
        "{}: evidence path {} is not canonical: {source}",
        error::INSTRUMENT_SOURCE_MISMATCH.code,
        path.display()
    )]
    EvidencePathInvalid {
        /// The rejected path.
        path: PathBuf,
        /// Why it is not canonical.
        #[source]
        source: crate::id::PathError,
    },
    /// A file used to bind retained build evidence could not be read exactly.
    #[error(
        "{}: cannot read evidence file {}: {source}",
        error::INSTRUMENT_SOURCE_MISMATCH.code,
        path.display()
    )]
    EvidenceReadFailed {
        /// The file that could not be retained in the evidence digest.
        path: PathBuf,
        /// The filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The session's execution-scratch allocator was poisoned by a panic while holding its state.
    #[error(
        "{}: the execution scratch allocator is poisoned, so no fresh process directory can be proved",
        error::SESSION_WRITE_FAILED.code
    )]
    ScratchStatePoisoned,
    /// The filtered-test routing cache was poisoned by a panic while holding its state.
    #[error(
        "{}: the filtered-test routing state is poisoned, so cached routing facts cannot be trusted",
        error::SESSION_WRITE_FAILED.code
    )]
    RoutingStatePoisoned,
    /// What the carry rule took of the tree was poisoned by a panic while holding it.
    #[error(
        "{}: the carry rule's record of which targets held their reach is poisoned, so it cannot be trusted",
        error::SESSION_WRITE_FAILED.code
    )]
    CarryStatePoisoned,
    /// A filtered-test establishment named more tests than the durable counter can represent.
    #[error(
        "{}: one filtered-test establishment named {count} tests, which exceeds the routing counter",
        error::SESSION_WRITE_FAILED.code
    )]
    RoutingCountTooLarge {
        /// The unrepresentable test count.
        count: usize,
    },
    /// An item index of a dense catalog names no item, which says the catalog was not built the way its numbering assumes.
    #[error(
        "{}: item {index} of the item catalog names no item",
        error::SESSION_WRITE_FAILED.code
    )]
    ItemCatalogGap {
        /// The index that named nothing.
        index: u32,
    },
    /// One expectation resolved to more mutants than its durable counter can represent.
    #[error(
        "{}: one expectation resolved to {count} mutants, which exceeds its durable counter",
        error::SESSION_WRITE_FAILED.code
    )]
    ExpectationCoverageTooLarge {
        /// The unrepresentable number of resolved mutants.
        count: usize,
    },
    /// Two expectations name one mutation, which then has two reasons.
    #[error(
        "{}: the expectations {first:?} and {second:?} both name mutation {mutant}; a mutant has one reason",
        error::CONFIG_INVALID.code
    )]
    ExpectationsOverlap {
        /// The expectation that named it first.
        first: String,
        /// The one that named it again.
        second: String,
        /// The mutation both name, with its line.
        mutant: String,
    },
    /// A run collection contains more rows than its durable counter can represent.
    #[error(
        "{}: a run collection contains {count} rows, which exceeds its durable counter",
        error::SESSION_WRITE_FAILED.code
    )]
    RunCountTooLarge {
        /// The exact unrepresentable host count.
        count: usize,
    },
    /// Folding one run's rows exhausted a durable counter.
    #[error(
        "{}: a run accounting counter overflowed",
        error::SESSION_WRITE_FAILED.code
    )]
    RunCountOverflow,
    /// The exact count of filtered tests started no longer fits its durable counter.
    #[error(
        "{}: the filtered-test routing counter is exhausted",
        error::SESSION_WRITE_FAILED.code
    )]
    RoutingCountExhausted,
    /// A filtered-test answer was replaced even though its state lock was held from lookup through insertion.
    #[error(
        "{}: a filtered-test routing answer changed during one locked establishment",
        error::SESSION_WRITE_FAILED.code
    )]
    RoutingAnswerAlreadyEstablished,
    /// A baseline duration cannot be multiplied into a finite derived mutation timeout.
    #[error(
        "{}: baseline duration {baseline:?} is too large to derive a mutation timeout",
        error::SESSION_WRITE_FAILED.code
    )]
    DerivedTimeoutOverflow {
        /// The measured baseline that could not be multiplied exactly.
        baseline: Duration,
    },
    /// The total duration of all executions cannot be represented exactly.
    #[error(
        "{}: the combined mutation execution duration overflowed",
        error::SESSION_WRITE_FAILED.code
    )]
    ExecutionDurationOverflow,
    /// A duration does not fit the millisecond field used by the trace wire.
    #[error(
        "{}: duration {duration:?} does not fit the trace millisecond field",
        error::SESSION_WRITE_FAILED.code
    )]
    DurationMillisOverflow {
        /// The duration that could not be represented exactly.
        duration: Duration,
    },
    /// A collection count does not fit the trace wire's integer field.
    #[error(
        "{}: {subject} count {count} does not fit the trace wire",
        error::SESSION_WRITE_FAILED.code
    )]
    TraceCountTooLarge {
        /// What was being counted.
        subject: &'static str,
        /// The exact unrepresentable count.
        count: usize,
    },
    /// Two baseline rows claimed the same target identity.
    #[error(
        "{}: baseline target {target:?} was recorded more than once",
        error::SESSION_WRITE_FAILED.code
    )]
    DuplicateBaselineTarget {
        /// The duplicated stable target identity.
        target: String,
    },
    /// The exact byte total of a snapshot cannot be represented.
    #[error(
        "{}: snapshot byte accounting overflowed",
        error::SESSION_WRITE_FAILED.code
    )]
    SnapshotBytesOverflow,
    /// A workspace-owned resource could not be released or removed.
    #[error(
        "{}: cannot {operation} {}: {source}",
        error::SESSION_WRITE_FAILED.code,
        path.display()
    )]
    CleanupFailed {
        /// The cleanup transition that refused.
        operation: &'static str,
        /// The resource that remained.
        path: PathBuf,
        /// The operating-system or ownership failure.
        #[source]
        source: std::io::Error,
    },
    /// A mutation execution panicked while holding the run's shared/exclusive coordination lock.
    #[error(
        "{}: mutation execution coordination is poisoned, so isolation can no longer be proved",
        error::SESSION_WRITE_FAILED.code
    )]
    CoordinationPoisoned,
    /// The worker allocator was poisoned by a panic while a work item was being claimed.
    #[error(
        "{}: the mutation worker state is poisoned, so ownership of the remaining work is unknown",
        error::SESSION_WRITE_FAILED.code
    )]
    WorkerStatePoisoned,
    /// The finite worker delivery queue could not be sized without overflow.
    #[error(
        "{}: {workers} mutation workers require an unrepresentable delivery queue",
        error::SESSION_WRITE_FAILED.code
    )]
    WorkerQueueTooLarge {
        /// How many workers were requested.
        workers: usize,
    },
    /// A mutation worker thread could not be started.
    #[error(
        "{}: cannot start mutation worker {worker}: {source}",
        error::SESSION_WRITE_FAILED.code
    )]
    WorkerStartFailed {
        /// The stable zero-based worker ordinal.
        worker: usize,
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// A mutation worker panicked before its owner joined it.
    #[error(
        "{}: a mutation worker panicked before it could be joined",
        error::SESSION_WRITE_FAILED.code
    )]
    WorkerPanicked,
    /// More completed work was delivered than the progress counter can represent.
    #[error(
        "{}: the completed-mutation progress counter is exhausted",
        error::SESSION_WRITE_FAILED.code
    )]
    CompletedCountExhausted,
    /// The session has exhausted the names available for fresh execution scratch directories.
    #[error(
        "{}: the execution scratch sequence is exhausted",
        error::SESSION_WRITE_FAILED.code
    )]
    ScratchSequenceExhausted,
    /// A fresh execution scratch directory could not be claimed, so the next run's sweep would remove the one this run is working in.
    #[error(
        "{}: cannot claim the execution scratch directory {}: {source}",
        error::SESSION_WRITE_FAILED.code,
        path.display()
    )]
    ScratchUnclaimed {
        /// The directory no claim could be taken on.
        path: PathBuf,
        /// Why the claim failed.
        #[source]
        source: std::io::Error,
    },
    /// A run's scratch directory could not be walked for what it left.
    #[error(
        "{}: cannot read what a run left in its scratch directory {}: {source}",
        error::SESSION_WRITE_FAILED.code,
        path.display()
    )]
    ScratchUnreadable {
        /// The directory, or the entry of it, that could not be read.
        path: PathBuf,
        /// What reading it said.
        source: std::io::Error,
    },
    /// The system gave no randomness for the nonce that ties a crash's notice to its execution.
    #[error(
        "{}: cannot draw the nonce a crash's notice must carry: {error}",
        error::SESSION_NONCE_UNAVAILABLE.code
    )]
    CrashNonceUnavailable {
        /// What the system said.
        error: getrandom::Error,
    },
    /// A fresh execution scratch directory could not be created exclusively.
    #[error(
        "{}: cannot reserve the fresh execution scratch directory {}: {source}",
        error::SESSION_WRITE_FAILED.code,
        path.display()
    )]
    ScratchCreateFailed {
        /// The exact directory whose exclusive creation failed.
        path: PathBuf,
        /// The filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// A workspace-relative path cannot be recorded exactly in the UTF-8 catalog representation.
    #[error("{}: {source}", error::SESSION_WRITE_FAILED.code)]
    WorkspacePathNotUtf8 {
        /// The exact platform path that cannot cross the UTF-8 boundary.
        #[from]
        source: crate::id::SlashedPathError,
    },
    /// Mutation source bytes cannot cross the catalog's UTF-8 wire boundary without changing their value.
    #[error(
        "{}: mutation {mutant} has non-UTF-8 {field} bytes and cannot be written to the catalog: {source}",
        error::SESSION_WRITE_FAILED.code
    )]
    CatalogTextNotUtf8 {
        /// The stable mutation identity whose bytes were not text.
        mutant: String,
        /// Which catalog field was not UTF-8.
        field: &'static str,
        /// The exact UTF-8 validation failure.
        #[source]
        source: std::str::Utf8Error,
    },
}

impl SessionError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::PristineBroken { .. } => error::SESSION_PRISTINE_BROKEN,
            Self::VerifyFailed { .. } => error::SESSION_VERIFY_FAILED,
            Self::UnknownMutant { .. } => error::SESSION_UNKNOWN_MUTANT,
            Self::NotBeside { .. } => error::SESSION_NOT_BESIDE,
            Self::CrashNonceUnavailable { .. } => error::SESSION_NONCE_UNAVAILABLE,
            Self::UnknownTarget { .. } | Self::SkippedTargetUnknown { .. } => {
                error::SESSION_UNKNOWN_TARGET
            }
            Self::NoTargets { .. } => error::SESSION_NO_TARGETS,
            Self::ApparatusChanged { .. } => error::SESSION_APPARATUS_CHANGED,
            Self::ExpectationsOverlap { .. } => error::CONFIG_INVALID,
            Self::WriteFailed { .. }
            | Self::TemporaryRootUnavailable { .. }
            | Self::ScratchStatePoisoned
            | Self::RoutingStatePoisoned
            | Self::CarryStatePoisoned
            | Self::RoutingCountTooLarge { .. }
            | Self::ExpectationCoverageTooLarge { .. }
            | Self::ItemCatalogGap { .. }
            | Self::RunCountTooLarge { .. }
            | Self::RunCountOverflow
            | Self::RoutingCountExhausted
            | Self::RoutingAnswerAlreadyEstablished
            | Self::DerivedTimeoutOverflow { .. }
            | Self::ExecutionDurationOverflow
            | Self::DurationMillisOverflow { .. }
            | Self::TraceCountTooLarge { .. }
            | Self::DuplicateBaselineTarget { .. }
            | Self::SnapshotBytesOverflow
            | Self::CleanupFailed { .. }
            | Self::CoordinationPoisoned
            | Self::WorkerStatePoisoned
            | Self::WorkerQueueTooLarge { .. }
            | Self::WorkerStartFailed { .. }
            | Self::WorkerPanicked
            | Self::CompletedCountExhausted
            | Self::ScratchSequenceExhausted
            | Self::ScratchUnclaimed { .. }
            | Self::ScratchCreateFailed { .. }
            | Self::ScratchUnreadable { .. }
            | Self::WorkspacePathNotUtf8 { .. }
            | Self::CatalogTextNotUtf8 { .. } => error::SESSION_WRITE_FAILED,
            Self::SelectionSourceMissing { .. }
            | Self::SelectionSourceNotUtf8 { .. }
            | Self::SelectionPositionInvalid { .. }
            | Self::EvidencePathOutside { .. }
            | Self::EvidencePathNotUtf8 { .. }
            | Self::EvidencePathInvalid { .. }
            | Self::EvidenceReadFailed { .. } => error::INSTRUMENT_SOURCE_MISMATCH,
            Self::ReachesOutside { .. } => error::WORKSPACE_REACHES_OUTSIDE,
            Self::RootIsNotTheWorkspace { .. } => error::ROOT_IS_NOT_THE_WORKSPACE,
        }
    }
}

fn trace_count(subject: &'static str, count: usize) -> Result<u64, SessionError> {
    u64::try_from(count).map_err(|_overflow| SessionError::TraceCountTooLarge { subject, count })
}

fn record_cleanup_failure(
    first: &mut Option<SessionError>,
    operation: &'static str,
    path: &Path,
    source: std::io::Error,
) {
    if first.is_none() {
        *first = Some(SessionError::CleanupFailed {
            operation,
            path: path.to_path_buf(),
            source,
        });
    }
}

impl Workspace {
    /// Refuses a tree a copy of which would not build: one that is a member of a workspace, and one that reads code from outside itself.
    fn reachable(
        root: &Path,
        toolchain: &Toolchain,
        options: &OpenOptions,
        cancel: &Cancel,
    ) -> Result<Option<String>, crate::EngineError> {
        let metadata = Metadata::load_no_deps(
            &Driver {
                toolchain,
                dir: root,
                cancel,
                trace: &options.trace,
            },
            MetadataOptions {
                locked: options.locked,
                offline: options.offline,
            },
        )?;
        let workspace_root = match crate::canonical::canonical(&metadata.workspace_root) {
            Ok(workspace_root) => workspace_root,
            Err(_metadata_root_has_no_physical_spelling) => metadata.workspace_root.clone(),
        };
        if workspace_root != root {
            return Err(SessionError::RootIsNotTheWorkspace {
                root: root.to_path_buf(),
                workspace_root,
            }
            .into());
        }
        let allowed: Vec<PathBuf> = options
            .allow_outside
            .iter()
            .map(|path| match crate::canonical::canonical(path) {
                Ok(path) => path,
                Err(_allowed_path_does_not_exist_yet) => path.clone(),
            })
            .collect();
        let patches = crate::cargo::manifest::patches(root)?;
        for outside in crate::cargo::reaching_outside(&metadata, root, &patches) {
            if allowed.iter().any(|allow| outside.path.starts_with(allow)) {
                continue;
            }
            return Err(SessionError::ReachesOutside {
                name: outside.name,
                manifest: outside.manifest,
                path: outside.path,
                root: root.to_path_buf(),
            }
            .into());
        }
        Ok(within(root, &metadata.target_directory).map_err(SessionError::from)?)
    }

    /// Sweeps the temporary area, copies `root` into a snapshot, and locates the toolchain inside the copy.
    ///
    /// # Errors
    /// The snapshot's refusals, and whatever stopped the toolchain from being located or `cargo metadata` from being read.
    pub fn open(
        root: &Path,
        options: OpenOptions,
        cancel: &Cancel,
    ) -> Result<Self, crate::EngineError> {
        let phase = options.trace.phase("open");
        let root = match crate::canonical::canonical(root) {
            Ok(root) => root,
            Err(_root_has_no_physical_spelling) => root.to_path_buf(),
        };
        let parent = TemporaryRoot::open(&options.temp_directory)?;
        let now = jiff::Timestamp::now();
        let swept = match tempowner::sweep(parent.path(), &SWEPT_PREFIXES, now) {
            Ok(swept) => swept,
            Err(source) => SweepResult {
                failures: vec![tempowner::SweepFailure {
                    dir: parent.path().to_path_buf(),
                    source,
                }],
                ..SweepResult::default()
            },
        };

        let toolchain = Toolchain::locate(
            &LocateOptions {
                cargo: options.cargo.clone(),
                search_path: options.search_path.clone(),
                env: Some(options.env.clone()),
            },
            &root,
            cancel,
        )?;
        let build_dir = Self::reachable(&root, &toolchain, &options, cancel)?;

        let rules = Self::rules_for(&root, build_dir, parent.path(), &options)?;
        let snapshot = Self::copy(&root, &rules, &options, now)?;
        let (watched, base_env) = Self::watching(&snapshot, &options.env)?;
        options.trace.open(OpenRecord {
            root: root.display().to_string(),
            snapshot_dir: snapshot.dir().display().to_string(),
            stable_dir: snapshot.stable_dir(),
            sweep: Some(SweepRecord {
                parent: parent.path().display().to_string(),
                removed: trace_count("removed temporary directories", swept.removed.len())?,
                removed_bytes: swept.removed_bytes,
                live: trace_count("live temporary directories", swept.live)?,
                kept: trace_count("kept temporary directories", swept.kept)?,
                failures: trace_count("temporary cleanup failures", swept.failures.len())?,
            }),
        });
        let metadata = Metadata::load(
            &Driver {
                toolchain: &toolchain,
                dir: snapshot.root(),
                cancel,
                trace: &options.trace,
            },
            MetadataOptions {
                locked: options.locked,
                offline: options.offline,
            },
        )?;
        let target_dir = target_of(parent.path(), &root);
        let target_owner = claim_target(&target_dir, now, &root);
        let (scratch_dir, scratch_owner) = claim_scratch(parent.path(), now)?;
        let scratch_owner = Some(scratch_owner);
        phase.end();
        Ok(Self {
            snapshot,
            rules,
            watched,
            toolchain,
            metadata,
            target_dir,
            target_owner,
            scratch_dir,
            scratch_owner,
            base_env,
            swept,
            keep_temp: options.keep_temp,
            offline: options.offline,
            locked: options.locked,
            trace: options.trace,
        })
    }

    /// The directory a process of the instrumented tree that lost the run's environment says so in, and the environment every process the run starts carries it in, whatever an outer run set.
    fn watching(
        snapshot: &Snapshot,
        env: &[(OsString, OsString)],
    ) -> Result<(String, Vec<(OsString, OsString)>), SessionError> {
        let path = snapshot.dir().join(WATCHED_NAME);
        let watched = path
            .to_str()
            .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: path.clone() })?
            .to_owned();
        let mut base_env: Vec<(OsString, OsString)> = env
            .iter()
            .filter(|(name, _)| name != crate::instrument::WATCHED_ENV)
            .cloned()
            .collect();
        base_env.push((
            OsString::from(crate::instrument::WATCHED_ENV),
            OsString::from(&watched),
        ));
        Ok((watched, base_env))
    }

    /// Copies the tree and records what that produced.
    fn copy(
        root: &Path,
        rules: &SnapshotOptions,
        options: &OpenOptions,
        now: jiff::Timestamp,
    ) -> Result<Snapshot, crate::EngineError> {
        let started = std::time::Instant::now();
        let snapshot = snapshot::create(rules, now)?;
        let files = trace_count("snapshot files", snapshot.manifest().len())?;
        let bytes = snapshot
            .manifest()
            .iter()
            .try_fold(0u64, |total, entry| total.checked_add(entry.size));
        let bytes = bytes.ok_or(SessionError::SnapshotBytesOverflow)?;
        let duration = started.elapsed();
        let duration_ms = u64::try_from(duration.as_millis())
            .map_err(|_overflow| SessionError::DurationMillisOverflow { duration })?;
        options.trace.snapshot(SnapshotRecord {
            source_root: root.display().to_string(),
            dir: snapshot.dir().display().to_string(),
            files,
            bytes,
            workspace_digest: Some(snapshot.workspace_digest().to_owned()),
            duration_ms,
            error: None,
        });
        Ok(snapshot)
    }

    /// The absolute source root that was copied.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.snapshot.source_root()
    }

    /// The root of the copy everything happens in.
    #[must_use]
    pub fn snapshot_root(&self) -> &Path {
        self.snapshot.root()
    }

    /// The directory the snapshot owns.
    #[must_use]
    pub fn snapshot_dir(&self) -> &Path {
        self.snapshot.dir()
    }

    /// Every entry the snapshot did not copy because it is not a regular file.
    #[must_use]
    pub fn passed_over(&self) -> &[snapshot::PassedOver] {
        self.snapshot.passed_over()
    }

    /// The frozen digest of the copied tree.
    #[must_use]
    pub fn workspace_digest(&self) -> &str {
        self.snapshot.workspace_digest()
    }

    /// The directory a process of the instrumented tree that lost the run's environment says so in.
    #[must_use]
    pub fn watched(&self) -> &str {
        &self.watched
    }

    /// Every regular file the copy holds, as it was copied, sorted by path.
    #[must_use]
    pub fn copied(&self) -> &[snapshot::Entry] {
        self.snapshot.manifest()
    }

    /// What the source tree holds now, read under the rules it was copied by.
    ///
    /// # Errors
    /// A walk or read failure rather than a tree read partly.
    pub fn survey(&self) -> Result<snapshot::Survey, snapshot::SnapshotError> {
        snapshot::survey(&self.rules)
    }

    /// What the tree at `root` holds now, read under the rules a workspace opened with `options` would copy it by, without copying it.
    ///
    /// # Errors
    /// What locating the toolchain and reading the workspace's metadata refuse, and a walk or read failure.
    pub fn survey_at(
        root: &Path,
        options: &OpenOptions,
        cancel: &Cancel,
    ) -> Result<snapshot::Survey, crate::EngineError> {
        let root = match crate::canonical::canonical(root) {
            Ok(root) => root,
            Err(_root_has_no_physical_spelling) => root.to_path_buf(),
        };
        let toolchain = Toolchain::locate(
            &LocateOptions {
                cargo: options.cargo.clone(),
                search_path: options.search_path.clone(),
                env: Some(options.env.clone()),
            },
            &root,
            cancel,
        )?;
        let build_dir = Self::reachable(&root, &toolchain, options, cancel)?;
        let rules = Self::rules_for(&root, build_dir, &options.temp_directory, options)?;
        Ok(snapshot::survey(&rules)?)
    }

    /// The rules a workspace opened with `options` copies the tree at `root` by.
    fn rules_for(
        root: &Path,
        build_dir: Option<String>,
        parent: &Path,
        options: &OpenOptions,
    ) -> Result<SnapshotOptions, crate::EngineError> {
        Ok(SnapshotOptions {
            exclude: options.exclude.clone(),
            layout: snapshot::Layout::plan(root, &options.allow_outside)?,
            report_dir: options.report_directory.clone(),
            build_dir,
            dest_parent: parent.to_path_buf(),
        })
    }

    /// Re-hashes the private source tree against the manifest captured while it was copied.
    /// Proof layers use this after restoring a temporary edit so a build script or verifier cannot silently write proof context for a later question.
    ///
    /// # Errors
    /// Returns a snapshot walk or read failure rather than treating an unreadable tree as unchanged.
    pub fn changes(&self) -> Result<Vec<snapshot::Drift>, snapshot::SnapshotError> {
        self.snapshot.redigest()
    }

    /// Makes the private tree as it stands now the baseline for later [`Self::changes`] checks.
    ///
    /// Proof layers use this only after independently checking the copied tree's digest and restoring every mutable source to its pristine bytes.
    /// That gives each proof attempt a baseline which contains neither instrumentation nor output written by an earlier process.
    ///
    /// # Errors
    /// Returns a snapshot walk or read failure instead of accepting a tree that could not be completely observed.
    pub fn reseal(&mut self) -> Result<Vec<snapshot::Drift>, snapshot::SnapshotError> {
        self.snapshot.reseal()
    }

    /// What the sweep on the way in collected.
    #[must_use]
    pub const fn swept(&self) -> &SweepResult {
        &self.swept
    }

    /// The located toolchain.
    #[must_use]
    pub const fn toolchain(&self) -> &Toolchain {
        &self.toolchain
    }

    /// What `cargo metadata` said about the copy.
    #[must_use]
    pub const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// The target directory builds go into: outside the snapshot, at a name stable for this source root, so successive runs share cargo's incremental state.
    #[must_use]
    pub fn target_dir(&self) -> &Path {
        &self.target_dir
    }

    /// The target directory with every member and the files of the copy under it, which a build settles before cargo judges a unit fresh.
    #[must_use]
    pub fn build_dir(&self) -> crate::cargo::BuildDir {
        let root = &self.metadata.workspace_root;
        let members = self
            .metadata
            .members()
            .map(|package| {
                let under = match package.manifest_dir().strip_prefix(root) {
                    Ok(relative) => match crate::id::slashed(relative) {
                        Ok(named) => Some(named),
                        Err(_unnameable) => None,
                    },
                    Err(_outside_the_root) => None,
                };
                crate::cargo::Member {
                    name: package.name.clone(),
                    files: self
                        .snapshot
                        .manifest()
                        .iter()
                        .filter(|entry| member_holds(under.as_deref(), &entry.rel_path))
                        .map(|entry| crate::cargo::MemberFile {
                            rel_path: entry.rel_path.clone(),
                            path: self.snapshot.root().join(&entry.rel_path),
                        })
                        .collect(),
                }
            })
            .collect();
        crate::cargo::BuildDir::new(self.target_dir.clone(), members)
    }

    /// Ends every process this run started that is still running, having left every execution's process group, and says so in the trace.
    fn stop_escaped(&self) {
        let found = match crate::escaped::working_under(&[self.snapshot.dir(), &self.scratch_dir]) {
            Ok(found) => found,
            Err(error) => {
                self.trace.note(
                    ESCAPED_PROCESSES,
                    &format!("the processes this run started could not be listed: {error}"),
                );
                return;
            }
        };
        if found.is_empty() {
            return;
        }
        let failed: Vec<String> = found
            .iter()
            .filter_map(|pid| match crate::escaped::stop(*pid) {
                Ok(()) => None,
                Err(error) => Some(format!("{pid}: {error}")),
            })
            .collect();
        self.trace.note(
            ESCAPED_PROCESSES,
            &format!(
                "{} process(es) a test started outside its execution's process group were still \
                 running when the run closed, and were ended: {found:?}{}",
                found.len(),
                if failed.is_empty() {
                    String::new()
                } else {
                    format!("; these could not be: {}", failed.join(", "))
                }
            ),
        );
    }

    /// The directory this run's test processes work in: beside the target directory, and removed when the run closes.
    #[must_use]
    pub fn scratch_dir(&self) -> &Path {
        &self.scratch_dir
    }

    /// Discovers, instruments, validates, and builds; see [`prepare()`].
    ///
    /// # Errors
    /// Every failure of the phases it runs.
    pub fn prepare(
        self,
        options: &PrepareOptions,
        cancel: &Cancel,
    ) -> Result<Session, crate::EngineError> {
        prepare(self, options, cancel)
    }

    /// Removes the snapshot, or preserves it when the workspace was opened with `keep_temp`, and reports what was preserved.
    ///
    /// # Errors
    /// A snapshot directory that could not be removed.
    pub fn close(mut self) -> Result<Vec<PathBuf>, crate::EngineError> {
        self.stop_escaped();
        let mut failure = None;
        if let Some(mut owner) = self.target_owner.take() {
            match owner.release() {
                Ok(()) => {}
                Err(source) => record_cleanup_failure(
                    &mut failure,
                    "release build-cache ownership of",
                    &self.target_dir,
                    source,
                ),
            }
        }
        if self.keep_temp {
            let dir = self.snapshot.dir().to_path_buf();
            if let Some(mut owner) = self.scratch_owner.take() {
                match owner.keep() {
                    Ok(()) => {}
                    Err(source) => record_cleanup_failure(
                        &mut failure,
                        "mark execution scratch kept at",
                        &self.scratch_dir,
                        std::io::Error::other(source),
                    ),
                }
            }
            self.snapshot.keep()?;
            if let Some(failure) = failure {
                return Err(failure.into());
            }
            return Ok(vec![dir, self.target_dir, self.scratch_dir]);
        }
        if let Some(mut owner) = self.scratch_owner.take() {
            match owner.empty() {
                Ok(()) => {}
                Err(source) => record_cleanup_failure(
                    &mut failure,
                    "empty execution scratch at",
                    &self.scratch_dir,
                    source,
                ),
            }
            match owner.release() {
                Ok(()) => {}
                Err(source) => record_cleanup_failure(
                    &mut failure,
                    "release execution-scratch ownership of",
                    &self.scratch_dir,
                    source,
                ),
            }
        }
        match tempowner::remove_tree(&self.scratch_dir) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => record_cleanup_failure(
                &mut failure,
                "remove execution scratch at",
                &self.scratch_dir,
                source,
            ),
        }
        match (failure, self.snapshot.cleanup()) {
            (_, Err(source)) => Err(source.into()),
            (Some(failure), Ok(())) => Err(failure.into()),
            (None, Ok(())) => Ok(Vec::new()),
        }
    }

    /// A driver for a cargo command in the snapshot.
    pub(crate) fn driver<'a>(&'a self, cancel: &'a Cancel) -> Driver<'a> {
        Driver {
            toolchain: &self.toolchain,
            dir: self.snapshot.root(),
            cancel,
            trace: &self.trace,
        }
    }

    /// How long a command may take, when the caller bounded it.
    pub(crate) const fn timeout(build_timeout: Option<Duration>) -> Option<Duration> {
        build_timeout
    }
}
