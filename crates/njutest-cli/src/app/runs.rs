// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Finding the run a command was asked about.

use std::path::{Path, PathBuf};

use rust_mutants::id::{StoredRunId, StoredRunIdError};

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
    /// A requested or indexed run name is not one canonical path component.
    #[error("{}: {source}", error::RUN_NOT_FOUND.code)]
    InvalidName {
        /// Why the name is unsafe or ambiguous.
        #[source]
        source: StoredRunIdError,
    },
    /// The configured report store or one of its indexes could not be read exactly.
    #[error(transparent)]
    Store(#[from] reports::StoreError),
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
            Self::NotFound { .. } | Self::InvalidName { .. } | Self::Unreadable { .. } => {
                error::RUN_NOT_FOUND
            }
            Self::Store(error) => error.code(),
        }
    }
}

/// One stored run whose report directory remains bound to the held store
/// capability that selected it.
#[derive(Debug)]
pub(crate) struct ResolvedRun {
    stored: reports::StoredRun,
    config: crate::config::Config,
}

impl ResolvedRun {
    /// The canonical run identity.
    #[must_use]
    pub(crate) const fn id(&self) -> &StoredRunId {
        self.stored.id()
    }

    /// The canonical workspace-relative document spelling for presentation.
    #[must_use]
    pub(crate) fn said_document(&self) -> &str {
        self.stored.said_document()
    }

    /// Reads one optional closed file through the held run directory.
    ///
    /// # Errors
    /// Returns the capability-backed store refusal.
    pub(crate) fn read(&self, file: reports::StoredFile) -> Result<Option<String>, RunError> {
        self.stored.read(file).map_err(RunError::from)
    }

    /// The configuration read through the same held workspace capability
    /// that selected this run.
    #[must_use]
    pub(crate) const fn config(&self) -> &crate::config::Config {
        &self.config
    }
}

/// The run `named`, or the latest one when nothing was named.
///
/// # Errors
/// [`RunError::NotFound`] when there is no such run, or none at all.
pub(crate) fn resolve(root: &Path, named: Option<&str>) -> Result<ResolvedRun, RunError> {
    let workspace = reports::WorkspaceRoot::open(root)?;
    let loaded = workspace.load_config()?;
    let store = workspace.store(&loaded.config.reports.directory)?;
    let stored = if let Some(run) = named {
        let id = StoredRunId::try_from(run).map_err(|source| RunError::InvalidName { source })?;
        store.open_run(&id)?
    } else {
        store
            .pointed_run(reports::Index::Any)?
            .ok_or_else(|| RunError::NotFound {
                message: format!(
                    "no run has completed here yet: {} names none",
                    store.index_display(reports::Index::Any)
                ),
            })?
    };
    Ok(ResolvedRun {
        stored,
        config: loaded.config,
    })
}

/// Where one run's recording is, whether or not it was asked for.
#[must_use]
pub fn recording(root: &Path, run: &StoredRunId) -> PathBuf {
    root.join(".njutest/trace").join(run.as_str())
}

/// The document one run wrote, as text.
///
/// # Errors
/// [`RunError::NotFound`] when the document is not there.
pub(crate) fn document(run: &ResolvedRun) -> Result<String, RunError> {
    run.stored.document().map_err(RunError::from)
}

/// The report one run wrote.
///
/// # Errors
/// [`RunError::NotFound`] when the document is not there and
/// [`RunError::Unreadable`] when it is not one this version understands.
pub(crate) fn report(run: &ResolvedRun) -> Result<Report, RunError> {
    let text = document(run)?;
    json::parse(&text).map_err(|source| RunError::Unreadable {
        run: run.id().to_string(),
        source,
    })
}
