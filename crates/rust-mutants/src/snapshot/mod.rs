// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A disposable copy of a source tree, so that mutation testing never writes
//! to the tree a user is editing.
//!
//! Every other design for a mutation tester ends up rewriting the sources in
//! place and restoring them afterwards. That is a promise no process can
//! keep: a crash, a SIGKILL, a full disk, or a second run racing the first
//! leaves the checkout holding mutated code. rust-mutants instead treats the
//! user's tree as strictly read-only. It is copied once, instrumented in the
//! copy, and the copy is deleted at the end of the run. The worst outcome of
//! a crash is a directory left behind in the temporary area, which the next
//! run's [`tempowner::sweep`] collects.
//!
//! # What a snapshot is
//!
//! [`create`] walks the source tree, copies it byte for byte into a directory
//! named after the source root, and returns a [`Snapshot`] holding the root
//! of the copy, a sorted [`Entry`] manifest, and the workspace digest that
//! names the tree's exact contents. The digest is what makes a cached outcome
//! trustworthy — a result is reusable only for a workspace whose every byte
//! hashes the same — and what proves two shards of one run looked at the
//! same code.
//!
//! The directory's name is the [`stable_name`] of the root rather than a
//! random one, because Cargo fingerprints build artifacts by absolute path:
//! a run at a fresh path is a whole-workspace rebuild that shares nothing
//! with the last run's target directory. Only the path is reused. A directory
//! left over from a previous run is swept and copied into again, never
//! adopted. When the name is held by a directory the sweep will not collect,
//! the fallback name is a fresh one, and [`Snapshot::stable_dir`] says which
//! of the two happened.
//!
//! # Refusals
//!
//! The walk copies directories and regular files. Everything else is refused
//! with a typed [`SnapshotError`] naming the path: symbolic links, Windows
//! reparse points, devices, sockets, and named pipes.
//!
//! Refusing rather than skipping is the point. A skipped link is a file that
//! silently is not there: the copy still compiles often enough to be
//! believed, the mutants that would have lived behind the link never appear
//! in the catalog, and the score comes out flattering and wrong. A link is
//! also the one way a copy can escape its own root, which would turn "the
//! tree is read-only" into a lie. Neither failure announces itself, so the
//! walk announces it instead, and the user decides — usually by adding an
//! exclude pattern, which is checked before the entry is even stat'ed.
//!
//! # Exclusions
//!
//! [`Options::exclude`] holds compiled [`glob`] patterns matched against the
//! `/`-normalized path of each entry relative to the source root. A directory
//! that matches is not descended into at all.
//!
//! Three exclusions are always applied, before the caller's: `.git` at any
//! depth, the conventional report directory [`DEFAULT_REPORT_DIR`], and
//! [`Options::report_dir`] when the caller configured a different one. The
//! middle one is deliberately unconditional, so a run cannot snapshot the
//! reports of a previous run just because the report directory was
//! reconfigured after the fact.
//!
//! # The workspace digest
//!
//! The recipe is frozen. Over the manifest in sorted order:
//!
//! ```text
//! SHA-256( enc("rust-mutants-workspace-v1")
//!          || enc(rel_path_1) || enc(sha256hex_1)
//!          || ...
//!          || enc(rel_path_n) || enc(sha256hex_n) )
//! ```
//!
//! where `enc(s)` is a 4-byte big-endian length of the UTF-8 bytes of `s`
//! followed by those bytes. Sizes are not hashed: a file's SHA-256 already
//! pins its bytes, and the size is carried in the manifest for reporting.
//! Directories are not hashed either, since an empty directory cannot change
//! what a build produces. The domain separator carries the version, so a v1
//! digest can never be mistaken for a v2 digest in a cache that outlived the
//! upgrade.
//!
//! [`glob`]: crate::glob

#![allow(
    clippy::create_dir,
    reason = "exclusive creation is the point: an existing directory is a fact to react to, never one to paper over with create_dir_all"
)]

use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

use crate::error::{self, ErrorCode};
use crate::glob::Pattern;
use crate::id::normalize_path;
use crate::tempowner::{self, ClaimError, Owner};

/// The domain separator hashed first for every workspace digest. It carries
/// the recipe version; see the module documentation.
pub const WORKSPACE_DOMAIN: &str = "rust-mutants-workspace-v1";

/// Begins the name of every snapshot directory, the stable one and the
/// fallback alike.
///
/// It is load bearing: [`Snapshot::cleanup`] refuses to delete a directory
/// whose name does not start with it, and the [`tempowner`] sweep collects
/// the abandoned ones by it.
pub const DIR_PREFIX: &str = "rust-mutants-snap-";

/// The subdirectory of a snapshot directory that holds the copy.
///
/// The copy is one level down rather than at the top because the snapshot
/// directory also carries its ownership files, and every byte under
/// [`Snapshot::root`] has to be a byte that came from the source tree:
/// [`Snapshot::redigest`] applies no exclusions, and a snapshot of a snapshot
/// — which is how the probe tree is made — must not copy the owner marker.
pub const TREE_NAME: &str = "tree";

/// The conventional location of a run's reports, excluded from every
/// snapshot whether or not it is the configured one.
pub const DEFAULT_REPORT_DIR: &str = "reports/mutation";

/// How much of the source root's digest [`stable_name`] spells out.
///
/// Sixty-four bits keeps the directories of every workspace on a machine
/// apart with room to spare, and the name stays short enough that deep paths
/// still fit.
pub const STABLE_NAME_HEX_LENGTH: usize = 16;

/// How many times [`Snapshot::cleanup`] tries the removal before giving up.
///
/// Bounded on purpose: a snapshot that will not go away is a diagnostic to
/// report, not a reason for the process to hang at the end of a run.
pub const CLEANUP_ATTEMPTS: usize = 5;

/// The pause before the second removal attempt; it doubles for each attempt
/// after that, so the ladder is 20, 40, 80, 160 ms and the whole loop costs
/// at most a third of a second.
pub const CLEANUP_BACKOFF: Duration = Duration::from_millis(20);

/// How many fresh names [`create`] tries when the stable one is taken.
const FALLBACK_ATTEMPTS: u32 = 64;

/// The copy buffer.
const COPY_BUFFER: usize = 64 * 1024;

/// Configures [`create`].
#[derive(Debug, Clone)]
pub struct Options {
    /// Patterns matched against each entry's `/`-normalized path relative to
    /// the source root. A matching directory is skipped whole.
    pub exclude: Vec<Pattern>,
    /// The configured report directory as a source-root-relative path. `None`
    /// means the default. It is excluded in addition to, never instead of,
    /// [`DEFAULT_REPORT_DIR`].
    pub report_dir: Option<String>,
    /// The absolute directory the snapshot is created in. The composition
    /// root decides where the temporary area is; this module never asks the
    /// process environment.
    pub dest_parent: PathBuf,
}

impl Options {
    /// Options with only the built-in exclusions, creating under `dest_parent`.
    pub fn new(dest_parent: impl Into<PathBuf>) -> Self {
        Self {
            exclude: Vec::new(),
            report_dir: None,
            dest_parent: dest_parent.into(),
        }
    }
}

/// One regular file in a snapshot's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The path relative to the snapshot root, with forward slashes on every
    /// platform.
    pub rel_path: String,
    /// The number of bytes copied.
    pub size: u64,
    /// The lowercase hex SHA-256 of the file's bytes.
    pub sha256: String,
}

impl Entry {
    /// The size and digest without the path.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            size: self.size,
            sha256: self.sha256.clone(),
        }
    }
}

/// The size and digest of one file, as recorded or as found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The number of bytes.
    pub size: u64,
    /// The lowercase hex SHA-256 of the bytes.
    pub sha256: String,
}

/// How a path in the snapshot stopped agreeing with the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriftKind {
    /// Present in the snapshot, absent from the manifest.
    Added,
    /// Present in the manifest, absent from the snapshot.
    Removed,
    /// Present in both, with different bytes.
    Changed,
}

impl DriftKind {
    /// The lowercase name, which is also the spelling used in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
        }
    }
}

/// One disagreement between the manifest and the snapshot as it stands now.
/// Both sides are carried where they exist, so a caller can report "1.2 kB
/// became 0 bytes" without walking the tree a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// A file the manifest does not know.
    Added {
        /// The `/`-normalized path relative to the snapshot root.
        rel_path: String,
        /// What the snapshot holds now.
        got: Fingerprint,
    },
    /// A file the manifest recorded that is gone.
    Removed {
        /// The `/`-normalized path relative to the snapshot root.
        rel_path: String,
        /// What the manifest recorded.
        want: Fingerprint,
    },
    /// A file whose bytes differ from the manifest.
    Changed {
        /// The `/`-normalized path relative to the snapshot root.
        rel_path: String,
        /// What the manifest recorded.
        want: Fingerprint,
        /// What the snapshot holds now.
        got: Fingerprint,
    },
}

impl Drift {
    /// How the path drifted.
    #[must_use]
    pub const fn kind(&self) -> DriftKind {
        match self {
            Self::Added { .. } => DriftKind::Added,
            Self::Removed { .. } => DriftKind::Removed,
            Self::Changed { .. } => DriftKind::Changed,
        }
    }

    /// The `/`-normalized path relative to the snapshot root.
    #[must_use]
    pub fn rel_path(&self) -> &str {
        match self {
            Self::Added { rel_path, .. }
            | Self::Removed { rel_path, .. }
            | Self::Changed { rel_path, .. } => rel_path,
        }
    }
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotErrorKind {
    /// [`Options`] that cannot be honoured, such as a report directory that
    /// is absolute or climbs out of the source root.
    InvalidOptions,
    /// A source root that is relative, cannot be read, or is not a directory.
    SourceRoot,
    /// An operating system failure while reading the tree: a directory that
    /// cannot be listed, an entry that cannot be stat'ed.
    Walk,
    /// A symbolic link inside the source tree.
    Symlink,
    /// A Windows reparse point — a junction, a mount point, or any other name
    /// surrogate — inside the source tree.
    ReparsePoint,
    /// A file that is neither a directory nor a regular file: a device, a
    /// socket, a named pipe.
    Irregular,
    /// A file name that cannot survive the round trip through a
    /// `/`-normalized relative path, such as one containing a backslash.
    UnsupportedName,
    /// A failure to create or claim the snapshot directory itself.
    Destination,
    /// A failure while copying the tree into the snapshot.
    Copy,
    /// A cleanup that was refused because the recorded directory does not
    /// look like one this module created. It is the guard that stands between
    /// a bug in rust-mutants and a user's source tree.
    CleanupRefused,
    /// A snapshot directory that survived every removal attempt, usually a
    /// file still locked by a test binary on Windows.
    CleanupFailed,
}

impl SnapshotErrorKind {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::InvalidOptions => error::SNAPSHOT_INVALID_OPTIONS,
            Self::SourceRoot => error::SNAPSHOT_SOURCE_ROOT,
            Self::Walk => error::SNAPSHOT_WALK,
            Self::Symlink => error::SNAPSHOT_SYMLINK,
            Self::ReparsePoint => error::SNAPSHOT_REPARSE_POINT,
            Self::Irregular => error::SNAPSHOT_IRREGULAR,
            Self::UnsupportedName => error::SNAPSHOT_UNSUPPORTED_NAME,
            Self::Destination => error::SNAPSHOT_DESTINATION,
            Self::Copy => error::SNAPSHOT_COPY,
            Self::CleanupRefused => error::SNAPSHOT_CLEANUP_REFUSED,
            Self::CleanupFailed => error::SNAPSHOT_CLEANUP_FAILED,
        }
    }
}

/// Every error this module returns, so a caller can always reach the code
/// and the path without matching on message text.
#[derive(Debug)]
pub struct SnapshotError {
    kind: SnapshotErrorKind,
    path: String,
    message: String,
    source: Option<io::Error>,
}

impl SnapshotError {
    fn new(kind: SnapshotErrorKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            message: message.into(),
            source: None,
        }
    }

    fn with_source(mut self, source: io::Error) -> Self {
        self.source = Some(source);
        self
    }

    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> SnapshotErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }

    /// The path the error is about: a `/`-normalized path relative to the
    /// tree being walked wherever one exists, because that is the spelling
    /// the manifest, the report, and the exclude patterns all use. It is an
    /// absolute path only when the error is about a root or a destination,
    /// which have no relative spelling.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The problem in one clause, without the code and the path, for a
    /// caller that already shows those.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The operating system error underneath, when there is one.
    #[must_use]
    pub const fn source(&self) -> Option<&io::Error> {
        self.source.as_ref()
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: snapshot: {}", self.code().code, self.message)?;
        if !self.path.is_empty() {
            write!(f, ": {:?}", self.path)?;
        }
        if let Some(source) = &self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| -> &(dyn std::error::Error + 'static) { source })
    }
}

/// Whether the snapshot directory is still this process's to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Owned: the deferred removal in `Drop` applies.
    Live,
    /// Preserved on purpose; nothing removes it.
    Kept,
    /// An explicit cleanup ran, whatever its outcome; `Drop` does nothing.
    Released,
}

/// A disposable copy of a source tree.
///
/// A snapshot is immutable once [`create`] returns, apart from what the rest
/// of the pipeline writes into the directory itself. It owns an operating
/// system resource: [`Snapshot::cleanup`] is the authoritative release, and
/// `Drop` is the best-effort one for the paths that never reach it.
#[derive(Debug)]
pub struct Snapshot {
    source_root: PathBuf,
    root: PathBuf,
    dir: PathBuf,
    dest_parent: PathBuf,
    manifest: Vec<Entry>,
    workspace_digest: String,
    stable_dir: bool,
    owner: Option<Owner>,
    state: State,
}

/// Copies the tree rooted at `source_root` into a directory named after that
/// root, inside [`Options::dest_parent`].
///
/// The source tree is only ever read. On any failure after the destination
/// exists, the partial copy is removed before the error is returned, so a
/// failed `create` leaves nothing behind.
///
/// # The directory it lands in
///
/// The name is the [`stable_name`] of the source root, so that Cargo's
/// fingerprints recognise the crates underneath it from one run to the next.
/// Only the path is ever reused. A directory found under the stable name is
/// swept — removed entirely — before anything is copied into it; there is no
/// path through this function that adopts a tree it did not write.
///
/// When the name is held by a directory the sweep will not collect, `create`
/// takes a fresh name instead rather than wait, because none of those
/// directories is going to be released: a locked one belongs to a concurrent
/// run of the same root, a kept one is evidence preserved on purpose, and an
/// unowned young one may be a run in progress under an older binary. That run
/// loses its build-cache hits and nothing else, and [`Snapshot::stable_dir`]
/// is where it says so.
///
/// # Two runs of one root at once
///
/// Neither order of the race puts two processes in one directory. Between the
/// `create_dir` that took the name and the claim, the directory carries no
/// marker and a modification time of a moment ago, which is exactly the
/// shape a concurrent sweep spares. Between a sweep removing an orphan and
/// the `create_dir` that follows it, another process may create the directory
/// first; then that `create_dir` fails and this run falls back — which is why
/// the sweep is followed by one more attempt and not by a retry loop.
///
/// # Errors
///
/// Every failure is a [`SnapshotError`] naming the path it is about.
pub fn create(
    source_root: &Path,
    options: &Options,
    now: Timestamp,
) -> Result<Snapshot, SnapshotError> {
    if !source_root.is_absolute() {
        return Err(SnapshotError::new(
            SnapshotErrorKind::SourceRoot,
            source_root.display().to_string(),
            "source root must be an absolute path",
        ));
    }
    // `metadata`, not `symlink_metadata`: a user whose whole checkout lives
    // behind a symlink has made that choice deliberately, and the rejection
    // rule is about links discovered inside the tree.
    let info = fs::metadata(source_root).map_err(|source| {
        SnapshotError::new(
            SnapshotErrorKind::SourceRoot,
            source_root.display().to_string(),
            "cannot read the source root",
        )
        .with_source(source)
    })?;
    if !info.is_dir() {
        return Err(SnapshotError::new(
            SnapshotErrorKind::SourceRoot,
            source_root.display().to_string(),
            "source root is not a directory",
        ));
    }
    let patterns = exclusions(options)?;

    let mut walker = Walker::new(source_root, &patterns);
    walker.walk("")?;
    walker.rejection()?;
    let Walker { files, dirs, .. } = walker;

    let (dir, stable) = destination(&options.dest_parent, source_root, now)?;
    // Claimed before a single byte is copied into it: a directory that holds
    // a copy of somebody's workspace and says nothing about who is using it
    // is exactly the orphan the sweep exists to collect.
    let owner = claim_destination(&dir, now)?;
    let mut snapshot = Snapshot {
        source_root: source_root.to_path_buf(),
        root: dir.join(TREE_NAME),
        dest_parent: dir.parent().map(Path::to_path_buf).unwrap_or_default(),
        dir,
        manifest: Vec::new(),
        workspace_digest: workspace_digest(&[]),
        stable_dir: stable,
        owner: Some(owner),
        state: State::Live,
    };
    match populate(&snapshot.root, &dirs, &files) {
        Ok(manifest) => {
            snapshot.workspace_digest = workspace_digest(&manifest);
            snapshot.manifest = manifest;
            Ok(snapshot)
        }
        Err(cause) => {
            // Best effort: reporting a failure to tidy up a directory the
            // caller never saw would bury the error that matters.
            drop(snapshot.cleanup());
            Err(cause)
        }
    }
}

/// Creates the tree and copies the files into it, returning the manifest.
fn populate(root: &Path, dirs: &[Record], files: &[Record]) -> Result<Vec<Entry>, SnapshotError> {
    // Explicit, so a source tree with no subdirectories still has a root.
    fs::create_dir(root).map_err(|source| {
        SnapshotError::new(
            SnapshotErrorKind::Destination,
            root.display().to_string(),
            "cannot create the snapshot tree",
        )
        .with_source(source)
    })?;
    // Directories first, in sorted order, which puts every parent before its
    // children. Creating them explicitly rather than on demand from the file
    // loop is what keeps an empty directory in the copy: a build can depend
    // on one existing even though nothing in it hashes.
    for dir in dirs {
        let path = path_of(root, &dir.rel);
        create_directory(&path, &dir.meta).map_err(|source| {
            SnapshotError::new(
                SnapshotErrorKind::Copy,
                dir.rel.clone(),
                "cannot create the directory in the snapshot",
            )
            .with_source(source)
        })?;
    }
    let mut manifest = Vec::with_capacity(files.len());
    for file in files {
        let (size, sha256) =
            copy_file(&file.abs, &path_of(root, &file.rel), &file.meta).map_err(|source| {
                SnapshotError::new(
                    SnapshotErrorKind::Copy,
                    file.rel.clone(),
                    "cannot copy the file into the snapshot",
                )
                .with_source(source)
            })?;
        manifest.push(Entry {
            rel_path: file.rel.clone(),
            size,
            sha256,
        });
    }
    Ok(manifest)
}

/// Turns a `/`-normalized relative path into a native path under `root`.
fn path_of(root: &Path, rel: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(rel.split('/'));
    path
}

/// Computes the frozen digest of a manifest.
///
/// The entries must already be sorted by path, which is how [`create`]
/// returns them; this function hashes what it is given and does not reorder,
/// because a digest that quietly repaired its input could not be
/// reimplemented from the recipe.
#[must_use]
pub fn workspace_digest(entries: &[Entry]) -> String {
    let mut hasher = Sha256::new();
    write_length_prefixed(&mut hasher, WORKSPACE_DOMAIN);
    for entry in entries {
        write_length_prefixed(&mut hasher, &entry.rel_path);
        write_length_prefixed(&mut hasher, &entry.sha256);
    }
    hex::encode(hasher.finalize())
}

/// Writes `enc(s)`: a 4-byte big-endian byte length, then the bytes.
///
/// This is byte for byte the encoding of the identity recipe in [`crate::id`],
/// duplicated rather than shared on purpose: the two recipes are frozen and
/// versioned separately, and a shared helper would let a change made for one
/// silently re-mint the other. Everything hashed here is a path or a
/// 64-character digest, far below the 32-bit prefix, so there is no error.
fn write_length_prefixed(hasher: &mut Sha256, s: &str) {
    let length = u32::try_from(s.len()).unwrap_or(u32::MAX);
    hasher.update(length.to_be_bytes());
    hasher.update(s.as_bytes());
}

/// Builds the pattern list: the always-on defaults first, then the caller's.
fn exclusions(options: &Options) -> Result<Vec<Pattern>, SnapshotError> {
    let mut patterns = Vec::with_capacity(options.exclude.len().saturating_add(3));
    // "**/.git" so a nested checkout or a submodule is caught at any depth.
    for builtin in ["**/.git", DEFAULT_REPORT_DIR] {
        patterns.push(Pattern::compile(builtin).map_err(|error| {
            SnapshotError::new(
                SnapshotErrorKind::InvalidOptions,
                builtin,
                format!("built-in exclusion is not a usable pattern: {error}"),
            )
        })?);
    }
    if let Some(report_dir) = &options.report_dir {
        // The same canonicalization identities use, so a report directory
        // spelled with backslashes excludes the same tree it would on POSIX,
        // and an absolute or escaping one is refused here rather than
        // silently excluding nothing.
        let normalized = normalize_path(report_dir).map_err(|error| {
            SnapshotError::new(
                SnapshotErrorKind::InvalidOptions,
                report_dir.clone(),
                format!("report directory is not a usable source-root-relative path: {error}"),
            )
        })?;
        if normalized != DEFAULT_REPORT_DIR {
            patterns.push(Pattern::compile(&normalized).map_err(|error| {
                SnapshotError::new(
                    SnapshotErrorKind::InvalidOptions,
                    report_dir.clone(),
                    format!("report directory is not a usable pattern: {error}"),
                )
            })?);
        }
    }
    patterns.extend(options.exclude.iter().cloned());
    Ok(patterns)
}

/// One entry the walk decided to keep.
#[derive(Debug)]
struct Record {
    rel: String,
    abs: PathBuf,
    meta: Metadata,
}

/// Collects a tree into sorted directory and file lists, and collects the
/// entries it refuses instead of failing at the first one.
struct Walker<'a> {
    root: &'a Path,
    exclude: &'a [Pattern],
    files: Vec<Record>,
    dirs: Vec<Record>,
    rejected: Vec<SnapshotError>,
}

impl<'a> Walker<'a> {
    const fn new(root: &'a Path, exclude: &'a [Pattern]) -> Self {
        Self {
            root,
            exclude,
            files: Vec::new(),
            dirs: Vec::new(),
            rejected: Vec::new(),
        }
    }

    /// Reads one directory and recurses. Entries are visited in name order so
    /// the traversal is deterministic; the lists are sorted by relative path
    /// at the end because per-directory name order and whole-path order are
    /// not the same ordering.
    fn walk(&mut self, rel_dir: &str) -> Result<(), SnapshotError> {
        let dir = self.path_of(rel_dir);
        let listing = fs::read_dir(&dir).map_err(|source| {
            SnapshotError::new(
                SnapshotErrorKind::Walk,
                self.err_path(rel_dir),
                "cannot read the directory",
            )
            .with_source(source)
        })?;
        let mut names = Vec::new();
        for entry in listing {
            let entry = entry.map_err(|source| {
                SnapshotError::new(
                    SnapshotErrorKind::Walk,
                    self.err_path(rel_dir),
                    "cannot read the directory",
                )
                .with_source(source)
            })?;
            names.push(entry.file_name());
        }
        names.sort_unstable();
        for name in names {
            self.visit(rel_dir, &name)?;
        }
        if rel_dir.is_empty() {
            self.files.sort_unstable_by(|a, b| a.rel.cmp(&b.rel));
            self.dirs.sort_unstable_by(|a, b| a.rel.cmp(&b.rel));
        }
        Ok(())
    }

    /// Classifies one directory entry: excluded, refused, descended, or kept.
    fn visit(&mut self, rel_dir: &str, name: &OsStr) -> Result<(), SnapshotError> {
        {
            let Some(utf8) = name.to_str() else {
                let rel = join_rel(rel_dir, &name.to_string_lossy());
                self.reject(
                    SnapshotErrorKind::UnsupportedName,
                    rel,
                    "refuses a file name that is not valid UTF-8",
                );
                return Ok(());
            };
            let rel = join_rel(rel_dir, utf8);
            if self.excluded(&rel) {
                return Ok(());
            }
            if let Some(bad) = unsupported_name(utf8) {
                self.reject(
                    SnapshotErrorKind::UnsupportedName,
                    rel,
                    format!("refuses a file name containing {bad}"),
                );
                return Ok(());
            }
            let abs = self.path_of(&rel);
            // `symlink_metadata` rather than the entry's file type: the whole
            // rejection policy hangs on the mode being exact, and one extra
            // syscall per entry buys the question away.
            let meta = fs::symlink_metadata(&abs).map_err(|source| {
                SnapshotError::new(
                    SnapshotErrorKind::Walk,
                    rel.clone(),
                    "cannot stat the entry",
                )
                .with_source(source)
            })?;
            let file_type = meta.file_type();
            if file_type.is_symlink() {
                self.reject(
                    SnapshotErrorKind::Symlink,
                    rel,
                    "refuses to follow a symbolic link",
                );
            } else if platform::is_reparse_point(&meta) {
                self.reject(
                    SnapshotErrorKind::ReparsePoint,
                    rel,
                    "refuses to follow a reparse point (junction or mount point)",
                );
            } else if file_type.is_dir() {
                self.dirs.push(Record {
                    rel: rel.clone(),
                    abs,
                    meta,
                });
                self.walk(&rel)?;
            } else if file_type.is_file() {
                self.files.push(Record { rel, abs, meta });
            } else {
                self.reject(
                    SnapshotErrorKind::Irregular,
                    rel,
                    format!(
                        "refuses a file that is neither a directory nor a regular file ({})",
                        platform::describe(&meta)
                    ),
                );
            }
        }
        Ok(())
    }

    fn path_of(&self, rel: &str) -> PathBuf {
        if rel.is_empty() {
            self.root.to_path_buf()
        } else {
            path_of(self.root, rel)
        }
    }

    /// The spelling an error about `rel` should carry: the relative path
    /// wherever one exists, and the absolute root when it does not. The root
    /// is reachable: a redigest of a snapshot whose tree has been removed
    /// cannot list it, and an error carrying "" would name nothing.
    fn err_path(&self, rel: &str) -> String {
        if rel.is_empty() {
            self.root.display().to_string()
        } else {
            rel.to_owned()
        }
    }

    fn excluded(&self, rel: &str) -> bool {
        self.exclude.iter().any(|pattern| pattern.matches(rel))
    }

    fn reject(&mut self, kind: SnapshotErrorKind, rel: String, message: impl Into<String>) {
        self.rejected.push(SnapshotError::new(kind, rel, message));
    }

    /// Fails with the refused entry that sorts first by relative path, if
    /// any. Reporting the first in path order rather than in visit order
    /// means a user who fixes it and runs again is told about the next one,
    /// in an order that does not depend on how the filesystem laid the
    /// directory out.
    fn rejection(&mut self) -> Result<(), SnapshotError> {
        let Some(position) = self
            .rejected
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.path.cmp(&b.path))
            .map(|(position, _)| position)
        else {
            return Ok(());
        };
        Err(self.rejected.swap_remove(position))
    }
}

fn join_rel(rel_dir: &str, name: &str) -> String {
    if rel_dir.is_empty() {
        name.to_owned()
    } else {
        format!("{rel_dir}/{name}")
    }
}

/// Names the reason a directory entry cannot be represented as a
/// `/`-normalized relative path, or `None` if it can.
///
/// A backslash in a name is possible on POSIX filesystems and would make the
/// snapshot path `a\b.rs` indistinguishable from the directory `a` holding
/// `b.rs` once [`normalize_path`] treats it as a separator — two different
/// files with one identity. It is refused for the same reason a symlink is:
/// quietly picking one of the two meanings is worse than stopping.
fn unsupported_name(name: &str) -> Option<&'static str> {
    if name.is_empty() || name == "." || name == ".." {
        Some("no usable name")
    } else if name.contains('\\') {
        Some("a backslash")
    } else if name.contains('/') {
        Some("a forward slash")
    } else if name.contains('\0') {
        Some("a NUL byte")
    } else {
        None
    }
}

/// Creates one directory of the copy with the source's permissions, forced
/// to owner rwx: a source tree may legitimately contain a r-x directory, and
/// the copy has to be writable or nothing can be instrumented inside it.
fn create_directory(path: &Path, meta: &Metadata) -> io::Result<()> {
    fs::create_dir(path)?;
    platform::finalize_dir_permissions(path, meta)
}

/// Copies one regular file byte for byte, hashing as it goes so the bytes
/// are read once, and returns the size and lowercase hex SHA-256 of what was
/// written.
///
/// There is no newline translation and no byte order mark handling. A CRLF
/// file arrives in the snapshot as a CRLF file, because the line ending is
/// part of the source digest that names every mutant in it.
fn copy_file(src: &Path, dst: &Path, meta: &Metadata) -> io::Result<(u64, String)> {
    let mut input = File::open(src)?;
    // `create_new`: a destination that already exists means the walk produced
    // the same path twice, which is a bug worth surfacing, not a file to
    // overwrite. The creation mode is only a floor (the umask filters it);
    // the explicit permission set below is what preserves the source bits,
    // and it runs after the bytes are written, so the file is briefly less
    // permissive than its source and never more.
    let mut output = platform::create_exclusive(dst, meta)?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let mut buffer = vec![0u8; COPY_BUFFER];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = buffer.get(..read).unwrap_or_default();
        output.write_all(chunk)?;
        hasher.update(chunk);
        size = size.saturating_add(to_u64(read));
    }
    // A file whose permissions could not be set is a failed copy, not a copy
    // with a footnote: a fixture script that lost its executable bit fails
    // inside the snapshot for a reason that has nothing to do with a mutant.
    platform::finalize_file_permissions(&output, meta)?;
    output.flush()?;
    Ok((size, hex::encode(hasher.finalize())))
}

/// The size and lowercase hex SHA-256 of a file already on disk: the
/// read-only half of [`copy_file`], used by [`Snapshot::redigest`].
fn hash_file(path: &Path) -> io::Result<(u64, String)> {
    let mut input = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let mut buffer = vec![0u8; COPY_BUFFER];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
        size = size.saturating_add(to_u64(read));
    }
    Ok((size, hex::encode(hasher.finalize())))
}

/// A byte count as the manifest carries it. Lossless on every supported
/// target; the saturation is for the type system.
fn to_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The name of the snapshot directory of `abs_source_root`: [`DIR_PREFIX`]
/// followed by the first [`STABLE_NAME_HEX_LENGTH`] lowercase hex characters
/// of the SHA-256 of the path's bytes.
///
/// A snapshot's name is derived from the tree it copies rather than drawn at
/// random because Cargo fingerprints artifacts by the absolute path of the
/// package. A crate compiled out of a fresh random directory is never a
/// cache hit, however identical its bytes are to the ones the last run
/// compiled; every run would rebuild the whole workspace from scratch.
///
/// The argument is the absolute source root exactly as spelled: no symlink
/// resolution and no case folding. Two spellings of one directory therefore
/// get two names, which costs the second spelling a first run's cache hits
/// and nothing else — while canonicalizing would mean deciding what to do on
/// the day the answer changes underneath a directory that already exists.
#[must_use]
pub fn stable_name(abs_source_root: &Path) -> String {
    let digest = Sha256::digest(abs_source_root.as_os_str().as_encoded_bytes());
    let hex = hex::encode(digest);
    let prefix = hex.get(..STABLE_NAME_HEX_LENGTH).unwrap_or(&hex);
    format!("{DIR_PREFIX}{prefix}")
}

/// Creates the directory a snapshot of `abs_src` will own inside `parent`,
/// and reports whether it got the stable name or a fresh one.
fn destination(
    parent: &Path,
    abs_src: &Path,
    now: Timestamp,
) -> Result<(PathBuf, bool), SnapshotError> {
    if !parent.is_absolute() {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InvalidOptions,
            parent.display().to_string(),
            "destination parent must be an absolute path",
        ));
    }
    let name = stable_name(abs_src);
    let dir = parent.join(&name);
    match fs::create_dir(&dir) {
        Ok(()) => return Ok((dir, true)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(SnapshotError::new(
                SnapshotErrorKind::Destination,
                dir.display().to_string(),
                "cannot create the snapshot directory",
            )
            .with_source(source));
        }
    }
    // Something is already using the name, and the sweep already knows every
    // case of what that can mean: a locked directory belongs to a run that is
    // still going, a kept one was preserved on purpose, a released lock means
    // the owner is gone and the directory is an orphan to remove, and an
    // unowned young one is spared. Its result is dropped: a directory that
    // refused to go is a reason to take another name, not to fail a run that
    // has not copied a byte yet.
    drop(tempowner::sweep(parent, &[name.as_str()], now));
    if fs::create_dir(&dir).is_ok() {
        return Ok((dir, true));
    }
    fallback_destination(parent, abs_src, now).map(|dir| (dir, false))
}

/// A fresh name under `parent`: [`DIR_PREFIX`] plus sixteen hex characters
/// that no other directory carries, proven by the exclusive `create_dir`.
fn fallback_destination(
    parent: &Path,
    abs_src: &Path,
    now: Timestamp,
) -> Result<PathBuf, SnapshotError> {
    for attempt in 0..FALLBACK_ATTEMPTS {
        let mut hasher = Sha256::new();
        hasher.update(abs_src.as_os_str().as_encoded_bytes());
        hasher.update(now.as_nanosecond().to_be_bytes());
        hasher.update(std::process::id().to_be_bytes());
        hasher.update(attempt.to_be_bytes());
        let hex = hex::encode(hasher.finalize());
        let suffix = hex.get(..STABLE_NAME_HEX_LENGTH).unwrap_or(&hex);
        let dir = parent.join(format!("{DIR_PREFIX}{suffix}"));
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::Destination,
                    dir.display().to_string(),
                    "cannot create the snapshot directory",
                )
                .with_source(source));
            }
        }
    }
    Err(SnapshotError::new(
        SnapshotErrorKind::Destination,
        parent.display().to_string(),
        format!("no free snapshot directory name after {FALLBACK_ATTEMPTS} attempts"),
    ))
}

/// Takes ownership of the directory [`destination`] just made, and decides
/// what happens to that directory when it cannot.
///
/// A claim that lost to another process's lock is left exactly as found: a
/// run stopped between its `create_dir` and this claim for longer than the
/// legacy window would find, on resuming, that another run of the same root
/// had swept the empty directory, recreated it and claimed it. The directory
/// is that run's now, and removing it would remove a live snapshot. Every
/// other failure happens inside a directory no other process has entered,
/// and the empty directory is removed so that a failed `create` leaves
/// nothing behind.
fn claim_destination(dir: &Path, now: Timestamp) -> Result<Owner, SnapshotError> {
    match tempowner::claim(dir, now) {
        Ok(owner) => Ok(owner),
        Err(error) => {
            if !matches!(error, ClaimError::Owned { .. }) {
                drop(fs::remove_dir_all(dir));
            }
            Err(SnapshotError::new(
                SnapshotErrorKind::Destination,
                dir.display().to_string(),
                "cannot claim the snapshot directory",
            )
            .with_source(io::Error::other(error)))
        }
    }
}

impl Snapshot {
    /// The absolute path of the tree that was copied.
    #[must_use]
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    /// The absolute path of the copy. Everything downstream — the build, the
    /// test binaries' working directories, the instrumented rewrites — happens
    /// under here. It is [`TREE_NAME`] inside [`Snapshot::dir`].
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory this snapshot owns: [`Snapshot::root`] and the ownership
    /// files live in it, and [`Snapshot::cleanup`] removes it whole.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory [`Snapshot::dir`] was created in. It is the answer to
    /// "where does a sibling of this snapshot belong", which the scratch
    /// directories beside it have to ask.
    #[must_use]
    pub fn parent(&self) -> &Path {
        &self.dest_parent
    }

    /// Every regular file in the snapshot, sorted by path. Directories are
    /// not listed; they are recreated faithfully but contribute nothing a
    /// build can observe.
    #[must_use]
    pub fn manifest(&self) -> &[Entry] {
        &self.manifest
    }

    /// The frozen digest of the manifest; see the module documentation.
    #[must_use]
    pub fn workspace_digest(&self) -> &str {
        &self.workspace_digest
    }

    /// Whether the directory carries the [`stable_name`] of the source root
    /// rather than a fresh fallback name.
    ///
    /// It is false when the stable name was held by a directory [`create`]
    /// was not allowed to remove. The snapshot is a perfectly good snapshot
    /// either way — the only thing lost is the build-cache hits the stable
    /// path buys — so the case is reported rather than refused, for a caller
    /// that wants to say why a run compiled everything from scratch.
    #[must_use]
    pub const fn stable_dir(&self) -> bool {
        self.stable_dir
    }

    /// Whether [`Snapshot::keep`] preserved the directory.
    #[must_use]
    pub fn kept(&self) -> bool {
        self.state == State::Kept
    }

    /// Re-walks the snapshot and reports every way it no longer matches the
    /// manifest, sorted by path.
    ///
    /// This is the gate behind a specific hazard. All workers share one
    /// snapshot, so a test that writes into its own package directory — a
    /// golden file it "updates", a database it creates in `testdata` —
    /// corrupts the tree that every later mutant is tested against, and the
    /// run's results quietly become unreproducible. Running this after the
    /// instrumented baseline turns that into a named list of files.
    ///
    /// The walk applies no exclusions: every byte under the root is ours, and
    /// something appearing inside a directory that would have been excluded
    /// on the way in is exactly the surprise worth reporting. A link or a
    /// device found in the snapshot is an error, not a drift: the tree has
    /// grown a shape this module refuses to reason about.
    ///
    /// # Errors
    ///
    /// The walk failures and refusals of [`create`].
    pub fn redigest(&self) -> Result<Vec<Drift>, SnapshotError> {
        let mut walker = Walker::new(&self.root, &[]);
        walker.walk("")?;
        walker.rejection()?;
        let recorded: std::collections::BTreeMap<&str, &Entry> = self
            .manifest
            .iter()
            .map(|entry| (entry.rel_path.as_str(), entry))
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        let mut drifts = Vec::new();
        for file in &walker.files {
            seen.insert(file.rel.as_str());
            let (size, sha256) = hash_file(&file.abs).map_err(|source| {
                SnapshotError::new(
                    SnapshotErrorKind::Walk,
                    file.rel.clone(),
                    "cannot read the file in the snapshot",
                )
                .with_source(source)
            })?;
            let got = Fingerprint { size, sha256 };
            match recorded.get(file.rel.as_str()) {
                None => drifts.push(Drift::Added {
                    rel_path: file.rel.clone(),
                    got,
                }),
                Some(entry) if entry.sha256 != got.sha256 || entry.size != got.size => {
                    drifts.push(Drift::Changed {
                        rel_path: file.rel.clone(),
                        want: entry.fingerprint(),
                        got,
                    });
                }
                Some(_) => {}
            }
        }
        for entry in &self.manifest {
            if !seen.contains(entry.rel_path.as_str()) {
                drifts.push(Drift::Removed {
                    rel_path: entry.rel_path.clone(),
                    want: entry.fingerprint(),
                });
            }
        }
        drifts.sort_by(|a, b| a.rel_path().cmp(b.rel_path()));
        Ok(drifts)
    }

    /// Replaces the manifest with the tree as it stands now, and reports
    /// what that absorbed.
    ///
    /// Instrumentation rewrites the snapshot on purpose, so the manifest
    /// [`create`] recorded stops describing it the moment the guards are
    /// written. Resealing is how the intended rewrite stops being drift, so
    /// that [`Snapshot::redigest`] afterwards means what it is for: a test
    /// wrote into the tree every later mutant is measured against.
    ///
    /// # Errors
    ///
    /// The walk failures and refusals of [`create`].
    pub fn reseal(&mut self) -> Result<Vec<Drift>, SnapshotError> {
        let absorbed = self.redigest()?;
        let mut walker = Walker::new(&self.root, &[]);
        walker.walk("")?;
        walker.rejection()?;
        let mut manifest = Vec::with_capacity(walker.files.len());
        for file in &walker.files {
            let (size, sha256) = hash_file(&file.abs).map_err(|source| {
                SnapshotError::new(
                    SnapshotErrorKind::Walk,
                    file.rel.clone(),
                    "cannot read the file in the snapshot",
                )
                .with_source(source)
            })?;
            manifest.push(Entry {
                rel_path: file.rel.clone(),
                size,
                sha256,
            });
        }
        manifest.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        self.workspace_digest = workspace_digest(&manifest);
        self.manifest = manifest;
        Ok(absorbed)
    }

    /// Preserves the directory instead of removing it, and records in the
    /// owner marker that this was asked for.
    ///
    /// The record is the whole point. A directory left behind with a lock
    /// nobody holds is indistinguishable from one a killed process
    /// abandoned, and the next run's sweep would collect it — so a keep that
    /// did not say so in a way the sweep obeys would be a keep in name only.
    /// A kept snapshot stays kept: [`Snapshot::cleanup`] and `Drop` become
    /// no-ops afterwards.
    ///
    /// # Errors
    ///
    /// A keep the marker did not record is not a keep: the error is returned
    /// and the snapshot stays removable.
    pub fn keep(&mut self) -> Result<(), SnapshotError> {
        if let Some(owner) = &mut self.owner {
            owner.keep().map_err(|error| {
                SnapshotError::new(
                    SnapshotErrorKind::CleanupFailed,
                    self.dir.display().to_string(),
                    "cannot mark the snapshot directory kept",
                )
                .with_source(io::Error::other(error))
            })?;
        }
        self.state = State::Kept;
        Ok(())
    }

    /// Removes the snapshot directory.
    ///
    /// Removal is retried because of Windows. A test binary that has just
    /// exited can still have its image mapped for a moment, a scanner can
    /// hold a handle open, and a file another process opened without
    /// `FILE_SHARE_DELETE` cannot be unlinked until that handle closes. Every
    /// one of those clears on its own within milliseconds, so a short backoff
    /// ladder turns a spurious failure into a pause. Before each retry the
    /// read-only attribute is cleared from the tree, which is the one cause
    /// that would otherwise never clear by waiting.
    ///
    /// After a keep this is a no-op that succeeds.
    ///
    /// # The guard
    ///
    /// This function deletes a directory tree from a path stored in a field
    /// anything in this process could have written. So it refuses unless the
    /// path still looks exactly like something [`create`] produced; see
    /// [`cleanup_guard`]. The guard is not defence against an attacker. It is
    /// defence against rust-mutants — against a refactor that assigns the
    /// workspace root to the wrong field. Deleting a user's source tree is
    /// the one bug this tool must never have.
    ///
    /// # Errors
    ///
    /// [`SnapshotErrorKind::CleanupRefused`] when the guard fires, and
    /// [`SnapshotErrorKind::CleanupFailed`] when the directory survived every
    /// attempt or its lock could not be released.
    pub fn cleanup(self) -> Result<(), SnapshotError> {
        self.cleanup_with(&|dir: &Path| fs::remove_dir_all(dir), &std::thread::sleep)
    }

    /// [`Snapshot::cleanup`] with the removal and the pause as arguments, so
    /// the retry ladder can be tested without a filesystem persuaded into
    /// failing.
    ///
    /// # Errors
    ///
    /// See [`Snapshot::cleanup`].
    pub fn cleanup_with(
        mut self,
        remove: &dyn Fn(&Path) -> io::Result<()>,
        sleep: &dyn Fn(Duration),
    ) -> Result<(), SnapshotError> {
        if self.state != State::Live {
            return Ok(());
        }
        // Whatever happens next, the caller is told about it; `Drop` must not
        // second-guess the answer with a second ladder.
        self.state = State::Released;
        self.remove(remove, sleep)
    }

    fn remove(
        &mut self,
        remove: &dyn Fn(&Path) -> io::Result<()>,
        sleep: &dyn Fn(Duration),
    ) -> Result<(), SnapshotError> {
        cleanup_guard(&self.dir, &self.dest_parent)?;
        // The lock is released before the first attempt, not after the last:
        // on Windows an open handle inside a directory is what makes the
        // removal fail, so a cleanup holding its own lock would spend the
        // whole ladder losing to itself.
        if let Some(owner) = &mut self.owner {
            owner.release().map_err(|source| {
                SnapshotError::new(
                    SnapshotErrorKind::CleanupFailed,
                    self.dir.display().to_string(),
                    "cannot release the snapshot directory's lock",
                )
                .with_source(source)
            })?;
        }
        let mut last = None;
        for attempt in 0..CLEANUP_ATTEMPTS {
            if let Some(exponent) = attempt.checked_sub(1) {
                platform::clear_read_only(&self.dir);
                let exponent = u32::try_from(exponent).unwrap_or(u32::MAX);
                sleep(
                    CLEANUP_BACKOFF.saturating_mul(1u32.checked_shl(exponent).unwrap_or(u32::MAX)),
                );
            }
            match remove(&self.dir) {
                Ok(()) => return Ok(()),
                Err(error) => last = Some(error),
            }
        }
        let mut error = SnapshotError::new(
            SnapshotErrorKind::CleanupFailed,
            self.dir.display().to_string(),
            format!("the snapshot directory survived {CLEANUP_ATTEMPTS} removal attempts"),
        );
        if let Some(source) = last {
            error = error.with_source(source);
        }
        Err(error)
    }
}

impl Drop for Snapshot {
    /// The deferred cleanup: best effort, errors dropped, nothing after a keep
    /// or an explicit cleanup.
    fn drop(&mut self) {
        if self.state == State::Live {
            self.state = State::Released;
            drop(self.remove(&|dir: &Path| fs::remove_dir_all(dir), &std::thread::sleep));
        }
    }
}

/// Reports why `dir` is not safe to delete as a snapshot directory, or `Ok`
/// if it is.
///
/// It must be non-empty and absolute, its final element must begin with
/// [`DIR_PREFIX`], and its parent must be `dest_parent`, the directory the
/// snapshot was created in.
///
/// # Errors
///
/// [`SnapshotErrorKind::CleanupRefused`] naming the failed condition.
pub fn cleanup_guard(dir: &Path, dest_parent: &Path) -> Result<(), SnapshotError> {
    let refuse = |reason: &str| {
        Err(SnapshotError::new(
            SnapshotErrorKind::CleanupRefused,
            dir.display().to_string(),
            format!("refuses to remove a path that is not a snapshot directory: {reason}"),
        ))
    };
    if dir.as_os_str().is_empty() {
        return refuse("the directory is empty");
    }
    if !dir.is_absolute() {
        return refuse("the directory is not absolute");
    }
    let named = dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(DIR_PREFIX));
    if !named {
        return refuse(&format!("the name does not begin with {DIR_PREFIX}"));
    }
    let parent_matches = dir
        .parent()
        .is_some_and(|parent| platform::paths_equal(parent, dest_parent));
    if !parent_matches {
        return refuse("the parent is not the destination parent");
    }
    Ok(())
}

#[cfg(unix)]
mod platform {
    //! POSIX: permission bits are propagated exactly, and there is no
    //! read-only attribute or reparse point to speak of.

    use std::fs::{self, File, Metadata, OpenOptions};
    use std::io;
    use std::os::unix::fs::{FileTypeExt as _, OpenOptionsExt as _, PermissionsExt as _};
    use std::path::Path;

    /// POSIX has no reparse points: a link is a link and everything else
    /// irregular, and the walk itself tells them apart.
    pub(super) const fn is_reparse_point(_: &Metadata) -> bool {
        false
    }

    /// Names an irregular file's type for the refusal message.
    pub(super) fn describe(meta: &Metadata) -> &'static str {
        let file_type = meta.file_type();
        if file_type.is_socket() {
            "a socket"
        } else if file_type.is_fifo() {
            "a named pipe"
        } else if file_type.is_block_device() {
            "a block device"
        } else if file_type.is_char_device() {
            "a character device"
        } else {
            "an unknown file type"
        }
    }

    /// Opens the destination exclusively with the source's permission bits
    /// as the creation mode, which the umask may still lower.
    pub(super) fn create_exclusive(dst: &Path, meta: &Metadata) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(meta.permissions().mode() & 0o777)
            .open(dst)
    }

    /// Sets the copied file's permissions to the source's exactly. It goes
    /// through the descriptor rather than the path so the bits land on the
    /// file that was just written, whatever has happened to the name.
    pub(super) fn finalize_file_permissions(file: &File, meta: &Metadata) -> io::Result<()> {
        file.set_permissions(fs::Permissions::from_mode(
            meta.permissions().mode() & 0o777,
        ))
    }

    /// Sets a copied directory's permissions to the source's, forced to
    /// owner rwx so the copy can be instrumented.
    pub(super) fn finalize_dir_permissions(path: &Path, meta: &Metadata) -> io::Result<()> {
        fs::set_permissions(
            path,
            fs::Permissions::from_mode((meta.permissions().mode() & 0o777) | 0o700),
        )
    }

    /// Nothing to clear: removing a file depends on the containing
    /// directory's permissions rather than the file's own.
    pub(super) const fn clear_read_only(_: &Path) {}

    /// Whether two paths name the same directory, for the cleanup guard.
    /// A comparison of spellings and not of inodes on purpose: the guard asks
    /// whether the directory is still the path `create` produced, and a
    /// symlink since pointed at it is not an answer of yes.
    pub(super) fn paths_equal(a: &Path, b: &Path) -> bool {
        !a.as_os_str().is_empty() && a.components().eq(b.components())
    }
}

#[cfg(windows)]
mod platform {
    //! Windows: no permission bits to preserve, a read-only attribute that
    //! blocks removal, and reparse points beyond symbolic links.

    use std::fs::{self, File, Metadata, OpenOptions};
    use std::io;
    use std::os::windows::fs::MetadataExt as _;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    /// Whether the entry is a reparse point of any kind. Symbolic links and
    /// junctions are caught by `is_symlink` first; this is for every other
    /// name surrogate.
    pub(super) fn is_reparse_point(meta: &Metadata) -> bool {
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    pub(super) const fn describe(_: &Metadata) -> &'static str {
        "an unknown file type"
    }

    pub(super) fn create_exclusive(dst: &Path, _: &Metadata) -> io::Result<File> {
        OpenOptions::new().write(true).create_new(true).open(dst)
    }

    /// Windows has no permission bits worth copying; the read-only attribute
    /// is deliberately not propagated, since the copy must be instrumentable.
    pub(super) const fn finalize_file_permissions(_: &File, _: &Metadata) -> io::Result<()> {
        Ok(())
    }

    pub(super) const fn finalize_dir_permissions(_: &Path, _: &Metadata) -> io::Result<()> {
        Ok(())
    }

    /// Clears the read-only attribute from every file under `dir`, which is
    /// the one removal failure that does not clear by waiting. Best effort:
    /// the removal that follows reports what is still in the way.
    pub(super) fn clear_read_only(dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                clear_read_only(&path);
            }
            let mut permissions = meta.permissions();
            if permissions.readonly() {
                #[allow(
                    clippy::permissions_set_readonly_false,
                    reason = "Windows has no group or world bits to open; this only clears the attribute"
                )]
                permissions.set_readonly(false);
                drop(fs::set_permissions(&path, permissions));
            }
        }
    }

    /// Whether two paths name the same directory, ignoring case as the
    /// filesystem does.
    pub(super) fn paths_equal(a: &Path, b: &Path) -> bool {
        if a.as_os_str().is_empty() || b.as_os_str().is_empty() {
            return false;
        }
        let fold = |path: &Path| -> Vec<String> {
            path.components()
                .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
                .collect()
        };
        fold(a) == fold(b)
    }
}
