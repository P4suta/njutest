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
/// The name becomes a path under the report directory, so `.` and `..` are
/// refused along with everything a separator could hide in: a run named `..`
/// writes its report over the directory that holds every other run, and reads
/// back as this run's when the next person asks about it.
///
/// # Errors
/// [`CliError::InvalidValue`] for a name that is not one a directory can be.
pub fn named(command: &cli::Command, now: Timestamp) -> Result<String, CliError> {
    let cli::Command::Run {
        run_id: Some(name), ..
    } = command
    else {
        return Ok(run_id(now));
    };
    let shaped = !name.is_empty()
        && name.len() <= 64
        && !matches!(name.as_str(), "." | "..")
        && name
            .chars()
            .all(|one| one.is_ascii_alphanumeric() || matches!(one, '.' | '_' | '-'));
    if shaped {
        Ok(name.clone())
    } else {
        Err(CliError::InvalidValue {
            flag: "--run-id".to_owned(),
            value: name.clone(),
            expected: "1 to 64 of letters, digits, `.`, `_` and `-`, and not `.` or `..`"
                .to_owned(),
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
///
/// # Errors
/// [`CliError::WriteFailed`] when either the report or the pointer cannot be
/// written. A report stored under a pointer that still names the run before it
/// is read as that run's, so both are written or the run says it failed.
pub fn store(
    directory: &Path,
    id: &str,
    document: &run_report::RunDocument,
) -> Result<PathBuf, CliError> {
    let path = directory.join(id).join(run_report::FILE_NAME);
    rust_mutants::replace::file(&path, json_line(document).as_bytes())
        .map_err(|failure| CliError::writing(&failure.path, failure.source))?;
    let latest = directory.join(run_report::LATEST_FILE_NAME);
    let pointer = serde_json::json!({
        "document_type": "rust-mutants/latest-run",
        "schema_version": 1,
        "run": id,
        "document": format!("{id}/{}", run_report::FILE_NAME),
    });
    rust_mutants::replace::file(&latest, json_line(&pointer).as_bytes())
        .map_err(|failure| CliError::writing(&failure.path, failure.source))?;
    Ok(path)
}

/// Keeps the newest `keep` stored runs and the newest `keep` recordings of the other commands. Both sort chronologically by name, so the oldest are the first.
///
/// A directory that holds no run report is not a run and never costs a run its
/// place: `traces/` sorts after every run name, and counting it would leave
/// `keep - 1` runs stored.
pub fn prune(directory: &Path, keep: u32) {
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

#[must_use]
/// The stored runs and, apart from them, the directories a run that wrote no report left a recording in.
///
/// A run asked to record and not to report still names itself and still keeps
/// what it recorded, so those directories are bounded by `keep` of their own
/// rather than either counting against the stored runs or growing forever.
pub fn kept(directory: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
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
pub fn oldest(directories: &[PathBuf], keep: u32) {
    let excess = directories
        .len()
        .saturating_sub(usize::try_from(keep).unwrap_or(usize::MAX));
    for old in directories.iter().take(excess) {
        drop(std::fs::remove_dir_all(old));
    }
}

/// Every directory directly under `directory`, in name order.
pub fn subdirectories(directory: &Path) -> Vec<PathBuf> {
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

/// The report of the run a command was told to read, or of the newest when it was told nothing.
///
/// A name is a name a person typed, so one no directory answers to is a
/// refusal rather than a fall back to the newest: a command that read another
/// run than the one it was asked for would answer confidently about the wrong
/// one.
///
/// # Errors
/// [`CliError::ReportMissing`] when `named` is not a stored run under
/// `directory`, and whatever [`newest`] refuses when nothing is named.
pub fn report_of(directory: &Path, named: Option<&str>) -> Result<PathBuf, CliError> {
    let Some(named) = named else {
        return newest(directory);
    };
    let path = directory.join(named).join(run_report::FILE_NAME);
    if path.is_file() {
        Ok(path)
    } else {
        Err(CliError::ReportMissing {
            message: format!(
                "{named:?} names no stored run under {}",
                directory.display()
            ),
        })
    }
}

/// The newest stored run, by the pointer the last run wrote, or by name when there is no pointer.
///
/// # Errors
/// [`CliError::ReportMissing`] when `directory` holds no run report at all,
/// which is what a person asking about a run they never made is told.
pub fn newest(directory: &Path) -> Result<PathBuf, CliError> {
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
