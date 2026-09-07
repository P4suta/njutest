// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What earlier runs established about individual mutants of this exact tree.
//!
//! A mutant's identity already covers the file it edits and the edit itself. A
//! record is keyed on more than that: on the tree the run was about, on the
//! catalog that tree produced, and on everything the command line told the
//! harness. Two runs share a key exactly when nothing that could change what
//! the tests say about that mutant has changed, so a warm cache executes
//! nothing rather than executing everything again to reach the same answer.
//!
//! The recipe carries three version numbers of its own. A release that changes
//! how a rule writes its replacement, how a guard is composed, or what a record
//! holds bumps the one it changed, and every record written before it stops
//! answering — silently, because a stale record is one no key names.

use std::path::{Path, PathBuf};

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
pub const INSTRUMENTATION_ABI: u32 = 1;

/// Bumped when a record changes what it holds, or when the recipe changes what a key is computed from.
pub const CACHE_ABI: u32 = 3;

/// What one earlier run established about one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// [`SCHEMA`].
    pub schema: String,
    /// The mutant's content-addressed identity.
    pub mutant: String,
    /// What the run established.
    pub outcome: String,
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
///
/// The set is the smallest one that decides the answer, not the largest one
/// that is easy to name. A tree's digest is easy and wrong: a note beside the
/// code, a workflow file, a crate this run never compiled all change it, and
/// every remembered answer stops answering for a reason that could not have
/// changed one of them. What decides is the sources the compilation actually
/// read, the manifests that chose its dependencies and flags, the toolchain
/// that compiled it, and what the command line told the harness.
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
    /// The cargo arguments the tree was compiled with, because the same tree compiled two ways is two programs.
    pub build: Vec<String>,
}

impl Keyed {
    /// Whether this names enough to remember anything by.
    ///
    /// A build whose dep-info could not be read leaves no closure, and a key
    /// over nothing would file every mutant of every tree under one name. A
    /// caller with nothing to key on remembers nothing, which is the honest
    /// answer and costs only the executions it would have saved.
    #[must_use]
    pub const fn usable(&self) -> bool {
        !self.closure.is_empty()
    }

    /// The key one mutant's record is filed under.
    #[must_use]
    pub fn key(&self, mutant: &str) -> String {
        let mut hasher = Sha256::new();
        for field in [
            SCHEMA,
            &RULE_ABI.to_string(),
            &INSTRUMENTATION_ABI.to_string(),
            &CACHE_ABI.to_string(),
            &self.closure,
            &self.manifests,
            &self.toolchain,
            mutant,
            &self.timeout,
        ] {
            hasher.update(u32::try_from(field.len()).unwrap_or(u32::MAX).to_be_bytes());
            hasher.update(field.as_bytes());
        }
        for list in [&self.args, &self.build] {
            hasher.update(u32::try_from(list.len()).unwrap_or(u32::MAX).to_be_bytes());
            for argument in list {
                hasher.update(
                    u32::try_from(argument.len())
                        .unwrap_or(u32::MAX)
                        .to_be_bytes(),
                );
                hasher.update(argument.as_bytes());
            }
        }
        hex::encode(hasher.finalize())
    }
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
    /// A record that does not parse, is about another mutant, or names an
    /// outcome this release does not know answers nothing. Nothing is not the
    /// same as a wrong answer, and a cache is the one place where it must not
    /// become one.
    #[must_use]
    pub fn get(&self, key: &str, mutant: &str) -> Option<(Outcome, Record)> {
        let text = std::fs::read_to_string(self.entry(key)).ok()?;
        let record: Record = serde_json::from_str(&text).ok()?;
        if record.schema != SCHEMA || record.mutant != mutant {
            return None;
        }
        Outcome::parse(&record.outcome).map(|outcome| (outcome, record))
    }

    /// Records what this run established. A failure to write is a run that will do the work again, which is not a reason to stop.
    pub fn put(&self, key: &str, record: &Record) {
        let Ok(text) = serde_json::to_string(record) else {
            return;
        };
        if std::fs::create_dir_all(&self.root).is_err() {
            return;
        }
        drop(std::fs::write(self.entry(key), text));
    }

    /// How many records the store holds, and how many bytes they take.
    #[must_use]
    pub fn size(&self) -> (u32, u64) {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return (0, 0);
        };
        let mut count = 0u32;
        let mut bytes = 0u64;
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata()
                && metadata.is_file()
            {
                count = count.saturating_add(1);
                bytes = bytes.saturating_add(metadata.len());
            }
        }
        (count, bytes)
    }

    /// Removes every record. What a cache holds is always re-derivable, so emptying one costs time and never correctness.
    #[must_use]
    pub fn clear(&self) -> (u32, u64) {
        let held = self.size();
        drop(std::fs::remove_dir_all(&self.root));
        held
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.json"))
    }
}
