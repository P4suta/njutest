// SPDX-FileCopyrightText: 2026 mjutest contributors
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

/// The marker's schema field. It carries the version, so a later document shape can never be read as this one.
pub const SCHEMA: &str = "rust-mutants-temp-owner-v1";
/// The advisory lock file inside a claimed directory.
pub const LOCK_NAME: &str = "owner.lock";
/// The JSON marker file inside a claimed directory.
pub const MARKER_NAME: &str = "owner.json";
/// How long an unowned directory must have been untouched before [`sweep`] treats it as a leftover.
pub const LEGACY_MAX_AGE: Duration = Duration::from_hours(24);

/// What a claimed directory is for. A scratch belongs to one run and goes away with it; a cache is meant to outlive the run that filled it, which is what makes a second run fast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    /// One run's working tree. A sweep reclaims it as soon as nobody holds its lock.
    #[default]
    Scratch,
    /// A build cache. A sweep spares it however old it is; only a caller that asks for it by name reclaims it.
    Cache,
}

/// The JSON document in a claimed directory. Written once at creation and rewritten only to record a deliberate keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// [`SCHEMA`].
    pub schema: String,
    /// The process that claimed the directory. Diagnostic only: liveness is the lock's job.
    pub pid: u32,
    /// When the directory was claimed, in UTC.
    pub started: Timestamp,
    /// Whether the directory was preserved on purpose and is not an orphan.
    pub kept: bool,
    /// What the directory is for. Absent in a marker written before roles existed, which means a scratch.
    #[serde(default)]
    pub role: Role,
    /// The tree a cache is keyed to, so a sweep can tell a cache a run will look up from one nothing can name again.
    ///
    /// A cache is spared however old it is, which is only safe while some
    /// later run can still hit it. The key is derived from the source tree, so
    /// a cache whose tree is gone is one no run will ever look up, and
    /// sparing it is how a temporary directory grows without bound. Absent in
    /// a marker written before caches said this, which says nothing either
    /// way and is therefore spared as it always was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyed_to: Option<String>,
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
#[non_exhaustive]
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

/// A claimed directory: the lock is held open and the marker is written. Releasing or keeping it closes the lock; neither removes anything.
#[derive(Debug)]
pub struct Owner {
    dir: PathBuf,
    lock: Option<Lock>,
    marker: Marker,
}

/// Writes the marker pair into an existing directory and takes its lock.
///
/// # Errors
/// Returns [`ClaimError::Owned`] when another process holds the lock, and
/// the I/O failure otherwise.
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

    /// Closes the lock without touching the directory. Idempotent, and it must be called before the directory is removed: on Windows an open handle inside a directory is what makes the removal fail.
    ///
    /// # Errors
    /// Returns the unlock or close failure.
    pub fn release(&mut self) -> io::Result<()> {
        self.lock.take().map_or(Ok(()), |mut lock| lock.release())
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
    serde_json::from_slice(&raw).map_err(|source| MarkerError::Malformed {
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

/// What one [`sweep`] did. Diagnostic: no report, no schema, and no exit code depends on it, because collecting somebody else's leftovers is housekeeping a run does on the way.
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
    /// The directories that could not be judged or removed. A failure does not stop the sweep of the others.
    pub failures: Vec<SweepFailure>,
}

/// Removes every abandoned directory directly under `parent` whose name begins with one of `prefixes`.
///
/// # Errors
/// Returns the failure to read `parent` itself.
pub fn sweep(parent: &Path, prefixes: &[&str], now: Timestamp) -> io::Result<SweepResult> {
    sweep_with(parent, prefixes, now, &|dir: &Path| fs::remove_dir_all(dir))
}

/// Removes every unlocked directory under `parent` whose name is prefixed, caches included.
///
/// This is what a person means by collecting the caches: [`sweep`] spares
/// them so that the next run is fast, and this does not.
///
/// # Errors
/// Returns the failure to read `parent` itself.
pub fn reclaim(parent: &Path, prefixes: &[&str], now: Timestamp) -> io::Result<SweepResult> {
    reclaim_with(parent, prefixes, now, &|dir: &Path| fs::remove_dir_all(dir))
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
struct Pass<'a> {
    prefixes: &'a [&'a str],
    now: Timestamp,
    remove: &'a dyn Fn(&Path) -> io::Result<()>,
    /// Whether a build cache nobody holds counts as reclaimable.
    caches_too: bool,
}

fn collect(parent: &Path, pass: &Pass<'_>) -> io::Result<SweepResult> {
    let Pass {
        prefixes,
        now,
        remove,
        caches_too,
    } = *pass;
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(SweepResult::default()),
        Err(error) => return Err(error),
    };
    let mut result = SweepResult::default();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(source) => {
                result.failures.push(SweepFailure {
                    dir: parent.to_path_buf(),
                    source,
                });
                continue;
            }
        };
        let name = entry.file_name();
        let is_prefixed = name.to_str().is_some_and(|name| {
            prefixes
                .iter()
                .any(|prefix| !prefix.is_empty() && name.starts_with(prefix))
        });
        if !is_prefixed || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let dir = parent.join(&name);
        let verdict = match judge(&dir, &entry, now) {
            Ok(Verdict::Cache) if caches_too => match acquire(&lock_path(&dir))? {
                None => Ok(Verdict::Live),
                Some(mut lock) => {
                    lock.release()?;
                    Ok(Verdict::Abandoned)
                }
            },
            other => other,
        };
        match verdict {
            Ok(Verdict::Live) => result.live = result.live.saturating_add(1),
            Ok(Verdict::Kept) => result.kept = result.kept.saturating_add(1),
            Ok(Verdict::Cache) => result.cached = result.cached.saturating_add(1),
            Ok(Verdict::Spared) => {}
            Ok(Verdict::Abandoned) => {
                let size = directory_size(&dir);
                match remove(&dir) {
                    Ok(()) => {
                        result.removed.push(dir);
                        result.removed_bytes = result.removed_bytes.saturating_add(size);
                    }
                    Err(source) => result.failures.push(SweepFailure { dir, source }),
                }
            }
            Err(source) => result.failures.push(SweepFailure { dir, source }),
        }
    }
    Ok(result)
}

/// What the sweep decided about one directory. Only `Abandoned` removes.
enum Verdict {
    Abandoned,
    /// Somebody holds the lock.
    Live,
    /// The marker says it was preserved.
    Kept,
    /// The marker says it is a build cache, which outlives the run that filled it.
    Cache,
    /// Left alone without being counted: an unowned directory too young to judge. Not a fact about a live owner, so not a number in the result.
    Spared,
}

/// A marker that cannot be read at all is treated as a marker that does not say kept, deliberately: the lock has already answered the only question that matters, and a half-written marker must not make a dead directory immortal.
fn judge(dir: &Path, entry: &fs::DirEntry, now: Timestamp) -> io::Result<Verdict> {
    match read_marker(dir) {
        Ok(marker) if marker.kept => return Ok(Verdict::Kept),
        Ok(marker) if marker.role == Role::Cache => {
            if !orphaned(marker.keyed_to.as_deref()) {
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
///
/// A cache with no key says nothing either way: it was written before caches
/// said what they are keyed to, and a sweep that guessed would remove one a
/// run is about to use.
fn orphaned(keyed_to: Option<&str>) -> bool {
    keyed_to.is_some_and(|tree| !Path::new(tree).exists())
}

/// A directory with no marker at all: one created before this convention, or one whose marker was lost. Age is the only evidence there is, and a young one is left alone because it may be a run in progress.
fn legacy(entry: &fs::DirEntry, now: Timestamp) -> io::Result<Verdict> {
    let modified = match entry.metadata() {
        Ok(metadata) => metadata.modified()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Verdict::Spared),
        Err(error) => return Err(error),
    };
    let modified = Timestamp::try_from(modified).map_err(io::Error::other)?;
    let age = now.as_second().saturating_sub(modified.as_second());
    let max_age = i64::try_from(LEGACY_MAX_AGE.as_secs()).unwrap_or(i64::MAX);
    if age < max_age {
        Ok(Verdict::Spared)
    } else {
        Ok(Verdict::Abandoned)
    }
}

/// Adds up the regular files under `dir`, best effort: the number is for a person reading a log line, and a sweep must not fail to reclaim a directory because it could not measure one file inside it.
fn directory_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file()
                && let Ok(metadata) = entry.metadata()
            {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}
