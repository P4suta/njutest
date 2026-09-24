// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledger of directories a run was asked to keep, so a later command can find them and a later sweep can leave them alone.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::filesystem::{EntryKind, entry_kind};

/// The file the ledger is kept in, under the report directory.
pub const FILE_NAME: &str = "kept-v1.json";

/// The document type the ledger carries.
pub const DOCUMENT_TYPE: &str = "rust-mutants-kept-v1";

/// One directory a run kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The directory, as an absolute path.
    pub path: PathBuf,
    /// The run that kept it, which is what a reader looks it up by.
    pub run_id: String,
}

/// Every directory the runs of one report directory kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The ledger under `directory`, or an empty one when no ledger exists yet.
    ///
    /// # Errors
    /// A present ledger is unreadable, malformed, or belongs to another schema.
    pub fn read(directory: &Path) -> Result<Self, std::io::Error> {
        let path = directory.join(FILE_NAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error),
        };
        let ledger: Self = crate::strictjson::decode_str(&text)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        if ledger.document_type != DOCUMENT_TYPE || ledger.schema_version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{} is not {DOCUMENT_TYPE} schema 1", path.display()),
            ));
        }
        Ok(ledger)
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
        let mut ledger = Self::read(directory)?;
        let mut present = Vec::with_capacity(ledger.kept.len());
        for entry in &ledger.kept {
            if entry_kind(&entry.path)? != EntryKind::Missing {
                present.push(entry.clone());
            }
        }
        ledger.kept = present;
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
    /// # Errors
    /// The ledger that could not be written.
    pub fn clear(directory: &Path) -> Result<(usize, Self), std::io::Error> {
        Self::clear_with(directory, &|path: &Path| std::fs::remove_dir_all(path))
    }

    /// [`Ledger::clear`] with its removal as an argument, so the directory that refuses to go can be tested without a filesystem persuaded into refusing.
    ///
    /// # Errors
    /// The ledger that could not be written.
    pub fn clear_with<F>(directory: &Path, remove: &F) -> Result<(usize, Self), std::io::Error>
    where
        F: Fn(&Path) -> std::io::Result<()>,
    {
        let ledger = Self::read(directory)?;
        let mut removed = 0usize;
        let mut left = Self::default();
        for entry in ledger.kept {
            if entry_kind(&entry.path)? == EntryKind::Missing {
                continue;
            }
            if !matches!(
                rust_mutants::tempowner::release_kept_with(&entry.path, remove),
                Ok(rust_mutants::tempowner::Released::Removed)
            ) {
                left.kept.push(entry);
                continue;
            }
            removed = removed.checked_add(1).ok_or_else(|| {
                std::io::Error::other("the kept-directory removal count does not fit usize")
            })?;
        }
        let text = serde_json::to_string_pretty(&left)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        rust_mutants::replace::file(&directory.join(FILE_NAME), format!("{text}\n").as_bytes())
            .map_err(|failure| failure.source)?;
        Ok((removed, left))
    }
}
