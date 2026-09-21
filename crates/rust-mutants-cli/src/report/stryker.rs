// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as a mutation testing report every Stryker reader understands.

use std::collections::BTreeMap;
use std::path::Path;

use rust_mutants::outcome::Outcome;
use serde::Serialize;

use super::run::RunDocument;

/// The report schema version this projection writes.
pub const SCHEMA_VERSION: &str = "2.0";

/// The language every file of this projection is in.
pub const LANGUAGE: &str = "rust";

/// One mutation testing report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    /// The schema this document answers to.
    pub schema_version: String,
    /// The bounds a reader colours by.
    pub thresholds: Thresholds,
    /// The tree the paths are relative to.
    pub project_root: String,
    /// Every mutated file, by workspace-relative path.
    pub files: BTreeMap<String, FileResult>,
}

/// The bounds a reader colours by.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Thresholds {
    /// At or above this, a reader shows green.
    pub high: u32,
    /// Below this, a reader shows red.
    pub low: u32,
}

/// One mutated file.
#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    /// What the file is written in.
    pub language: String,
    /// The whole file, so a reader can show the mutation in place.
    pub source: String,
    /// Every mutant of it.
    pub mutants: Vec<MutantResult>,
}

/// One mutation, as the schema spells it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutantResult {
    /// What names this mutation across runs: the place it is in, not the file's bytes, which any edit re-mints.
    pub id: String,
    /// The rule that proposed it.
    pub mutator_name: String,
    /// What the bytes became.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    /// Where it is, in UTF-16 columns.
    pub location: Location,
    /// What the run established.
    pub status: String,
    /// Why the status is what it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,
    /// The targets that noticed it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub killed_by: Vec<String>,
    /// How long its executions took together.
    pub duration: u64,
}

/// Where a mutation is: start inclusive, end exclusive.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Location {
    /// Where it starts.
    pub start: Position,
    /// Where it ends.
    pub end: Position,
}

/// A place in a file: a 1-based line and a 1-based UTF-16 column, which is what this schema counts in.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Position {
    /// The 1-based line.
    pub line: u32,
    /// The 1-based UTF-16 column.
    pub column: u32,
}

/// Projects one run report from the sources it names, with the thresholds a reader colours by.
///
/// # Errors
/// [`crate::error::CliError::SourceUnreadable`] when a file the report names is not under
/// `root`.
pub fn project(
    document: &RunDocument,
    root: &Path,
    thresholds: Thresholds,
    sources: &BTreeMap<String, super::sources::Held>,
) -> Result<Projection, crate::error::CliError> {
    let project_root =
        rust_mutants::id::slashed(root).map_err(|source| crate::error::CliError::PathNotUtf8 {
            context: "the Stryker project root has no exact UTF-8 spelling",
            source,
        })?;
    let mut files: BTreeMap<String, FileResult> = BTreeMap::new();
    for mutant in &document.mutants {
        if !files.contains_key(&mutant.path) {
            let source = match sources.get(&mutant.path) {
                Some(super::sources::Held::Measured(text)) => text.clone(),
                Some(super::sources::Held::Changed) => {
                    return Err(crate::error::CliError::moved_on(&mutant.path));
                }
                None => return Err(crate::error::CliError::absent(&mutant.path, root)),
            };
            files.insert(
                mutant.path.clone(),
                FileResult {
                    language: LANGUAGE.to_owned(),
                    source,
                    mutants: Vec::new(),
                },
            );
        }
        let Some(file) = files.get_mut(&mutant.path) else {
            continue;
        };
        let location = located(&file.source, mutant.line, mutant.column, &mutant.original)?;
        file.mutants.push(MutantResult {
            id: format!(
                "{}:{}:{}:{}",
                mutant.path, mutant.item, mutant.rule, mutant.original
            ),
            mutator_name: mutant.rule.clone(),
            replacement: (!mutant.replacement.is_empty()).then(|| mutant.replacement.clone()),
            location,
            status: status_of(mutant).to_owned(),
            status_reason: reason_of(mutant),
            killed_by: killed_by(mutant),
            duration: mutant.duration_ms,
        });
    }
    Ok(Projection {
        schema_version: SCHEMA_VERSION.to_owned(),
        thresholds,
        project_root,
        files,
    })
}

/// What this run's outcome is called in the other vocabulary.
const fn status_of(mutant: &super::run::RunMutantDocument) -> &'static str {
    match mutant.outcome {
        Outcome::Killed => "Killed",
        Outcome::Survived => "Survived",
        Outcome::Inconclusive | Outcome::Errored => "RuntimeError",
        Outcome::NotRun if mutant.unreached => "NoCoverage",
        Outcome::NotRun | Outcome::StepLimitReached | Outcome::Waited => "Pending",
    }
}

/// What a reader is told about the status.
fn reason_of(mutant: &super::run::RunMutantDocument) -> Option<String> {
    match mutant.outcome {
        Outcome::StepLimitReached => Some(
            "the execution reached its configured guard-take limit without deciding the mutation"
                .to_owned(),
        ),
        Outcome::Waited => Some(
            "this machine stopped waiting twice, so the run did not decide the mutation".to_owned(),
        ),
        Outcome::Inconclusive => Some(
            "one timeout that did not reproduce, so the run cannot say what the tests noticed"
                .to_owned(),
        ),
        Outcome::Errored => Some(format!(
            "the harness itself failed with exit {}",
            mutant.exit_code
        )),
        Outcome::NotRun if mutant.unreached => Some("no measured test reaches it".to_owned()),
        Outcome::NotRun => Some("this run did not execute it".to_owned()),
        Outcome::Killed | Outcome::Survived => None,
    }
}

/// The tests that noticed it, and the target that held them when the harness did not name one.
fn killed_by(mutant: &super::run::RunMutantDocument) -> Vec<String> {
    if mutant.outcome != Outcome::Killed {
        return Vec::new();
    }
    if !mutant.killed_by.is_empty() {
        return mutant.killed_by.clone();
    }
    if mutant.target.is_empty() {
        Vec::new()
    } else {
        vec![mutant.target.clone()]
    }
}

/// Where one mutation is, counted in UTF-16 as this schema requires.
fn located(
    source: &str,
    line: u32,
    column: u32,
    original: &str,
) -> Result<Location, crate::error::CliError> {
    let start = Position {
        line,
        column: utf16_column(source, line, column)?,
    };
    let end = end_of(source, line, column, original)?;
    Ok(Location { start, end })
}

/// The 1-based UTF-16 column of a 1-based byte column on `line`.
fn utf16_column(source: &str, line: u32, byte_column: u32) -> Result<u32, crate::error::CliError> {
    let Some(text) = line_of(source, line) else {
        return Ok(1);
    };
    let zero_based =
        byte_column
            .checked_sub(1)
            .ok_or(crate::error::CliError::ProjectionOverflow {
                projection: "Stryker",
                field: "a zero byte column",
            })?;
    let bytes = usize::try_from(zero_based).map_err(|_overflow| {
        crate::error::CliError::ProjectionOverflow {
            projection: "Stryker",
            field: "a byte column",
        }
    })?;
    let prefix = match text.get(..bytes.min(text.len())) {
        Some(prefix) => prefix,
        None => text,
    };
    let units: usize = prefix.chars().map(char::len_utf16).sum();
    let one_based = units
        .checked_add(1)
        .ok_or(crate::error::CliError::ProjectionOverflow {
            projection: "Stryker",
            field: "a UTF-16 column",
        })?;
    u32::try_from(one_based).map_err(|_overflow| crate::error::CliError::ProjectionOverflow {
        projection: "Stryker",
        field: "a UTF-16 column",
    })
}

/// Where a mutation ends: one past its last byte, on whichever line that is.
fn end_of(
    source: &str,
    line: u32,
    column: u32,
    original: &str,
) -> Result<Position, crate::error::CliError> {
    let lines = original.lines().count().max(1);
    let additional = lines
        .checked_sub(1)
        .ok_or(crate::error::CliError::ProjectionOverflow {
            projection: "Stryker",
            field: "a mutation's empty line span",
        })?;
    let additional = u32::try_from(additional).map_err(|_overflow| {
        crate::error::CliError::ProjectionOverflow {
            projection: "Stryker",
            field: "a mutation's line span",
        }
    })?;
    let last = line
        .checked_add(additional)
        .ok_or(crate::error::CliError::ProjectionOverflow {
            projection: "Stryker",
            field: "a mutation's ending line",
        })?;
    if lines == 1 {
        let width = original.chars().map(char::len_utf16).sum::<usize>();
        let width = u32::try_from(width).map_err(|_overflow| {
            crate::error::CliError::ProjectionOverflow {
                projection: "Stryker",
                field: "a mutation's UTF-16 width",
            }
        })?;
        let column = utf16_column(source, line, column)?
            .checked_add(width)
            .ok_or(crate::error::CliError::ProjectionOverflow {
                projection: "Stryker",
                field: "a mutation's ending column",
            })?;
        return Ok(Position { line: last, column });
    }
    let tail = match original.rsplit('\n').next() {
        Some(tail) => tail,
        None => "",
    };
    let units: usize = tail.chars().map(char::len_utf16).sum();
    let one_based = units
        .checked_add(1)
        .ok_or(crate::error::CliError::ProjectionOverflow {
            projection: "Stryker",
            field: "a mutation's ending UTF-16 column",
        })?;
    let column = u32::try_from(one_based).map_err(|_overflow| {
        crate::error::CliError::ProjectionOverflow {
            projection: "Stryker",
            field: "a mutation's ending UTF-16 column",
        }
    })?;
    Ok(Position { line: last, column })
}

/// The text of one 1-based line, without its ending.
fn line_of(source: &str, line: u32) -> Option<&str> {
    let zero_based = line.checked_sub(1)?;
    let Ok(index) = usize::try_from(zero_based) else {
        return None;
    };
    source.lines().nth(index)
}
