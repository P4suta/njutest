// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the command line itself can fail at, each with a stable code documented in `docs/errors.md`.

use std::path::{Path, PathBuf};

use rust_mutants::EngineError;
use rust_mutants::error::{self, ErrorCode};

use crate::config::ConfigError;

/// Why a command could not do what it was asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CliError {
    /// The engine could not do it.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The configuration could not be used.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The process environment already selects a mutant.
    #[error(
        "{}: {name} is already set; a run composes the activation itself, and nothing a test \
         process said under an inherited one would be about this run",
        error::ENVIRONMENT_RESERVED.code
    )]
    EnvironmentReserved {
        /// The variable that was already set.
        name: String,
    },
    /// No stored run report answers to what was asked for.
    #[error("{}: {message}", error::REPORT_MISSING.code)]
    ReportMissing {
        /// What was looked for, and where.
        message: String,
    },
    /// A file the command would write is already there.
    #[error(
        "{}: {} is already there; pass --force to overwrite it",
        error::FILE_EXISTS.code,
        path.display()
    )]
    FileExists {
        /// The file that is in the way.
        path: PathBuf,
    },
    /// A file the command had to write could not be written.
    #[error("{}: writing {}: {source}", error::WRITE_FAILED.code, path.display())]
    WriteFailed {
        /// The file that could not be written.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
}

impl CliError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Engine(inner) => inner.code(),
            Self::Config(inner) => inner.code(),
            Self::EnvironmentReserved { .. } => error::ENVIRONMENT_RESERVED,
            Self::ReportMissing { .. } => error::REPORT_MISSING,
            Self::FileExists { .. } => error::FILE_EXISTS,
            Self::WriteFailed { .. } => error::WRITE_FAILED,
        }
    }

    /// The failure of writing `path`.
    #[must_use]
    pub fn writing(path: &Path, source: std::io::Error) -> Self {
        Self::WriteFailed {
            path: path.to_path_buf(),
            source,
        }
    }
}
