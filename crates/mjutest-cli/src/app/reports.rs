// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a completed verification is kept.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::{self, ErrorCode};
use crate::report::{Report, json};

/// The directory every run directory lives under.
pub const RUNS_DIR: &str = "reports/runs";

/// The index of the latest completed run of any scope.
pub const LATEST_ANY: &str = "reports/latest-any.json";

/// The index of the latest completed full run.
pub const LATEST_FULL: &str = "reports/latest-full.json";

/// The canonical document inside a run directory.
pub const DOCUMENT_NAME: &str = "mjutest-assurance-report-v1.json";

/// The published schema, copied in beside the document it describes.
pub const SCHEMA_NAME: &str = "mjutest-assurance-report-v1.schema.json";

/// The page a person opens.
pub const HTML_NAME: &str = "mjutest-assurance-report-v1.html";

/// The findings, for a code-scanning surface.
pub const SARIF_NAME: &str = "mjutest-assurance-report-v1.sarif";

/// The targets and findings, for a continuous integration surface.
pub const JUNIT_NAME: &str = "mjutest-assurance-report-v1.junit.xml";

/// Why a report could not be kept.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The report itself is not one that may be persisted.
    #[error(transparent)]
    Report(#[from] json::ReportError),
    /// The run directory could not be written.
    #[error("{}: writing {path}: {source}", error::REPORT_NOT_KEPT.code)]
    NotKept {
        /// The path.
        path: String,
        /// The failure.
        #[source]
        source: io::Error,
    },
}

impl StoreError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Report(error) => error.code(),
            Self::NotKept { .. } => error::REPORT_NOT_KEPT,
        }
    }
}

/// Where one run's report was kept.
#[derive(Debug, Clone)]
pub struct Kept {
    /// The run's directory.
    pub directory: PathBuf,
    /// The canonical document.
    pub document: PathBuf,
}

/// Writes `report` into its own directory under `root`, then points the indexes at it.
///
/// # Errors
/// [`StoreError::Report`] when the report fails its own audit — nothing is
/// written then — and [`StoreError::NotKept`] for the I/O failure.
pub fn keep(root: &Path, report: &Report) -> Result<Kept, StoreError> {
    let document_text = json::document(report)?;
    let directory = root.join(RUNS_DIR).join(&report.run_id);
    create(&directory)?;

    let document = directory.join(DOCUMENT_NAME);
    write(&document, document_text.as_bytes())?;
    write(
        &directory.join(crate::report::lines::FILE_NAME),
        crate::report::lines::stream(report).as_bytes(),
    )?;

    write(
        &directory.join(HTML_NAME),
        crate::report::html::document(report).as_bytes(),
    )?;
    write(
        &directory.join(SARIF_NAME),
        format!("{:#}\n", crate::report::sarif::document(report)).as_bytes(),
    )?;
    write(
        &directory.join(JUNIT_NAME),
        crate::report::junit::document(report).as_bytes(),
    )?;

    point(root, LATEST_ANY, &report.run_id)?;
    if report.run_kind == crate::report::RunKind::Full {
        point(root, LATEST_FULL, &report.run_id)?;
    }
    Ok(Kept {
        directory,
        document,
    })
}

/// Removes the oldest run directories beyond `keep`, newest first by name — which is chronological, because that is what a run identity is for.
#[must_use]
pub fn retain(root: &Path, keep: u32) -> Vec<PathBuf> {
    let runs = root.join(RUNS_DIR);
    let mut names: Vec<PathBuf> = match std::fs::read_dir(&runs) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect(),
        Err(_error) => return Vec::new(),
    };
    names.sort();
    names.reverse();
    let protected: Vec<String> = [LATEST_ANY, LATEST_FULL]
        .iter()
        .filter_map(|index| pointed_at(root, index))
        .collect();

    let mut removed = Vec::new();
    let keep = usize::try_from(keep).unwrap_or(usize::MAX);
    for path in names.into_iter().skip(keep) {
        let candidate = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if protected.contains(&candidate) {
            continue;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

/// The run one index names, if it names one.
#[must_use]
pub fn pointed_at(root: &Path, index: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join(index)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Writes one index.
fn point(root: &Path, index: &str, run_id: &str) -> Result<(), StoreError> {
    let path = root.join(index);
    if let Some(parent) = path.parent() {
        create(parent)?;
    }
    let mut text = serde_json::to_string_pretty(&serde_json::json!({
        "schema": crate::report::SCHEMA,
        "run_id": run_id,
        "directory": format!("{RUNS_DIR}/{run_id}"),
    }))
    .unwrap_or_default();
    text.push('\n');
    write(&path, text.as_bytes())
}

fn create(directory: &Path) -> Result<(), StoreError> {
    std::fs::create_dir_all(directory).map_err(|source| StoreError::NotKept {
        path: directory.display().to_string(),
        source,
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    std::fs::write(path, bytes).map_err(|source| StoreError::NotKept {
        path: path.display().to_string(),
        source,
    })
}
