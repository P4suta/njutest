// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledger of directories a run was asked to keep, so a later command can find them and a later sweep can leave them alone.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file the ledger is kept in, under the report directory.
pub const FILE_NAME: &str = "kept-v1.json";

/// The document type the ledger carries.
pub const DOCUMENT_TYPE: &str = "rust-mutants-kept-v1";

/// One directory a run kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The directory, as an absolute path.
    pub path: PathBuf,
    /// The run that kept it, which is what a reader looks it up by.
    pub run_id: String,
}

/// Every directory the runs of one report directory kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    /// Names the shape, so a reader can tell versions apart.
    pub document_type: String,
    /// The version of that shape.
    pub schema_version: u32,
    /// The directories, oldest first.
    pub kept: Vec<Entry>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            document_type: DOCUMENT_TYPE.to_owned(),
            schema_version: 1,
            kept: Vec::new(),
        }
    }
}

impl Ledger {
    /// The ledger under `directory`, or an empty one when there is none this release reads.
    ///
    /// A ledger this release cannot read authorises nothing: it is read as
    /// empty rather than guessed at, so a sweep never removes a directory on
    /// the strength of a document it did not understand, and never keeps one
    /// on the strength of one either.
    #[must_use]
    pub fn read(directory: &Path) -> Self {
        std::fs::read_to_string(directory.join(FILE_NAME))
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .filter(|ledger| ledger.document_type == DOCUMENT_TYPE && ledger.schema_version == 1)
            .unwrap_or_default()
    }

    /// Adds what one run kept and writes the ledger back, dropping what is no longer there.
    ///
    /// # Errors
    /// The ledger that could not be written.
    pub fn record(
        directory: &Path,
        run_id: &str,
        kept: &[PathBuf],
    ) -> Result<Self, std::io::Error> {
        let mut ledger = Self::read(directory);
        ledger.kept.retain(|entry| entry.path.exists());
        for path in kept {
            if ledger.kept.iter().any(|entry| entry.path == *path) {
                continue;
            }
            ledger.kept.push(Entry {
                path: path.clone(),
                run_id: run_id.to_owned(),
            });
        }
        let text = serde_json::to_string_pretty(&ledger)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        rust_mutants::replace::file(&directory.join(FILE_NAME), format!("{text}\n").as_bytes())
            .map_err(|failure| failure.source)?;
        Ok(ledger)
    }

    /// Removes the directories the ledger names, and writes back the ones that are still there.
    ///
    /// What it could not remove stays in the ledger. A directory something
    /// else is holding — a mount, a device, a process with it open — refuses,
    /// and a ledger that forgot it would leave a directory on the disk that
    /// nothing now names: not the ledger, which just dropped it, and not the
    /// person, who was told the clearing was done. Keeping it is what lets the
    /// next clearing try again, and what lets `cache` still say it is there.
    ///
    /// The removals are bounded for the same reason the sweep is. A refusal
    /// can take minutes, and a few hundred of those would run for a day; what
    /// is not reached this time is still named, which is the whole point of
    /// writing the survivors back.
    ///
    /// # Errors
    /// The ledger that could not be written.
    pub fn clear(directory: &Path) -> Result<(usize, Self), std::io::Error> {
        Self::clear_with(directory, &|path: &Path| std::fs::remove_dir_all(path))
    }

    /// [`Ledger::clear`] with its removal as an argument, so the directory that refuses to go can be tested without a filesystem persuaded into refusing.
    ///
    /// # Errors
    /// The ledger that could not be written.
    pub fn clear_with(
        directory: &Path,
        remove: &dyn Fn(&Path) -> std::io::Result<()>,
    ) -> Result<(usize, Self), std::io::Error> {
        let ledger = Self::read(directory);
        let started = std::time::Instant::now();
        let mut removed = 0usize;
        let mut left = Self::default();
        for entry in ledger.kept {
            if !entry.path.exists() {
                continue;
            }
            if started.elapsed() >= rust_mutants::tempowner::SWEEP_BUDGET
                || remove(&entry.path).is_err()
            {
                left.kept.push(entry);
                continue;
            }
            removed = removed.saturating_add(1);
        }
        let text = serde_json::to_string_pretty(&left)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        rust_mutants::replace::file(&directory.join(FILE_NAME), format!("{text}\n").as_bytes())
            .map_err(|failure| failure.source)?;
        Ok((removed, left))
    }
}
