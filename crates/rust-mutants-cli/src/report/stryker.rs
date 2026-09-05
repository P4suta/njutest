// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as a mutation testing report every Stryker reader understands.
//!
//! The projection is lossy on purpose: it says what the schema can say and
//! nothing more. A candidate the compiler refused has no position, so it is
//! not in the document at all rather than placed at a guess, and the
//! `unreached` a coverage-routed run establishes becomes `NoCoverage`, which
//! is the same claim in the other vocabulary.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::run::RunDocument;

/// The report schema version this projection writes.
pub const SCHEMA_VERSION: &str = "2.0";

/// The language every file of this projection is in.
pub const LANGUAGE: &str = "rust";

/// The file a projection is written to.
pub const FILE_NAME: &str = "mutation.json";

/// The thresholds the projection declares, which the schema requires and this engine does not use: a verdict is a claim a reader can check, and a percentage is not.
pub const THRESHOLDS: Thresholds = Thresholds { high: 80, low: 60 };

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
    /// The mutant's own identity, which is stable across runs.
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

/// Projects one run report, reading the sources it names from `root`.
///
/// A file that cannot be read is left out: a reader that cannot show the
/// source cannot show the mutation either, and a document that names a file
/// it has no source for is one no reader can use.
#[must_use]
pub fn project(document: &RunDocument, root: &Path) -> Projection {
    let mut files: BTreeMap<String, FileResult> = BTreeMap::new();
    for mutant in &document.mutants {
        if !files.contains_key(&mutant.path) {
            let Ok(source) = std::fs::read_to_string(root.join(&mutant.path)) else {
                continue;
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
        let location = located(&file.source, mutant.line, mutant.column, &mutant.original);
        file.mutants.push(MutantResult {
            id: mutant.id.clone(),
            mutator_name: mutant.rule.clone(),
            replacement: (!mutant.replacement.is_empty()).then(|| mutant.replacement.clone()),
            location,
            status: status_of(mutant).to_owned(),
            status_reason: reason_of(mutant),
            killed_by: killed_by(mutant),
            duration: mutant.duration_ms,
        });
    }
    Projection {
        schema_version: SCHEMA_VERSION.to_owned(),
        thresholds: THRESHOLDS,
        project_root: root.to_string_lossy().into_owned(),
        files,
    }
}

/// What this run's outcome is called in the other vocabulary.
fn status_of(mutant: &super::run::RunMutantDocument) -> &'static str {
    match mutant.outcome.as_str() {
        "killed" => "Killed",
        "survived" => "Survived",
        "timed_out" => "Timeout",
        "inconclusive" | "errored" => "RuntimeError",
        "not_run" if mutant.unreached => "NoCoverage",
        _ => "Pending",
    }
}

/// What a reader is told about the status.
fn reason_of(mutant: &super::run::RunMutantDocument) -> Option<String> {
    match mutant.outcome.as_str() {
        "inconclusive" => Some(
            "one timeout that did not reproduce, so the run cannot say what the tests noticed"
                .to_owned(),
        ),
        "errored" => Some(format!(
            "the harness itself failed with exit {}",
            mutant.exit_code
        )),
        "not_run" if mutant.unreached => Some("no measured test reaches it".to_owned()),
        "not_run" => Some("this run did not execute it".to_owned()),
        _ => None,
    }
}

/// The targets that noticed it, which for this engine is the one that did.
fn killed_by(mutant: &super::run::RunMutantDocument) -> Vec<String> {
    if mutant.outcome == "killed" && !mutant.target.is_empty() {
        vec![mutant.target.clone()]
    } else {
        Vec::new()
    }
}

/// Where one mutation is, counted in UTF-16 as this schema requires.
fn located(source: &str, line: u32, column: u32, original: &str) -> Location {
    let start = Position {
        line,
        column: utf16_column(source, line, column),
    };
    let end = end_of(source, line, column, original);
    Location { start, end }
}

/// The 1-based UTF-16 column of a 1-based byte column on `line`.
fn utf16_column(source: &str, line: u32, byte_column: u32) -> u32 {
    let Some(text) = line_of(source, line) else {
        return 1;
    };
    let bytes = usize::try_from(byte_column.saturating_sub(1)).unwrap_or(0);
    let prefix = text.get(..bytes.min(text.len())).unwrap_or(text);
    let units: usize = prefix.chars().map(char::len_utf16).sum();
    u32::try_from(units.saturating_add(1)).unwrap_or(u32::MAX)
}

/// Where a mutation ends: one past its last byte, on whichever line that is.
fn end_of(source: &str, line: u32, column: u32, original: &str) -> Position {
    let lines = original.lines().count().max(1);
    let last = line.saturating_add(u32::try_from(lines.saturating_sub(1)).unwrap_or(0));
    if lines == 1 {
        return Position {
            line: last,
            column: utf16_column(source, line, column).saturating_add(
                u32::try_from(original.chars().map(char::len_utf16).sum::<usize>()).unwrap_or(0),
            ),
        };
    }
    let tail = original.rsplit('\n').next().unwrap_or("");
    let units: usize = tail.chars().map(char::len_utf16).sum();
    Position {
        line: last,
        column: u32::try_from(units.saturating_add(1)).unwrap_or(u32::MAX),
    }
}

/// The text of one 1-based line, without its ending.
fn line_of(source: &str, line: u32) -> Option<&str> {
    let index = usize::try_from(line.saturating_sub(1)).ok()?;
    source.lines().nth(index)
}
