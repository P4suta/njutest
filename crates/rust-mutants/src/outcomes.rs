// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What earlier runs established about individual mutants of this exact tree.

use std::io;
use std::path::{Path, PathBuf};

use crate::id::HexDigest;
use crate::outcome::Outcome;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// The name of the shape.
pub const SCHEMA: &str = "rust-mutants-outcome-v1";

/// The directory records live in, below the user's cache directory.
pub const LAYOUT: &str = "rust-mutants/outcomes-v1";

/// Bumped when a rule changes what it writes, so a record about the old edit stops answering.
pub const RULE_ABI: u32 = 1;

/// Bumped when a guard changes shape, so a record about the old instrumentation stops answering.
pub const INSTRUMENTATION_ABI: u32 = 2;

/// Bumped when the policy interpreting a finite step allowance changes.
/// A step limit is an execution bound in this ABI, never a detected mutant.
pub const STEP_POLICY_ABI: u32 = 1;

/// Bumped when a record changes what it holds, or when the recipe changes what a key is computed from.
pub const CACHE_ABI: u32 = 6;

/// An outcome strong enough to answer a later identical run.
///
/// Keeping this set separate from [`Outcome`] makes an inconclusive execution,
/// a finite bound, or a harness failure impossible to put in the cache through the typed API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheOutcome {
    /// A test failed with the mutant active.
    Killed,
    /// Every selected test passed with the mutant active.
    Survived,
}

impl From<CacheOutcome> for Outcome {
    fn from(outcome: CacheOutcome) -> Self {
        match outcome {
            CacheOutcome::Killed => Self::Killed,
            CacheOutcome::Survived => Self::Survived,
        }
    }
}

/// What one earlier run established about one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// [`SCHEMA`].
    pub schema: String,
    /// The mutant's content-addressed identity.
    pub mutant: HexDigest,
    /// What the run established.
    pub outcome: CacheOutcome,
    /// The target that ran.
    pub target: String,
    /// How many tests ran, when the harness said.
    pub tests_run: Option<u32>,
    /// Every test that failed with the mutant active, so a reused outcome still says who noticed it.
    #[serde(default)]
    pub failed_tests: Vec<String>,
    /// The run that established it.
    pub run_id: String,
}

/// Everything a key is computed from beyond the mutant's own identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyed {
    /// The digest of the pristine sources every unit of the build compiled.
    pub closure: String,
    /// The digest of the manifests, the lock file, and the cargo configuration the build read.
    pub manifests: String,
    /// The toolchain that compiled it, because two compilers are two programs.
    pub toolchain: String,
    /// The arguments the command line gave the test binaries.
    pub args: Vec<String>,
    /// The budget one execution may take, as the configuration spells it: `auto`, or a duration.
    pub timeout: String,
    /// How many times an active guard may be entered before the execution is stopped.
    /// Zero disables the bound.
    pub steps: u64,
    /// The cargo arguments the tree was compiled with, because the same tree compiled two ways is two programs.
    pub build: Vec<String>,
}

impl Keyed {
    /// Whether this names enough to remember anything by.
    #[must_use]
    pub const fn usable(&self) -> bool {
        !self.closure.is_empty()
    }

    /// The key one mutant's record is filed under.
    #[must_use]
    pub fn key(&self, mutant: &HexDigest) -> HexDigest {
        let mut hasher = Sha256::new();
        for field in [
            SCHEMA,
            &RULE_ABI.to_string(),
            &INSTRUMENTATION_ABI.to_string(),
            &STEP_POLICY_ABI.to_string(),
            &CACHE_ABI.to_string(),
            &self.closure,
            &self.manifests,
            &self.toolchain,
            mutant.as_str(),
            &self.timeout,
            &self.steps.to_string(),
        ] {
            hash_length(&mut hasher, field.len());
            hasher.update(field.as_bytes());
        }
        for list in [&self.args, &self.build] {
            hash_length(&mut hasher, list.len());
            for argument in list {
                hash_length(&mut hasher, argument.len());
                hasher.update(argument.as_bytes());
            }
        }
        HexDigest::finish(hasher)
    }
}

/// Why a durable outcome could not be read or written exactly.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The filesystem refused the operation.
    #[error("{}: {source}", path.display())]
    Io {
        /// The entry being used.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: io::Error,
    },
    /// The entry exists but is not the typed record its name promises.
    #[error("{}: {message}", path.display())]
    Corrupt {
        /// The entry being used.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
}

/// The records earlier runs left.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The store under `cache_directory`.
    #[must_use]
    pub fn new(cache_directory: &Path) -> Self {
        Self {
            root: cache_directory.join(LAYOUT),
        }
    }

    /// Where the records live.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What an earlier run established for `key`, when a record is there and is the record it claims to be.
    ///
    /// # Errors
    /// Returns every I/O or decoding failure; a corrupt cache entry is not an absent one.
    pub fn get(
        &self,
        key: &HexDigest,
        mutant: &HexDigest,
    ) -> Result<Option<(Outcome, Record)>, StoreError> {
        let path = self.entry(key);
        let text = match read_through_a_replacement(&path) {
            Ok(Some(text)) => text,
            Ok(None) => return Ok(None),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let record: Record =
            crate::strictjson::decode_str(&text).map_err(|error| StoreError::Corrupt {
                path: path.clone(),
                message: error.to_string(),
            })?;
        if record.schema != SCHEMA || &record.mutant != mutant {
            return Err(StoreError::Corrupt {
                path,
                message: format!(
                    "record schema {:?} and mutant {} do not match {SCHEMA:?} and {mutant}",
                    record.schema, record.mutant
                ),
            });
        }
        Ok(Some((record.outcome.into(), record)))
    }

    /// Records what this run established.
    ///
    /// # Errors
    /// Returns serialization and filesystem failures rather than silently turning a durable result into a cache miss on the next run.
    pub fn put(&self, key: &HexDigest, record: &Record) -> Result<PathBuf, StoreError> {
        let path = self.entry(key);
        let text = serde_json::to_string(record).map_err(|error| StoreError::Corrupt {
            path: path.clone(),
            message: error.to_string(),
        })?;
        crate::replace::file(&path, text.as_bytes()).map_err(|failure| StoreError::Io {
            path: failure.path,
            source: failure.source,
        })?;
        Ok(path)
    }

    /// How many records the store holds, and how many bytes they take.
    ///
    /// # Errors
    /// Returns the operating-system error when the store cannot be enumerated completely.
    pub fn size(&self) -> io::Result<(u32, u64)> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
            Err(error) => return Err(error),
        };
        let mut count = 0u32;
        let mut bytes = 0u64;
        for entry in entries {
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if metadata.is_file() {
                count = count.checked_add(1).ok_or_else(|| {
                    io::Error::other("outcome entry count does not fit its u32 ledger")
                })?;
                bytes = bytes.checked_add(metadata.len()).ok_or_else(|| {
                    io::Error::other("outcome byte count does not fit its u64 ledger")
                })?;
            }
        }
        Ok((count, bytes))
    }

    /// Removes every record, and says what is still there afterwards.
    ///
    /// # Errors
    /// Returns the operating-system error when the store cannot be enumerated or removed wholly.
    pub fn clear(&self) -> io::Result<(u32, u64)> {
        let (held, bytes) = self.size()?;
        match crate::tempowner::remove_tree(&self.root) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let (left, left_bytes) = self.size()?;
        let removed = held.checked_sub(left).ok_or_else(|| {
            io::Error::other("outcome store grew while its removal was being measured")
        })?;
        let removed_bytes = bytes.checked_sub(left_bytes).ok_or_else(|| {
            io::Error::other("outcome store gained bytes while its removal was being measured")
        })?;
        Ok((removed, removed_bytes))
    }

    fn entry(&self, key: &HexDigest) -> PathBuf {
        self.root.join(format!("{key}.json"))
    }
}

/// Hashes an address-space length as one platform-independent 128-bit frame.
///
/// `usize` cannot hold a value wider than the language's widest integer.
/// The zero-extension is expressed bytewise so neither a cast nor a truncating fallback can weaken that invariant on a different target width.
fn hash_length(hasher: &mut Sha256, length: usize) {
    let native = length.to_be_bytes();
    let mut canonical = [0u8; size_of::<u128>()];
    for (destination, source) in canonical.iter_mut().rev().zip(native.iter().rev()) {
        *destination = *source;
    }
    hasher.update(canonical);
}

/// How long a reader waits out a replacement before deciding a refusal is about permissions rather than timing.
///
/// A replacement is a rename and clears in microseconds, so this is orders of magnitude more than it needs and still nothing a person waits on.
const REPLACEMENT_CLEARS_WITHIN: std::time::Duration = std::time::Duration::from_millis(250);

/// How often the reader looks again while a replacement is in flight.
const LOOK_AGAIN_EVERY: std::time::Duration = std::time::Duration::from_millis(1);

/// Reads `path` whole, waiting out a replacement that is in flight rather than reporting it as a failure.
///
/// POSIX `rename` is atomic for a reader holding the old inode, so this returns on the first attempt there.
/// Windows gives an *opener* no such guarantee: while a replacement is in flight the name is briefly delete-pending and opening it answers `ERROR_ACCESS_DENIED`, so a reader racing a writer sees a refusal where the store's contract promises a whole record or nothing.
/// Answering `None` to any refusal would read an unreadable directory as a permanent cache miss, which is the same fault pointing the other way, so a refusal that outlasts a replacement is returned as itself.
fn read_through_a_replacement(path: &Path) -> io::Result<Option<String>> {
    let started = std::time::Instant::now();
    loop {
        match std::fs::read_to_string(path) {
            Ok(text) => return Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) if in_flight(&error) && started.elapsed() < REPLACEMENT_CLEARS_WITHIN => {
                std::thread::sleep(LOOK_AGAIN_EVERY);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Whether a refusal to open is the one a replacement in flight produces.
///
/// `ERROR_SHARING_VIOLATION` is 32 and has no `ErrorKind` of its own on every supported compiler, so it is read by number.
fn in_flight(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(32)
}
