// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run leaves behind: where its report is stored, and how many of them are kept.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::id::{RunId, RunIdError, StoredRunId, StoredRunIdError};

use super::{json_line, trace};
use crate::cli;
use crate::error::CliError;
use crate::report::run as run_report;

/// Where this project keeps what its runs leave behind, and the only thing that knows it.
///
/// The directory is configuration, so every command has to ask the same
/// question the same way. Three of them used to join the default instead, and
/// a project that had moved the directory got two commands reading a place
/// nothing was written to.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The store `configured` names under `root`.
    #[must_use]
    pub fn of(root: &Path, configured: &Path) -> Self {
        Self {
            root: root.join(configured),
        }
    }

    /// The store this project is configured for, defaulting when the configuration cannot be read.
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let configured = match crate::config::Config::load(root) {
            Ok(config) => config.reports.directory,
            Err(_unreadable_configuration) => {
                PathBuf::from(crate::config::DEFAULT_REPORTS_DIRECTORY)
            }
        };
        Self::of(root, &configured)
    }

    /// The directory every run writes its own directory under.
    #[must_use]
    pub fn root(&self) -> PathBuf {
        self.root.clone()
    }
}

/// The name this run goes by, which is what its report directory is called.
///
/// # Errors
/// [`CliError::InvalidValue`] for a name that is not one a directory can be.
pub fn named(command: &cli::Command, now: Timestamp) -> Result<RunId, CliError> {
    let cli::Command::Run {
        run_id: Some(name), ..
    } = command
    else {
        return run_id(now).map_err(|error| invalid_run_id("<generated>", &error));
    };
    RunId::try_from(name.clone()).map_err(|error| invalid_run_id(name, &error))
}

/// The name a run goes by when nobody named it: the instant it started, which sorts chronologically as a directory name.
///
/// # Errors
/// Returns the canonical run-name failure if the formatted timestamp cannot be represented.
pub fn run_id(now: Timestamp) -> Result<RunId, RunIdError> {
    let name = now
        .strftime("%Y%m%dT%H%M%S%3fZ")
        .to_string()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    RunId::try_from(name)
}

/// Writes the report under `directory/<id>/`, and the pointer that names the newest run.
///
/// # Errors
/// [`CliError::WriteFailed`] when either the report or the pointer cannot be
/// written. A report stored under a pointer that still names the run before it
/// is read as that run's, so both are written or the run says it failed.
pub fn store(
    directory: &Path,
    id: &RunId,
    document: &run_report::RunDocument,
) -> Result<PathBuf, CliError> {
    ensure_writable_spelling(directory, id)?;
    let run_directory = directory.join(id.as_str());
    validate_real_directory(&run_directory)?;
    let path = run_directory.join(run_report::FILE_NAME);
    rust_mutants::replace::file(&path, json_line(document)?.as_bytes())
        .map_err(|failure| CliError::writing(&failure.path, failure.source))?;
    disowned(directory)?;
    let latest = directory.join(run_report::LATEST_FILE_NAME);
    let pointer = serde_json::json!({
        "document_type": "rust-mutants/latest-run",
        "schema_version": 1,
        "run": id.as_str(),
        "document": format!("{id}/{}", run_report::FILE_NAME),
    });
    rust_mutants::replace::file(&latest, json_line(&pointer)?.as_bytes())
        .map_err(|failure| CliError::writing(&failure.path, failure.source))?;
    Ok(path)
}

/// Says, inside the directory this tool writes, that git has no business with what is in it.
fn disowned(directory: &Path) -> Result<(), CliError> {
    let path = directory.join(".gitignore");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => return Ok(()),
        Ok(_unsafe_or_unexpected_type) => {
            return Err(CliError::StoredRunCorrupt {
                path,
                message: ".gitignore is not a regular file".to_owned(),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(CliError::writing(&path, source)),
    }
    std::fs::write(&path, b"*\n").map_err(|source| CliError::writing(&path, source))
}

/// Keeps the newest `keep` stored runs and the newest `keep` recordings of the other commands. Both sort chronologically by name, so the oldest are the first.
///
/// # Errors
/// [`CliError::StoredRunsUnreadable`] when the directories cannot be enumerated completely.
pub fn prune(directory: &Path, keep: u32) -> Result<(), CliError> {
    if keep == 0 {
        return Ok(());
    }
    let (runs, recordings) = kept(directory)?;
    oldest(&runs, keep)?;
    oldest(&recordings, keep)?;
    oldest(
        &subdirectories(&directory.join(trace::TRACES_DIRECTORY_NAME))?,
        keep,
    )?;
    Ok(())
}

/// The stored runs and, apart from them, the directories a run that wrote no report left a recording in.
///
/// # Errors
/// [`CliError::StoredRunsUnreadable`] when the directory cannot be enumerated completely.
pub fn kept(directory: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), CliError> {
    validate_spellings(directory)?;
    let mut runs = Vec::new();
    let mut recordings = Vec::new();
    for path in subdirectories(directory)? {
        if path
            .file_name()
            .is_some_and(|name| name == trace::TRACES_DIRECTORY_NAME)
        {
            continue;
        }
        let name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| CliError::StoredRunCorrupt {
                path: path.clone(),
                message: "the run directory name is not UTF-8".to_owned(),
            })?;
        validate_run_name(name, &path)?;
        if real_file(&path.join(run_report::FILE_NAME))? {
            runs.push(path);
        } else if real_directory(&path.join(trace::RUN_DIRECTORY_NAME))? {
            recordings.push(path);
        }
    }
    Ok((runs, recordings))
}

/// Removes everything but the newest `keep` of `directories`, within a budget.
///
/// This runs on every run that stores a report, so it is the one thing here
/// that must be faster than what it is cleaning up after: a directory on a
/// wedged mount takes minutes to refuse, and the run cannot exit until it
/// does. What is not reached stays, is still the oldest, and is what the next
/// run starts with.
///
/// # Errors
/// Returns the exact count-conversion or cleanup failure; a partial cleanup is never called whole.
pub fn oldest(directories: &[PathBuf], keep: u32) -> Result<(), CliError> {
    let keep = usize::try_from(keep).map_err(|source| CliError::InvalidValue {
        flag: "reports.keep".to_owned(),
        value: keep.to_string(),
        expected: format!("a count representable on this platform: {source}"),
    })?;
    let Some(excess) = directories.len().checked_sub(keep) else {
        return Ok(());
    };
    let reclaimed =
        rust_mutants::reclaim::all(directories.iter().take(excess).map(PathBuf::as_path));
    let Some(path) = reclaimed.left().first().copied() else {
        return Ok(());
    };
    let refused = reclaimed
        .refused
        .iter()
        .map(|(path, reason)| format!("{}: {reason}", path.display()))
        .chain(
            reclaimed
                .unreached
                .iter()
                .map(|path| format!("{}: cleanup budget exhausted", path.display())),
        )
        .collect::<Vec<_>>()
        .join("; ");
    Err(CliError::writing(path, std::io::Error::other(refused)))
}

/// Every directory directly under `directory`, in name order.
///
/// # Errors
/// [`CliError::StoredRunsUnreadable`] when opening the directory, reading an entry, or reading its
/// type fails. A partial list is never reported as the complete set of stored runs.
pub fn subdirectories(directory: &Path) -> Result<Vec<PathBuf>, CliError> {
    let unreadable = |source| CliError::StoredRunsUnreadable {
        path: directory.to_path_buf(),
        source,
    };
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(unreadable(error)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(&unreadable)?;
        if entry.file_type().map_err(&unreadable)?.is_dir() {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

/// The report of the run a command was told to read, or of the newest when it was told nothing.
///
/// # Errors
/// [`CliError::ReportMissing`] when `named` is not a stored run under
/// `directory`, and whatever [`newest`] refuses when nothing is named.
pub fn report_of(directory: &Path, named: Option<&str>) -> Result<PathBuf, CliError> {
    let Some(named) = named else {
        return newest(directory);
    };
    let run_id =
        StoredRunId::try_from(named).map_err(|error| invalid_stored_run_id(named, &error))?;
    let run_directory = directory.join(run_id.as_str());
    if !real_directory(&run_directory)? {
        return Err(CliError::ReportMissing {
            message: format!(
                "{named:?} names no stored run under {}",
                directory.display()
            ),
        });
    }
    let path = run_directory.join(run_report::FILE_NAME);
    if real_file(&path)? {
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
    validate_spellings(directory)?;
    let missing = || CliError::ReportMissing {
        message: format!("no run report is stored under {}", directory.display()),
    };
    let pointer = directory.join(run_report::LATEST_FILE_NAME);
    let pointer_exists = real_file(&pointer)?;
    match pointer_exists
        .then(|| std::fs::read_to_string(&pointer))
        .transpose()
    {
        Ok(Some(text)) => {
            let value =
                crate::strictjson::decode_str::<serde_json::Value>(&text).map_err(|error| {
                    CliError::StoredRunCorrupt {
                        path: pointer.clone(),
                        message: error.to_string(),
                    }
                })?;
            let run = value
                .get("run")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| CliError::StoredRunCorrupt {
                    path: pointer.clone(),
                    message: "the pointer has no string run".to_owned(),
                })?;
            let run = StoredRunId::try_from(run).map_err(|error| CliError::StoredRunCorrupt {
                path: pointer.clone(),
                message: error.to_string(),
            })?;
            let run_directory = directory.join(run.as_str());
            if !real_directory(&run_directory)? {
                return newest_by_name(directory);
            }
            let path = run_directory.join(run_report::FILE_NAME);
            if real_file(&path)? {
                return Ok(path);
            }
        }
        Ok(None) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(CliError::StoredRunsUnreadable {
                path: pointer,
                source,
            });
        }
    }
    newest_by_name(directory).map_err(|error| match error {
        CliError::ReportMissing { .. } => missing(),
        other @ (CliError::Engine(_)
        | CliError::Config(_)
        | CliError::Evidence(_)
        | CliError::TraceSummary { .. }
        | CliError::RouteAccounting { .. }
        | CliError::WorkAccounting { .. }
        | CliError::InvalidCandidate { .. }
        | CliError::CandidateTextNotUtf8 { .. }
        | CliError::CandidatePosition { .. }
        | CliError::EnvironmentReserved { .. }
        | CliError::StoredRunsUnreadable { .. }
        | CliError::StoredRunCorrupt { .. }
        | CliError::CacheUnreadable { .. }
        | CliError::KeptLedgerUnreadable { .. }
        | CliError::FileExists { .. }
        | CliError::Shard { .. }
        | CliError::ChangeSetUnavailable { .. }
        | CliError::InvalidValue { .. }
        | CliError::PathNotUtf8 { .. }
        | CliError::SourceUnreadable { .. }
        | CliError::SourceTextNotUtf8 { .. }
        | CliError::WriteFailed { .. }
        | CliError::OutputFailed { .. }
        | CliError::OutputEncodingFailed { .. }
        | CliError::ProjectionOverflow { .. }
        | CliError::PreparationStartFailed { .. }
        | CliError::PreparationPanicked) => other,
    })
}

fn newest_by_name(directory: &Path) -> Result<PathBuf, CliError> {
    validate_spellings(directory)?;
    let missing = || CliError::ReportMissing {
        message: format!("no run report is stored under {}", directory.display()),
    };
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(missing()),
        Err(source) => {
            return Err(CliError::StoredRunsUnreadable {
                path: directory.to_path_buf(),
                source,
            });
        }
    };
    let mut runs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| CliError::StoredRunsUnreadable {
            path: directory.to_path_buf(),
            source,
        })?;
        let entry_path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| CliError::StoredRunCorrupt {
            path: entry_path.clone(),
            message: "the run directory name is not UTF-8".to_owned(),
        })?;
        let run = match StoredRunId::try_from(name) {
            Ok(run) => run,
            Err(_not_a_run_directory) => continue,
        };
        let entry_type = entry
            .file_type()
            .map_err(|source| CliError::StoredRunsUnreadable {
                path: entry_path.clone(),
                source,
            })?;
        if !entry_type.is_dir() {
            if entry_type.is_symlink() {
                return Err(CliError::StoredRunCorrupt {
                    path: entry_path,
                    message: "a run entry is a symlink rather than a directory".to_owned(),
                });
            }
            continue;
        }
        let path = directory.join(run.as_str()).join(run_report::FILE_NAME);
        if real_file(&path)? {
            runs.push(path);
        }
    }
    runs.sort();
    runs.pop().ok_or_else(missing)
}

fn ensure_writable_spelling(directory: &Path, run: &RunId) -> Result<(), CliError> {
    let spellings = stored_spellings(directory)?;
    let wanted = StoredRunId::from(run);
    if let Some((existing, path)) = spellings.get(&wanted.case_folded())
        && existing != &wanted
    {
        return Err(CliError::StoredRunCorrupt {
            path: path.clone(),
            message: format!(
                "stored run {existing:?} aliases new writable run {wanted:?} by ASCII case"
            ),
        });
    }
    Ok(())
}

fn validate_spellings(directory: &Path) -> Result<(), CliError> {
    match stored_spellings(directory) {
        Ok(_) => Ok(()),
        Err(error) => Err(error),
    }
}

fn validate_run_name(name: &str, path: &Path) -> Result<(), CliError> {
    match StoredRunId::try_from(name) {
        Ok(_) => Ok(()),
        Err(error) => Err(CliError::StoredRunCorrupt {
            path: path.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

fn stored_spellings(
    directory: &Path,
) -> Result<BTreeMap<String, (StoredRunId, PathBuf)>, CliError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(source) => {
            return Err(CliError::StoredRunsUnreadable {
                path: directory.to_path_buf(),
                source,
            });
        }
    };
    let mut spellings = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|source| CliError::StoredRunsUnreadable {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(run) = StoredRunId::try_from(name) else {
            continue;
        };
        remember_unique_spelling(&mut spellings, &run, &path)?;
    }
    Ok(spellings)
}

fn remember_unique_spelling(
    spellings: &mut BTreeMap<String, (StoredRunId, PathBuf)>,
    run: &StoredRunId,
    path: &Path,
) -> Result<(), CliError> {
    let folded = run.case_folded();
    if let Some((other, other_path)) = spellings.insert(folded, (run.clone(), path.to_path_buf())) {
        return Err(CliError::StoredRunCorrupt {
            path: path.to_path_buf(),
            message: format!(
                "run ids {other:?} at {} and {run:?} differ only by ASCII case",
                other_path.display()
            ),
        });
    }
    Ok(())
}

fn real_directory(path: &Path) -> Result<bool, CliError> {
    real_entry(path, "directory", std::fs::FileType::is_dir)
}

fn validate_real_directory(path: &Path) -> Result<(), CliError> {
    match real_directory(path) {
        Ok(true | false) => Ok(()),
        Err(error) => Err(error),
    }
}

fn real_file(path: &Path) -> Result<bool, CliError> {
    real_entry(path, "regular file", std::fs::FileType::is_file)
}

fn real_entry(
    path: &Path,
    expected: &str,
    accepts: impl FnOnce(&std::fs::FileType) -> bool,
) -> Result<bool, CliError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if accepts(&metadata.file_type()) => Ok(true),
        Ok(_unsafe_or_unexpected_type) => Err(CliError::StoredRunCorrupt {
            path: path.to_path_buf(),
            message: format!("the entry is not a real {expected}"),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(CliError::StoredRunsUnreadable {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn invalid_run_id(value: &str, error: &RunIdError) -> CliError {
    CliError::InvalidValue {
        flag: "--run-id".to_owned(),
        value: value.to_owned(),
        expected: error.to_string(),
    }
}

fn invalid_stored_run_id(value: &str, error: &StoredRunIdError) -> CliError {
    CliError::InvalidValue {
        flag: "--run".to_owned(),
        value: value.to_owned(),
        expected: error.to_string(),
    }
}
