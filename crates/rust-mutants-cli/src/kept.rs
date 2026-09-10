// SPDX-FileCopyrightText: 2026 mjutest contributors
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

    /// Removes every directory the ledger names, and writes an empty ledger back.
    ///
    /// # Errors
    /// The ledger that could not be written.
    pub fn clear(directory: &Path) -> Result<(usize, Self), std::io::Error> {
        let ledger = Self::read(directory);
        let mut removed = 0usize;
        for entry in &ledger.kept {
            if std::fs::remove_dir_all(&entry.path).is_ok() {
                removed = removed.saturating_add(1);
            }
        }
        let empty = Self::default();
        let text = serde_json::to_string_pretty(&empty)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        rust_mutants::replace::file(&directory.join(FILE_NAME), format!("{text}\n").as_bytes())
            .map_err(|failure| failure.source)?;
        Ok((removed, empty))
    }
}
