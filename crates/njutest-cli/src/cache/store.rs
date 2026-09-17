// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The answers earlier runs reached, keyed by the identity of what they were about.

use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;

use crate::error::{self, ErrorCode};
use crate::report::{Report, audit};

/// The directory a store lives in, below the user's cache directory. The version is in the name: a future layout becomes `outcomes-v2` and the two never read each other's entries.
pub const LAYOUT: &str = "njutest/outcomes-v1";

/// The extension of a stored answer.
pub const ENTRY_EXTENSION: &str = "json";

/// The extension of the claim a run holds while it establishes one.
pub const LEASE_EXTENSION: &str = "lock";

/// Why the store could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// An entry could not be read or written.
    #[error("{}: {}: {source}", error::CACHE_UNUSABLE.code, path.display())]
    Unusable {
        /// The entry.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// An entry is there and is not the answer it claims to be.
    #[error("{}: {}: {message}", error::CACHE_CORRUPT.code, path.display())]
    Corrupt {
        /// The entry.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
    /// A report was offered for storage that must not be stored.
    #[error("{}: {message}", error::CACHE_UNUSABLE.code)]
    Refused {
        /// Why it must not be stored.
        message: String,
    },
    /// The stream answers were being carried on or off this machine stopped.
    #[error("{}: carrying answers: {source}", error::CACHE_UNUSABLE.code)]
    Carrying {
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A line offered to this machine is not an answer at all.
    #[error("{}: line {line}: {message}", error::CACHE_CORRUPT.code)]
    Arriving {
        /// Which line, counting from one.
        line: u32,
        /// What is wrong with it.
        message: String,
    },
}

impl CacheError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unusable { .. } | Self::Refused { .. } | Self::Carrying { .. } => {
                error::CACHE_UNUSABLE
            }
            Self::Corrupt { .. } | Self::Arriving { .. } => error::CACHE_CORRUPT,
        }
    }
}

/// What a store holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Status {
    /// How many answers.
    pub entries: u32,
    /// How many bytes they take.
    pub bytes: u64,
}

/// What a collection removed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Collected {
    /// The entries removed for being older than the time to live.
    pub expired: Vec<PathBuf>,
    /// The entries removed to bring the store under its size, oldest first.
    pub evicted: Vec<PathBuf>,
    /// How many bytes went away.
    pub bytes: u64,
}

/// The answers earlier runs reached.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
    max_bytes: u64,
    ttl: Duration,
}

impl Store {
    /// A store under `cache_directory`, bounded by `max_bytes` and `ttl`.
    #[must_use]
    pub fn new(cache_directory: &Path, max_bytes: u64, ttl: Duration) -> Self {
        Self {
            root: cache_directory.join(LAYOUT),
            max_bytes,
            ttl,
        }
    }

    /// Where the answers live.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The entry one identity is stored at.
    #[must_use]
    pub fn entry(&self, identity: &str) -> PathBuf {
        self.root.join(format!("{identity}.{ENTRY_EXTENSION}"))
    }

    /// The claim a run holds while it establishes one identity.
    #[must_use]
    pub fn lease(&self, identity: &str) -> PathBuf {
        self.root.join(format!("{identity}.{LEASE_EXTENSION}"))
    }

    /// The answer an earlier run reached for `identity`, if one is stored and is the answer it claims to be.
    ///
    /// # Errors
    /// [`CacheError::Corrupt`] when an entry is there and is not that answer: a
    /// stored document that does not parse, does not carry the identity it is
    /// filed under, or does not satisfy the audit every durable report must.
    /// Nothing is silently ignored, because an entry that is quietly wrong is
    /// exactly what a wrong answer looks like.
    pub fn get(&self, identity: &str) -> Result<Option<Report>, CacheError> {
        let path = self.entry(identity);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(CacheError::Unusable { path, source }),
        };
        let report = crate::report::json::parse(&text).map_err(|error| CacheError::Corrupt {
            path: path.clone(),
            message: error.to_string(),
        })?;
        if report.provenance.identity != identity {
            return Err(CacheError::Corrupt {
                path,
                message: format!(
                    "filed under {identity} and carries {}",
                    report.provenance.identity
                ),
            });
        }
        let violations = audit::validate_for_persistence(&report);
        if !violations.is_empty() {
            return Err(CacheError::Corrupt {
                path,
                message: violations
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            });
        }
        Ok(Some(report))
    }

    /// Stores `report` under its own identity, and returns where it went.
    ///
    /// # Errors
    /// [`CacheError::Refused`] for a report that must not be stored: one with
    /// no identity, and one that was itself read back, because a chain of
    /// copies is not a chain of evidence.
    pub fn put(&self, report: &Report) -> Result<PathBuf, CacheError> {
        let identity = report.provenance.identity.as_str();
        if identity.trim().is_empty() || identity == crate::report::UNAVAILABLE {
            return Err(CacheError::Refused {
                message: "a report with no identity answers for no inputs".to_owned(),
            });
        }
        if report.provenance.cached {
            return Err(CacheError::Refused {
                message: "a report that was read back is already stored where it came from"
                    .to_owned(),
            });
        }
        let violations = audit::validate_for_persistence(report);
        if !violations.is_empty() {
            return Err(CacheError::Refused {
                message: format!(
                    "the report is not one a reader could check: {}",
                    violations
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            });
        }
        let text = crate::report::json::render(report).map_err(|error| CacheError::Refused {
            message: error.to_string(),
        })?;
        let path = self.entry(identity);
        rust_mutants::replace::file(&path, text.as_bytes()).map_err(|failure| {
            CacheError::Unusable {
                path: failure.path,
                source: failure.source,
            }
        })?;
        Ok(path)
    }

    /// What the store holds.
    ///
    /// # Errors
    /// The store's own directory could not be listed, or an entry selected for
    /// removal could not be removed.
    pub fn status(&self) -> Result<Status, CacheError> {
        let mut status = Status::default();
        for entry in self.entries()? {
            status.entries = status.entries.saturating_add(1);
            status.bytes = status.bytes.saturating_add(entry.bytes);
        }
        Ok(status)
    }

    /// Removes what has expired, then the oldest of what is left until the store is under its size.
    ///
    /// # Errors
    /// The store's own directory could not be listed.
    pub fn collect(&self, now: Timestamp) -> Result<Collected, CacheError> {
        let entries = self.entries()?;
        let mut collected = Collected::default();
        let mut remaining = Vec::new();
        for entry in entries {
            let age = now
                .as_second()
                .saturating_sub(entry.modified.as_second())
                .max(0);
            let ttl = i64::try_from(self.ttl.as_secs()).unwrap_or(i64::MAX);
            if self.ttl > Duration::ZERO && age >= ttl {
                remove(&entry)?;
                collected.bytes = collected.bytes.saturating_add(entry.bytes);
                collected.expired.push(entry.path);
            } else {
                remaining.push(entry);
            }
        }
        if self.max_bytes == 0 {
            return Ok(collected);
        }
        remaining.sort_by_key(|entry| entry.modified);
        let mut total: u64 = remaining.iter().map(|entry| entry.bytes).sum();
        for entry in remaining {
            if total <= self.max_bytes {
                break;
            }
            remove(&entry)?;
            total = total.saturating_sub(entry.bytes);
            collected.bytes = collected.bytes.saturating_add(entry.bytes);
            collected.evicted.push(entry.path);
        }
        Ok(collected)
    }

    /// Writes every answer this store holds, one to a line, and says how many.
    ///
    /// # Errors
    /// [`CacheError::Corrupt`] for an entry that is not the answer it claims
    /// to be, [`CacheError::Unusable`] for a store that cannot be listed, and
    /// [`CacheError::Carrying`] for a destination that stops taking bytes.
    pub fn export(&self, out: &mut dyn std::io::Write) -> Result<u32, CacheError> {
        let mut written: u32 = 0;
        for entry in self.entries()? {
            let report = self
                .get(&entry.identity)?
                .ok_or_else(|| CacheError::Unusable {
                    path: entry.path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "the entry disappeared while it was being exported",
                    ),
                })?;
            let text = crate::report::json::line(&report).map_err(|error| CacheError::Corrupt {
                path: entry.path.clone(),
                message: error.to_string(),
            })?;
            writeln!(out, "{text}").map_err(|source| CacheError::Carrying { source })?;
            written = written.saturating_add(1);
        }
        Ok(written)
    }

    /// Reads answers another machine wrote, and says how many this one now holds.
    ///
    /// # Errors
    /// [`CacheError::Arriving`] for a line that is not a report, naming the
    /// line, [`CacheError::Refused`] for an answer this machine may not keep,
    /// and [`CacheError::Carrying`] for a source that stops mid-stream.
    pub fn import(&self, input: &mut dyn std::io::Read) -> Result<u32, CacheError> {
        let mut read: u32 = 0;
        for (at, line) in std::io::BufRead::lines(std::io::BufReader::new(input)).enumerate() {
            let line = line.map_err(|source| CacheError::Carrying { source })?;
            if line.trim().is_empty() {
                continue;
            }
            let report =
                crate::report::json::parse(&line).map_err(|error| CacheError::Arriving {
                    line: u32::try_from(at).unwrap_or(u32::MAX).saturating_add(1),
                    message: error.to_string(),
                })?;
            let _at = self.put(&report)?;
            read = read.saturating_add(1);
        }
        Ok(read)
    }

    fn entries(&self) -> Result<Vec<Entry>, CacheError> {
        let listing = match std::fs::read_dir(&self.root) {
            Ok(listing) => listing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(CacheError::Unusable {
                    path: self.root.clone(),
                    source,
                });
            }
        };
        let mut entries = Vec::new();
        for found in listing {
            let found = found.map_err(|source| CacheError::Unusable {
                path: self.root.clone(),
                source,
            })?;
            let path = found.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some(ENTRY_EXTENSION) {
                continue;
            }
            let identity = path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or_else(|| CacheError::Corrupt {
                    path: path.clone(),
                    message: "the entry name is not text".to_owned(),
                })?
                .to_owned();
            let metadata = found.metadata().map_err(|source| CacheError::Unusable {
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file() {
                return Err(CacheError::Corrupt {
                    path,
                    message: "an outcome entry is not a regular file".to_owned(),
                });
            }
            let modified = metadata
                .modified()
                .map_err(|source| CacheError::Unusable {
                    path: path.clone(),
                    source,
                })
                .and_then(|when| {
                    Timestamp::try_from(when).map_err(|error| CacheError::Corrupt {
                        path: path.clone(),
                        message: format!("its modification time is not representable: {error}"),
                    })
                })?;
            entries.push(Entry {
                path,
                identity,
                bytes: metadata.len(),
                modified,
            });
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }
}

#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    identity: String,
    bytes: u64,
    modified: Timestamp,
}

fn remove(entry: &Entry) -> Result<(), CacheError> {
    std::fs::remove_file(&entry.path).map_err(|source| CacheError::Unusable {
        path: entry.path.clone(),
        source,
    })
}
