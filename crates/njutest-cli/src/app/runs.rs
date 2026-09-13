// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Finding the run a command was asked about.

use std::path::{Path, PathBuf};

use crate::app::reports;
use crate::error::{self, ErrorCode};
use crate::report::{Report, json};

/// Why a run could not be answered about.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunError {
    /// No run of that name, or no run at all.
    #[error("{}: {message}", error::RUN_NOT_FOUND.code)]
    NotFound {
        /// What was looked for and where.
        message: String,
    },
    /// The run is there and its report cannot be read.
    #[error("{}: the report of {run} could not be read: {source}", error::RUN_NOT_FOUND.code)]
    Unreadable {
        /// The run.
        run: String,
        /// The failure.
        #[source]
        source: json::ReportError,
    },
}

impl RunError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound { .. } | Self::Unreadable { .. } => error::RUN_NOT_FOUND,
        }
    }
}

/// The run `named`, or the latest one when nothing was named.
///
/// # Errors
/// [`RunError::NotFound`] when there is no such run, or none at all.
pub fn resolve(root: &Path, named: Option<&str>) -> Result<String, RunError> {
    let Some(run) = named else {
        return reports::pointed_at(root, reports::LATEST_ANY).ok_or_else(|| RunError::NotFound {
            message: format!(
                "no run has completed here yet: {} names none",
                root.join(reports::LATEST_ANY).display()
            ),
        });
    };
    if directory(root, run).is_dir() {
        return Ok(run.to_owned());
    }
    Err(RunError::NotFound {
        message: format!(
            "there is no run {run} under {}",
            root.join(reports::RUNS_DIR).display()
        ),
    })
}

/// Where one run's report directory is.
#[must_use]
pub fn directory(root: &Path, run: &str) -> PathBuf {
    root.join(reports::RUNS_DIR).join(run)
}

/// Where one run's recording is, whether or not it was asked for.
#[must_use]
pub fn recording(root: &Path, run: &str) -> PathBuf {
    root.join(".njutest/trace").join(run)
}

/// The document one run wrote, as text.
///
/// # Errors
/// [`RunError::NotFound`] when the document is not there.
pub fn document(root: &Path, run: &str) -> Result<String, RunError> {
    let path = directory(root, run).join(reports::DOCUMENT_NAME);
    std::fs::read_to_string(&path).map_err(|source| RunError::NotFound {
        message: format!("reading {}: {source}", path.display()),
    })
}

/// The report one run wrote.
///
/// # Errors
/// [`RunError::NotFound`] when the document is not there and
/// [`RunError::Unreadable`] when it is not one this version understands.
pub fn report(root: &Path, run: &str) -> Result<Report, RunError> {
    let text = document(root, run)?;
    json::parse(&text).map_err(|source| RunError::Unreadable {
        run: run.to_owned(),
        source,
    })
}
