// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which target last killed each mutant, kept by the mutant's identity so it outlives an edit elsewhere, and read only to choose which target a run asks first.

use std::path::{Path, PathBuf};

use crate::outcomes::StoreError;

/// Where the hints live under the cache directory.
pub const LAYOUT: &str = "rust-mutants/killers-v1";

/// The hints under one cache directory.
#[derive(Debug, Clone)]
pub struct Killers {
    root: PathBuf,
}

impl Killers {
    /// The hints under `cache_directory`.
    #[must_use]
    pub fn new(cache_directory: &Path) -> Self {
        Self {
            root: cache_directory.join(LAYOUT),
        }
    }

    /// The target that last killed `mutant`, or `None` where nothing was recorded.
    ///
    /// # Errors
    /// [`StoreError::Io`] where a hint exists and cannot be read.
    pub fn of(&self, mutant: &str) -> Result<Option<String>, StoreError> {
        let path = self.root.join(mutant);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let target = text.trim();
                Ok((!target.is_empty()).then(|| target.to_owned()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(StoreError::Io { path, source }),
        }
    }

    /// Where the hints live.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Removes every hint, returning how many there were.
    ///
    /// # Errors
    /// Whatever the filesystem refused.
    pub fn clear(&self) -> std::io::Result<u32> {
        let held = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries.count(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        crate::tempowner::remove_tree(&self.root)?;
        u32::try_from(held).map_err(|_overflow| {
            std::io::Error::other("more killer hints than a count of them can hold")
        })
    }

    /// Records that `target` killed `mutant`.
    ///
    /// # Errors
    /// [`StoreError::Io`] where the hint could not be written.
    pub fn remember(&self, mutant: &str, target: &str) -> Result<(), StoreError> {
        let path = self.root.join(mutant);
        crate::replace::file(&path, target.as_bytes()).map_err(|failure| StoreError::Io {
            path: failure.path,
            source: failure.source,
        })
    }
}
