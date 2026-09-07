// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind: where its report is stored, and how many of them are kept.

use std::path::{Path, PathBuf};

use jiff::Timestamp;

use super::{json_line, trace};
use crate::cli;
use crate::error::CliError;
use crate::report::run as run_report;

/// The name this run goes by, which is what its report directory is called.
///
/// # Errors
/// [`CliError::InvalidValue`] for a name that is not one a directory can be.
pub(super) fn named(command: &cli::Command, now: Timestamp) -> Result<String, CliError> {
    let cli::Command::Run {
        run_id: Some(name), ..
    } = command
    else {
        return Ok(run_id(now));
    };
    let shaped = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|one| one.is_ascii_alphanumeric() || matches!(one, '.' | '_' | '-'));
    if shaped {
        Ok(name.clone())
    } else {
        Err(CliError::InvalidValue {
            flag: "--run-id".to_owned(),
            value: name.clone(),
            expected: "1 to 64 of letters, digits, `.`, `_` and `-`".to_owned(),
        })
    }
}

/// The name a run goes by when nobody named it: the instant it started, which sorts chronologically as a directory name.
#[must_use]
pub fn run_id(now: Timestamp) -> String {
    now.strftime("%Y%m%dT%H%M%S%3fZ")
        .to_string()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Writes the report under `directory/<id>/`, and the pointer that names the newest run.
pub(super) fn store(
    directory: &Path,
    id: &str,
    document: &run_report::RunDocument,
) -> Result<PathBuf, CliError> {
    let dir = directory.join(id);
    std::fs::create_dir_all(&dir).map_err(|source| CliError::writing(&dir, source))?;
    let path = dir.join(run_report::FILE_NAME);
    std::fs::write(&path, json_line(document))
        .map_err(|source| CliError::writing(&path, source))?;
    let latest = directory.join(run_report::LATEST_FILE_NAME);
    let pointer = serde_json::json!({
        "document_type": "rust-mutants/latest-run",
        "schema_version": 1,
        "run": id,
        "document": format!("{id}/{}", run_report::FILE_NAME),
    });
    std::fs::write(&latest, json_line(&pointer))
        .map_err(|source| CliError::writing(&latest, source))?;
    Ok(path)
}

/// Keeps the newest `keep` stored runs and the newest `keep` recordings of the other commands. Both sort chronologically by name, so the oldest are the first.
///
/// A directory that holds no run report is not a run and never costs a run its
/// place: `traces/` sorts after every run name, and counting it would leave
/// `keep - 1` runs stored.
pub(super) fn prune(directory: &Path, keep: u32) {
    if keep == 0 {
        return;
    }
    let (runs, recordings) = kept(directory);
    oldest(&runs, keep);
    oldest(&recordings, keep);
    oldest(
        &subdirectories(&directory.join(trace::TRACES_DIRECTORY_NAME)),
        keep,
    );
}

/// The stored runs and, apart from them, the directories a run that wrote no report left a recording in.
///
/// A run asked to record and not to report still names itself and still keeps
/// what it recorded, so those directories are bounded by `keep` of their own
/// rather than either counting against the stored runs or growing forever.
fn kept(directory: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut runs = Vec::new();
    let mut recordings = Vec::new();
    for path in subdirectories(directory) {
        if path
            .file_name()
            .is_some_and(|name| name == trace::TRACES_DIRECTORY_NAME)
        {
            continue;
        }
        if path.join(run_report::FILE_NAME).is_file() {
            runs.push(path);
        } else if path.join(trace::RUN_DIRECTORY_NAME).is_dir() {
            recordings.push(path);
        }
    }
    (runs, recordings)
}

/// Removes everything but the newest `keep` of `directories`.
fn oldest(directories: &[PathBuf], keep: u32) {
    let excess = directories
        .len()
        .saturating_sub(usize::try_from(keep).unwrap_or(usize::MAX));
    for old in directories.iter().take(excess) {
        drop(std::fs::remove_dir_all(old));
    }
}

/// Every directory directly under `directory`, in name order.
fn subdirectories(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

/// The newest stored run, by the pointer the last run wrote, or by name when there is no pointer.
pub(super) fn newest(directory: &Path) -> Result<PathBuf, CliError> {
    let missing = || CliError::ReportMissing {
        message: format!("no run report is stored under {}", directory.display()),
    };
    let pointer = directory.join(run_report::LATEST_FILE_NAME);
    if let Ok(text) = std::fs::read_to_string(&pointer)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(relative) = value.get("document").and_then(serde_json::Value::as_str)
    {
        let path = directory.join(relative);
        if path.is_file() {
            return Ok(path);
        }
    }
    let entries = std::fs::read_dir(directory).map_err(|_error| missing())?;
    let mut runs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(run_report::FILE_NAME))
        .filter(|path| path.is_file())
        .collect();
    runs.sort();
    runs.pop().ok_or_else(missing)
}
