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
pub const SCHEMA: &str = "rust-mutants-outcome-v2";

/// The directory records live in, below the user's cache directory.
pub const LAYOUT: &str = "rust-mutants/outcomes-v2";

/// Bumped when a rule changes what it writes, so a record about the old edit stops answering.
pub const RULE_ABI: u32 = 1;

/// Bumped when a guard changes shape, so a record about the old instrumentation stops answering.
pub const INSTRUMENTATION_ABI: u32 = 2;

/// Bumped when the policy interpreting a finite step allowance changes.
/// A step limit is an execution bound in this ABI, never a detected mutant.
pub const STEP_POLICY_ABI: u32 = 1;

/// Bumped when a record changes what it holds, or when the recipe changes what a key is computed from.
pub const CACHE_ABI: u32 = 7;

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
    /// Everything beyond the mutant the record is keyed on, so the name it is filed under can be recomputed wherever it is read.
    pub keyed: Keyed,
}

impl Record {
    /// The key this record is filed under, recomputed from what it says it was keyed on.
    #[must_use]
    pub fn key(&self) -> HexDigest {
        self.keyed.key(&self.mutant)
    }
}

/// Everything a key is computed from beyond the mutant's own identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The digest of the engine that decided, because two builds of it may mean two different things by the same verdict.
    /// Empty where the engine could not be read, which remembers nothing.
    pub engine: String,
}

impl Keyed {
    /// Whether this names enough to remember anything by.
    #[must_use]
    pub const fn usable(&self) -> bool {
        !self.closure.is_empty() && !self.engine.is_empty()
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
            &self.engine,
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

/// The document a store travels in between checkouts, machines and CI jobs.
pub const EXPORT_DOCUMENT: &str = "rust-mutants/outcomes-export";

/// The version of [`EXPORT_DOCUMENT`] this release writes and reads.
pub const EXPORT_VERSION: u32 = 1;

/// The versions of everything a key is recomputed under, so records from a release that computed keys another way are refused rather than filed under names this one would compute differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Abi {
    /// [`RULE_ABI`].
    pub rule: u32,
    /// [`INSTRUMENTATION_ABI`].
    pub instrumentation: u32,
    /// [`STEP_POLICY_ABI`].
    pub step_policy: u32,
    /// [`CACHE_ABI`].
    pub cache: u32,
}

impl Abi {
    /// The versions this release keys under.
    pub const CURRENT: Self = Self {
        rule: RULE_ABI,
        instrumentation: INSTRUMENTATION_ABI,
        step_policy: STEP_POLICY_ABI,
        cache: CACHE_ABI,
    };
}

/// A whole store, as it travels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exported {
    /// [`EXPORT_DOCUMENT`].
    pub document_type: String,
    /// [`EXPORT_VERSION`].
    pub schema_version: u32,
    /// The versions the records were keyed under.
    pub abi: Abi,
    /// Every record, each carrying what it was keyed on, in key order.
    pub records: Vec<Record>,
}

/// The digest of the engine executable at `program`, which is what a remembered verdict was decided by.
///
/// # Errors
/// The file could not be read.
pub fn engine_of(program: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(program)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0_u8; 1 << 16];
    loop {
        let read = io::Read::read(&mut file, &mut chunk)?;
        let Some(held) = chunk.get(..read).filter(|held| !held.is_empty()) else {
            return Ok(HexDigest::finish(hasher).to_string());
        };
        hasher.update(held);
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
    /// A travelling store is not one this release can file.
    #[error("{message}")]
    Refused {
        /// Why.
        message: String,
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
        if record.key() != *key {
            return Err(StoreError::Corrupt {
                path,
                message: format!(
                    "the record says it was keyed on inputs whose key is {}, not the {key} it is filed under",
                    record.key()
                ),
            });
        }
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

    /// Records what a run established, under the key its own inputs name, so no record can be filed under a name it does not derive.
    ///
    /// # Errors
    /// Returns serialization and filesystem failures rather than silently turning a durable result into a cache miss on the next run.
    pub fn put(&self, record: &Record) -> Result<PathBuf, StoreError> {
        let path = self.entry(&record.key());
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
        match std::fs::remove_dir_all(&self.root) {
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

    /// Every record the store holds, each checked against the name it is filed under, in key order.
    ///
    /// # Errors
    /// Returns the first entry that cannot be enumerated, read, decoded, or recomputed to its own name.
    pub fn export(&self) -> Result<Exported, StoreError> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(exported(Vec::new()));
            }
            Err(source) => {
                return Err(StoreError::Io {
                    path: self.root.clone(),
                    source,
                });
            }
        };
        let mut records = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| StoreError::Io {
                path: self.root.clone(),
                source,
            })?;
            let path = entry.path();
            let stem = path.file_stem().and_then(std::ffi::OsStr::to_str);
            let key = match stem.map(HexDigest::try_from) {
                Some(Ok(key)) => key,
                Some(Err(_)) | None => {
                    return Err(StoreError::Corrupt {
                        path,
                        message: "an entry whose name is not a key".to_owned(),
                    });
                }
            };
            let text = std::fs::read_to_string(&path).map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
            let record: Record =
                crate::strictjson::decode_str(&text).map_err(|error| StoreError::Corrupt {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
            if self.get(&key, &record.mutant)?.is_none() {
                return Err(StoreError::Corrupt {
                    path,
                    message: "the entry went away while it was being exported".to_owned(),
                });
            }
            records.push(record);
        }
        records.sort_by_key(Record::key);
        Ok(exported(records))
    }

    /// Files every record of a travelling store under the key its own inputs name, and says how many.
    ///
    /// # Errors
    /// Refuses a document of another type or version, or keyed under another release's versions, before writing anything; returns the first write that fails.
    pub fn import(&self, exported: &Exported) -> Result<usize, StoreError> {
        let refused = |message: String| Err(StoreError::Refused { message });
        if exported.document_type != EXPORT_DOCUMENT || exported.schema_version != EXPORT_VERSION {
            return refused(format!(
                "a {:?} version {} document is not a {EXPORT_DOCUMENT:?} version {EXPORT_VERSION} store",
                exported.document_type, exported.schema_version
            ));
        }
        if exported.abi != Abi::CURRENT {
            return refused(format!(
                "these records were keyed under {:?}, and this release keys under {:?}, so a name recomputed here would not be the name they were answered under",
                exported.abi,
                Abi::CURRENT
            ));
        }
        if let Some(record) = exported.records.iter().find(|one| one.schema != SCHEMA) {
            return refused(format!(
                "a record of schema {:?} is not a {SCHEMA:?} record",
                record.schema
            ));
        }
        for record in &exported.records {
            self.put(record)?;
        }
        Ok(exported.records.len())
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

/// A travelling store holding `records`, as this release writes one.
fn exported(records: Vec<Record>) -> Exported {
    Exported {
        document_type: EXPORT_DOCUMENT.to_owned(),
        schema_version: EXPORT_VERSION,
        abi: Abi::CURRENT,
        records,
    }
}
