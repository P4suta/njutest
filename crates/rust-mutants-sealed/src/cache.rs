// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A compilation cache retained until its producer process has ended.

use std::path::{Path, PathBuf};

use crate::SealedError;

/// A durable compilation-cache directory, disposed only after its producer process has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationCache {
    directory: PathBuf,
}

impl CompilationCache {
    /// Retains a cache independently of temporary-directory cleanup inside its producer process.
    ///
    /// # Errors
    /// The directory could not be created or its canonical identity could not be read.
    pub fn retained(directory: PathBuf) -> Result<Self, SealedError> {
        std::fs::create_dir_all(&directory).map_err(|source| SealedError::Preparation {
            path: directory.clone(),
            source,
        })?;
        let canonical =
            std::fs::canonicalize(&directory).map_err(|source| SealedError::Preparation {
                path: directory,
                source,
            })?;
        Ok(Self {
            directory: canonical,
        })
    }

    /// The durable directory whose producer process retains its lifetime.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}
