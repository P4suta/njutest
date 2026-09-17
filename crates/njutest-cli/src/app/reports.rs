// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a completed verification is kept.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::{self, ErrorCode};
use crate::report::{Report, json};

/// Where the runs of one report directory live, and the only thing that knows the layout.
///
/// Every place that wanted a path used to join a constant, tests included, so
/// the layout was a fact spread across the tree and the configuration could
/// not name it without moving all of them. Production and a test now ask the
/// same value the same way, and a project that already means something by
/// `reports/` can say so.
#[derive(Debug, Clone)]
pub struct Store {
    runs: PathBuf,
    root: PathBuf,
    at: PathBuf,
    configured: PathBuf,
}

impl Store {
    /// The store `configured` names under `root`.
    #[must_use]
    pub fn of(root: &Path, configured: &Path) -> Self {
        let here = root.join(configured);
        Self {
            runs: here.join(RUNS_NAME),
            root: here,
            at: root.to_path_buf(),
            configured: configured.to_path_buf(),
        }
    }

    /// Where this project writes, relative to its own root, which is what the engine is told to keep out of a snapshot.
    #[must_use]
    pub fn relative(&self) -> String {
        self.configured.to_string_lossy().into_owned()
    }

    /// The store a command uses when it has read the configuration.
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let configured = crate::config::Config::load(root).map_or_else(
            |_error| PathBuf::from(crate::config::DEFAULT_REPORTS_DIRECTORY),
            |config| config.reports.directory,
        );
        Self::of(root, &configured)
    }

    /// The directory every run writes its own directory under.
    #[must_use]
    pub fn runs(&self) -> &Path {
        &self.runs
    }

    /// Where one run writes.
    #[must_use]
    pub fn run(&self, id: &str) -> PathBuf {
        self.runs.join(id)
    }

    /// The directory of the run one index names, or nothing when it names none.
    ///
    /// An index names a run the way somebody standing in the project would:
    /// a jq one-liner and a person reading the file want the same path, and a
    /// reader who joined it onto the wrong root would be reading a run nobody
    /// stored.
    #[must_use]
    pub fn run_of(&self, index: Index) -> Option<PathBuf> {
        let text = std::fs::read_to_string(self.index(index)).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let directory = value.get("directory")?.as_str()?;
        Some(self.at.join(directory))
    }

    /// Where one index that names the newest run sits.
    #[must_use]
    pub fn index(&self, index: Index) -> PathBuf {
        self.root.join(index.file())
    }

    /// What an index calls one run's directory, which is where it is from the project's own root.
    #[must_use]
    pub fn named(&self, run_id: &str) -> String {
        format!("{}/{RUNS_NAME}/{run_id}", self.relative())
    }
}

/// The directory runs sit in, under the report directory the configuration names.
const RUNS_NAME: &str = "runs";

/// One of the two files that name the newest run.
///
/// A file name is not a path: joined onto the wrong root it reads as a run
/// nobody stored, which is what happened to six tests when the report
/// directory became configuration. Only [`Store`] turns one into a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Index {
    /// The latest completed run of any scope.
    Any,
    /// The latest completed run that looked at the whole project.
    Full,
}

impl Index {
    /// Both, in the order a run advances them.
    pub const BOTH: [Self; 2] = [Self::Any, Self::Full];

    /// What the file is called inside the report directory.
    #[must_use]
    pub const fn file(self) -> &'static str {
        match self {
            Self::Any => "latest-any.json",
            Self::Full => "latest-full.json",
        }
    }
}

/// The canonical document inside a run directory.
pub const DOCUMENT_NAME: &str = "njutest-assurance-report-v1.json";

/// The published schema, copied in beside the document it describes.
pub const SCHEMA_NAME: &str = "njutest-assurance-report-v1.schema.json";

const SCHEMA_TEXT: &str = include_str!("../../../../schema/njutest-assurance-report-v1.json");

/// The page a person opens.
pub const HTML_NAME: &str = "njutest-assurance-report-v1.html";

/// The findings, for a code-scanning surface.
pub const SARIF_NAME: &str = "njutest-assurance-report-v1.sarif";

/// The targets and findings, for a continuous integration surface.
pub const JUNIT_NAME: &str = "njutest-assurance-report-v1.junit.xml";

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
    let store = Store::read(root);
    let directory = store.run(&report.run_id);

    disowned(store.runs());
    let document = directory.join(DOCUMENT_NAME);
    write(&document, document_text.as_bytes())?;
    write(&directory.join(SCHEMA_NAME), SCHEMA_TEXT.as_bytes())?;
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

    point(&store, Index::Any, &report.run_id)?;
    if report.run_kind == crate::report::RunKind::Full {
        point(&store, Index::Full, &report.run_id)?;
    }
    Ok(Kept {
        directory,
        document,
    })
}

/// Removes the oldest run directories beyond `keep`, newest first by name — which is chronological, because that is what a run identity is for.
#[must_use]
pub fn retain(root: &Path, keep: u32) -> Vec<PathBuf> {
    let runs = Store::read(root).runs().to_path_buf();
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
    let protected: Vec<String> = Index::BOTH
        .iter()
        .filter_map(|index| pointed_at(root, *index))
        .collect();

    let keep = usize::try_from(keep).unwrap_or(usize::MAX);
    let collectable: Vec<PathBuf> = names
        .into_iter()
        .skip(keep)
        .filter(|path| {
            let candidate = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            !protected.contains(&candidate)
        })
        .collect();
    rust_mutants::reclaim::all(collectable.iter().map(PathBuf::as_path)).removed
}

/// The run one index names, if it names one.
#[must_use]
pub fn pointed_at(root: &Path, index: Index) -> Option<String> {
    let text = std::fs::read_to_string(Store::read(root).index(index)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Says, inside the directory this tool writes, that git has no business with what is in it.
fn disowned(directory: &Path) {
    if !directory.is_dir() {
        return;
    }
    let path = directory.join(".gitignore");
    if path.exists() {
        return;
    }
    drop(std::fs::write(&path, b"*\n"));
}

/// Writes one index.
fn point(store: &Store, index: Index, run_id: &str) -> Result<(), StoreError> {
    let path = store.index(index);
    let mut text = serde_json::to_string_pretty(&serde_json::json!({
        "schema": crate::report::SCHEMA,
        "run_id": run_id,
        "directory": store.named(run_id),
    }))
    .unwrap_or_default();
    text.push('\n');
    write(&path, text.as_bytes())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    rust_mutants::replace::file(path, bytes).map_err(|failure| StoreError::NotKept {
        path: failure.path.display().to_string(),
        source: failure.source,
    })
}
