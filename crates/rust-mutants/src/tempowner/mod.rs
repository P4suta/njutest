// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every temporary directory has an owner and a collector (ADR 0006).

mod lock;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

pub use lock::{Lock, acquire};

/// The marker's schema field.
/// It carries the version, so a later document shape can never be read as this one.
pub const SCHEMA: &str = "rust-mutants-temp-owner-v1";
/// The advisory lock file inside a claimed directory.
pub const LOCK_NAME: &str = "owner.lock";
/// The JSON marker file inside a claimed directory.
pub const MARKER_NAME: &str = "owner.json";
/// How long an unowned directory must have been untouched before [`sweep`] treats it as a leftover.
pub const LEGACY_MAX_AGE: Duration = Duration::from_hours(24);

/// What a claimed directory is for.
/// A scratch belongs to one run and goes away with it; a cache is meant to outlive the run that filled it, which is what makes a second run fast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    /// One run's working tree.
    /// A sweep reclaims it as soon as nobody holds its lock.
    Scratch,
    /// A build cache.
    /// A sweep spares it however old it is; only a caller that asks for it by name reclaims it.
    Cache,
}

/// The JSON document in a claimed directory.
/// Written once at creation and rewritten only to record a deliberate keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// [`SCHEMA`].
    pub schema: String,
    /// The process that claimed the directory.
    /// Diagnostic only: liveness is the lock's job.
    pub pid: u32,
    /// When the directory was claimed, in UTC.
    pub started: Timestamp,
    /// Whether the directory was preserved on purpose and is not an orphan.
    pub kept: bool,
    /// What the directory is for.
    /// Absent in a marker written before roles existed, which means a scratch.
    #[serde(default = "scratch_role")]
    pub role: Role,
    /// The tree a cache is keyed to, so a sweep can tell a cache a run will look up from one nothing can name again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyed_to: Option<String>,
}

/// The meaning of an absent role in the historical v1 marker shape.
const fn scratch_role() -> Role {
    Role::Scratch
}

/// The lock file inside `dir`.
#[must_use]
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK_NAME)
}

/// The marker file inside `dir`.
#[must_use]
pub fn marker_path(dir: &Path) -> PathBuf {
    dir.join(MARKER_NAME)
}

/// Why a directory could not be claimed.
#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    /// The directory's lock is held by another process: it is theirs, not the caller's to remove.
    #[error("{dir} is already owned by another process")]
    Owned {
        /// The directory.
        dir: PathBuf,
    },
    /// The lock file could not be opened or locked.
    #[error("locking {dir}: {source}")]
    Lock {
        /// The directory.
        dir: PathBuf,
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// The marker could not be written.
    #[error("marking {dir}: {source}")]
    Marker {
        /// The directory.
        dir: PathBuf,
        /// The failure.
        #[source]
        source: io::Error,
    },
}

/// A claimed directory: the lock is held open and the marker is written.
/// Releasing or keeping it closes the lock; neither removes anything.
#[derive(Debug)]
pub struct Owner {
    dir: PathBuf,
    lock: Option<Lock>,
    marker: Marker,
}

/// Writes the marker pair into an existing directory and takes its lock.
///
/// # Errors
/// Returns [`ClaimError::Owned`] when another process holds the lock, and the I/O failure otherwise.
pub fn claim(dir: &Path, now: Timestamp) -> Result<Owner, ClaimError> {
    claim_as(dir, now, SCHEMA)
}

/// [`claim`], with the marker naming the program that wrote it.
///
/// # Errors
/// Those of [`claim`].
pub fn claim_as(dir: &Path, now: Timestamp, schema: &str) -> Result<Owner, ClaimError> {
    claim_with(
        dir,
        now,
        Claiming {
            schema,
            role: Role::Scratch,
            keyed_to: None,
        },
    )
}

/// [`claim_as`] for a build cache: a directory a sweep spares however old it is, because the next run wants what is in it.
///
/// # Errors
/// Those of [`claim`].
pub fn claim_cache(dir: &Path, now: Timestamp, schema: &str) -> Result<Owner, ClaimError> {
    claim_with(
        dir,
        now,
        Claiming {
            schema,
            role: Role::Cache,
            keyed_to: None,
        },
    )
}

/// [`claim_cache`] for a cache keyed to a tree, which is what lets a sweep collect it once the tree is gone.
///
/// # Errors
/// Those of [`claim`].
pub fn claim_cache_of(
    dir: &Path,
    now: Timestamp,
    schema: &str,
    keyed_to: &Path,
) -> Result<Owner, ClaimError> {
    claim_with(
        dir,
        now,
        Claiming {
            schema,
            role: Role::Cache,
            keyed_to: Some(keyed_to.display().to_string()),
        },
    )
}

/// What a claim writes into the marker beside the lock.
struct Claiming<'a> {
    /// The schema the marker names, which says which program wrote it.
    schema: &'a str,
    /// What the directory is for.
    role: Role,
    /// The tree a cache is keyed to, where it is one.
    keyed_to: Option<String>,
}

fn claim_with(dir: &Path, now: Timestamp, claiming: Claiming<'_>) -> Result<Owner, ClaimError> {
    let Claiming {
        schema,
        role,
        keyed_to,
    } = claiming;
    let lock = acquire(&lock_path(dir)).map_err(|source| ClaimError::Lock {
        dir: dir.to_path_buf(),
        source,
    })?;
    let Some(lock) = lock else {
        return Err(ClaimError::Owned {
            dir: dir.to_path_buf(),
        });
    };
    let marker = Marker {
        schema: schema.to_owned(),
        pid: std::process::id(),
        started: now,
        kept: false,
        role,
        keyed_to,
    };
    if let Err(source) = write_marker(dir, &marker) {
        drop(lock);
        return Err(ClaimError::Marker {
            dir: dir.to_path_buf(),
            source,
        });
    }
    Ok(Owner {
        dir: dir.to_path_buf(),
        lock: Some(lock),
        marker,
    })
}

fn write_marker(dir: &Path, marker: &Marker) -> io::Result<()> {
    let mut raw = serde_json::to_vec(marker).map_err(io::Error::other)?;
    raw.push(b'\n');
    let path = marker_path(dir);
    fs::write(&path, raw)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

impl Owner {
    /// The claimed directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The marker as written.
    #[must_use]
    pub const fn marker(&self) -> &Marker {
        &self.marker
    }

    /// Closes the lock without touching the directory.
    /// Idempotent, and it must be called before the directory is removed: on Windows an open handle inside a directory is what makes the removal fail.
    ///
    /// # Errors
    /// Returns the unlock or close failure.
    pub fn release(&mut self) -> io::Result<()> {
        match self.lock.take() {
            Some(mut lock) => lock.release(),
            None => Ok(()),
        }
    }

    /// Removes everything in the directory but its lock and marker, while the lock is held.
    ///
    /// # Errors
    /// An entry could not be removed.
    pub fn empty(&self) -> io::Result<()> {
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == LOCK_NAME || name == MARKER_NAME {
                continue;
            }
            if entry.file_type()?.is_dir() {
                remove_tree(&entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    /// Records that the directory was preserved on purpose and releases the lock, so that a later [`sweep`] reads the marker rather than finding a lock nobody holds and concluding the directory was abandoned.
    ///
    /// # Errors
    /// Returns the marker write failure; the lock is released either way.
    pub fn keep(&mut self) -> Result<(), ClaimError> {
        let mut marker = self.marker.clone();
        marker.kept = true;
        let written = write_marker(&self.dir, &marker);
        let released = self.release();
        written.map_err(|source| ClaimError::Marker {
            dir: self.dir.clone(),
            source,
        })?;
        self.marker = marker;
        released.map_err(|source| ClaimError::Lock {
            dir: self.dir.clone(),
            source,
        })
    }
}

/// Why a marker could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MarkerError {
    /// The marker file does not exist.
    #[error("{dir} has no owner marker")]
    Missing {
        /// The directory.
        dir: PathBuf,
    },
    /// The marker could not be read.
    #[error("reading the marker of {dir}: {source}")]
    Io {
        /// The directory.
        dir: PathBuf,
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// The marker is not a document this version understands.
    #[error("the marker of {dir} is malformed: {source}")]
    Malformed {
        /// The directory.
        dir: PathBuf,
        /// The failure.
        #[source]
        source: serde_json::Error,
    },
}

/// Removes a tree this process's user owns, giving the owner back the access a test took away from a directory of it before giving up on it.
///
/// A mutation makes tests panic, and a panicking test is the one that leaves a directory it made read-only; the run's own cleanup must not stop there.
/// Links are removed as links and never followed, so nothing outside the tree is touched.
///
/// # Errors
/// The tree could not be removed even with its owner's access restored.
pub fn remove_tree(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            restore_owner_access(dir)?;
            fs::remove_dir_all(dir)
        }
        Err(error) => Err(error),
    }
}

/// Gives the owner back full access to every directory in a tree, without following a link.
/// Only a Unix mode takes that access away; a Windows read-only attribute on a directory does not stop its entries being removed.
fn restore_owner_access(dir: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(dir)?;
    if !metadata.file_type().is_dir() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o700);
        fs::set_permissions(dir, permissions)?;
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            restore_owner_access(&entry.path())?;
        }
    }
    Ok(())
}

/// Decodes the marker in `dir`.
///
/// # Errors
/// Returns a missing, unreadable, or malformed marker.
pub fn read_marker(dir: &Path) -> Result<Marker, MarkerError> {
    let raw = match fs::read(marker_path(dir)) {
        Ok(raw) => raw,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(MarkerError::Missing {
                dir: dir.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(MarkerError::Io {
                dir: dir.to_path_buf(),
                source,
            });
        }
    };
    crate::strictjson::decode_slice(&raw).map_err(|source| MarkerError::Malformed {
        dir: dir.to_path_buf(),
        source,
    })
}

/// One directory a sweep could not judge or remove.
#[derive(Debug)]
pub struct SweepFailure {
    /// The directory.
    pub dir: PathBuf,
    /// What went wrong.
    pub source: io::Error,
}

/// What one [`sweep`] did.
/// Diagnostic: no report, no schema, and no exit code depends on it, because collecting somebody else's leftovers is housekeeping a run does on the way.
#[derive(Debug, Default)]
pub struct SweepResult {
    /// The absolute path of every directory the sweep deleted.
    pub removed: Vec<PathBuf>,
    /// What they held, as far as the walk could measure.
    pub removed_bytes: u64,
    /// How many directories were still locked by a running process.
    pub live: usize,
    /// How many were preserved on purpose.
    pub kept: usize,
    /// How many are build caches, which a sweep spares.
    pub cached: usize,
    /// The directories that could not be judged or removed.
    /// A failure does not stop the sweep of the others.
    pub failures: Vec<SweepFailure>,
    /// How many prefixed directories the sweep never reached, because it had spent its budget.
    pub unreached: usize,
}

/// How long a sweep spends before it leaves the rest for the next one.
pub const SWEEP_BUDGET: Duration = Duration::from_secs(10);

/// Removes every abandoned directory directly under `parent` whose name begins with one of `prefixes`.
///
/// # Errors
/// Returns the failure to read `parent` itself.
pub fn sweep(parent: &Path, prefixes: &[&str], now: Timestamp) -> io::Result<SweepResult> {
    sweep_with(parent, prefixes, now, &|dir: &Path| remove_tree(dir))
}

/// Removes every unlocked directory under `parent` whose name is prefixed, caches included.
///
/// # Errors
/// Returns the failure to read `parent` itself.
pub fn reclaim(parent: &Path, prefixes: &[&str], now: Timestamp) -> io::Result<SweepResult> {
    reclaim_with(parent, prefixes, now, &|dir: &Path| remove_tree(dir))
}

/// [`reclaim`] with its removal operation as an argument.
///
/// # Errors
/// See [`reclaim`].
pub fn reclaim_with(
    parent: &Path,
    prefixes: &[&str],
    now: Timestamp,
    remove: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<SweepResult> {
    collect(
        parent,
        &Pass {
            prefixes,
            now,
            remove,
            caches_too: true,
        },
    )
}

/// [`sweep`] with its removal operation as an argument, so the "one directory refuses to go" case can be tested without a filesystem persuaded into failing.
///
/// # Errors
/// See [`sweep`].
pub fn sweep_with(
    parent: &Path,
    prefixes: &[&str],
    now: Timestamp,
    remove: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<SweepResult> {
    collect(
        parent,
        &Pass {
            prefixes,
            now,
            remove,
            caches_too: false,
        },
    )
}

/// What one pass over the temporary directory looks for.
///
/// A failure about one directory is recorded against that directory and the pass goes on.
/// Propagating it would throw away everything the pass had already established — every directory removed, every byte counted, every other failure — and answer with the temporary root's name, which is not the directory that refused.
struct Pass<'a> {
    prefixes: &'a [&'a str],
    now: Timestamp,
    remove: &'a dyn Fn(&Path) -> io::Result<()>,
    /// Whether a build cache nobody holds counts as reclaimable.
    caches_too: bool,
}

fn collect(parent: &Path, pass: &Pass<'_>) -> io::Result<SweepResult> {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(SweepResult::default()),
        Err(error) => return Err(error),
    };
    let mut result = SweepResult::default();
    let started = std::time::Instant::now();
    let collecting = Collecting {
        parent,
        pass,
        started: &started,
    };
    for entry in entries {
        collect_entry(entry, &collecting, &mut result)?;
    }
    Ok(result)
}

struct Collecting<'a, 'pass> {
    parent: &'a Path,
    pass: &'a Pass<'pass>,
    started: &'a std::time::Instant,
}

fn collect_entry(
    entry: io::Result<fs::DirEntry>,
    collecting: &Collecting<'_, '_>,
    result: &mut SweepResult,
) -> io::Result<()> {
    let parent = collecting.parent;
    let pass = collecting.pass;
    let started = collecting.started;
    let entry = match entry {
        Ok(entry) => entry,
        Err(source) => {
            result.failures.push(SweepFailure {
                dir: parent.to_path_buf(),
                source,
            });
            return Ok(());
        }
    };
    let name = entry.file_name();
    let is_prefixed = name.to_str().is_some_and(|name| {
        pass.prefixes
            .iter()
            .any(|prefix| !prefix.is_empty() && name.starts_with(prefix))
    });
    if !is_prefixed {
        return Ok(());
    }
    let entry_type = match entry.file_type() {
        Ok(entry_type) => entry_type,
        Err(source) => {
            result.failures.push(SweepFailure {
                dir: entry.path(),
                source,
            });
            return Ok(());
        }
    };
    if !entry_type.is_dir() {
        return Ok(());
    }
    if started.elapsed() >= SWEEP_BUDGET {
        result.unreached = checked_count(result.unreached, "unreached directories")?;
        return Ok(());
    }
    let dir = parent.join(name);
    let verdict = cache_verdict(judge(&dir, &entry, pass.now), &dir, pass.caches_too);
    record_verdict(result, verdict, dir, pass.remove)
}

fn cache_verdict(
    verdict: io::Result<Verdict>,
    dir: &Path,
    caches_too: bool,
) -> io::Result<Verdict> {
    match verdict {
        Ok(Verdict::Cache) if caches_too => match acquire(&lock_path(dir)) {
            Ok(None) => Ok(Verdict::Live),
            Ok(Some(mut lock)) => match lock.release() {
                Ok(()) => Ok(Verdict::Abandoned),
                Err(source) => Err(source),
            },
            Err(source) => Err(source),
        },
        other => other,
    }
}

fn record_verdict(
    result: &mut SweepResult,
    verdict: io::Result<Verdict>,
    dir: PathBuf,
    remove: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    match verdict {
        Ok(Verdict::Live) => result.live = checked_count(result.live, "live directories")?,
        Ok(Verdict::Kept) => result.kept = checked_count(result.kept, "kept directories")?,
        Ok(Verdict::Cache) => {
            result.cached = checked_count(result.cached, "cached directories")?;
        }
        Ok(Verdict::Spared) => {}
        Ok(Verdict::Abandoned) => remove_abandoned(result, dir, remove)?,
        Err(source) => result.failures.push(SweepFailure { dir, source }),
    }
    Ok(())
}

fn remove_abandoned(
    result: &mut SweepResult,
    dir: PathBuf,
    remove: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let size = match directory_size(&dir) {
        Ok(size) => size,
        Err(source) => {
            result.failures.push(SweepFailure { dir, source });
            return Ok(());
        }
    };
    match remove(&dir) {
        Ok(()) => {
            result.removed.push(dir);
            result.removed_bytes = result
                .removed_bytes
                .checked_add(size)
                .ok_or_else(|| io::Error::other("removed-byte accounting overflowed"))?;
        }
        Err(source) => result.failures.push(SweepFailure { dir, source }),
    }
    Ok(())
}

fn checked_count(count: usize, subject: &str) -> io::Result<usize> {
    count
        .checked_add(1)
        .ok_or_else(|| io::Error::other(format!("{subject} count overflowed")))
}

/// What the sweep decided about one directory.
/// Only `Abandoned` removes.
enum Verdict {
    Abandoned,
    /// Somebody holds the lock.
    Live,
    /// The marker says it was preserved.
    Kept,
    /// The marker says it is a build cache, which outlives the run that filled it.
    Cache,
    /// Left alone without being counted: an unowned directory too young to judge.
    /// Not a fact about a live owner, so not a number in the result.
    Spared,
}

/// A marker that cannot be read at all is treated as a marker that does not say kept, deliberately: the lock has already answered the only question that matters, and a half-written marker must not make a dead directory immortal.
fn judge(dir: &Path, entry: &fs::DirEntry, now: Timestamp) -> io::Result<Verdict> {
    match read_marker(dir) {
        Ok(marker) if marker.kept => return Ok(Verdict::Kept),
        Ok(marker) if marker.role == Role::Cache => {
            if !orphaned(marker.keyed_to.as_deref())? {
                return Ok(Verdict::Cache);
            }
        }
        Err(MarkerError::Missing { .. }) => return legacy(entry, now),
        Ok(_) | Err(_) => {}
    }
    match acquire(&lock_path(dir))? {
        None => Ok(Verdict::Live),
        Some(mut lock) => {
            lock.release()?;
            Ok(Verdict::Abandoned)
        }
    }
}

/// Whether a cache is keyed to a tree that is no longer there.
fn orphaned(keyed_to: Option<&str>) -> io::Result<bool> {
    let Some(tree) = keyed_to else {
        return Ok(false);
    };
    match fs::symlink_metadata(Path::new(tree)) {
        Ok(_metadata) => Ok(false),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(source) => Err(source),
    }
}

/// A directory with no marker at all: one created before this convention, or one whose marker was lost.
/// Age is the only evidence there is, and a young one is left alone because it may be a run in progress.
fn legacy(entry: &fs::DirEntry, now: Timestamp) -> io::Result<Verdict> {
    let modified = match entry.metadata() {
        Ok(metadata) => metadata.modified()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Verdict::Spared),
        Err(error) => return Err(error),
    };
    let modified = Timestamp::try_from(modified).map_err(io::Error::other)?;
    let Some(age) = now.as_second().checked_sub(modified.as_second()) else {
        return Ok(Verdict::Spared);
    };
    let max_age = i64::try_from(LEGACY_MAX_AGE.as_secs()).map_err(io::Error::other)?;
    if age < max_age {
        Ok(Verdict::Spared)
    } else {
        Ok(Verdict::Abandoned)
    }
}

/// Adds up the regular files under `dir`.
///
/// A path which vanishes while it is being counted contributes nothing.
/// That is the normal race with another collector.
/// Every other read failure is reported rather than producing a smaller, apparently exact count.
///
/// # Errors
/// Returns an I/O error when the directory cannot be enumerated completely.
pub fn directory_size(dir: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                match entry.metadata() {
                    Ok(metadata) => {
                        total = total.checked_add(metadata.len()).ok_or_else(|| {
                            io::Error::other("directory-size accounting overflowed")
                        })?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(total)
}
