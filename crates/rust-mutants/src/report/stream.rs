// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as it happens, one JSON object per line, for a program rather than a person.

use serde::{Deserialize, Serialize};

use super::catalog::SelectionDocument;
use super::run::{Accounting, FindingDocument, ScoreDocument};

/// The document type every line of a run stream carries.
pub const SCHEMA: &str = "rust-mutants-run-stream-v1";

/// One line of a run stream.
///
/// A stream is written as a run happens and read a line at a time, so a
/// consumer sees a mutant the moment it is judged rather than a report when
/// everything is over. The lines are additive: a reader from this release
/// ignores a kind it does not know, which is what lets a later release say
/// more without breaking one that already works.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Line {
    /// The run has started, and this is what it is about.
    RunStart {
        /// The stream's own schema, so a reader can tell what it is holding.
        schema: String,
        /// The engine that wrote it.
        tool_version: String,
        /// The run's identity, which is also its report directory's name.
        run_id: String,
        /// The workspace root's directory name.
        root_name: String,
        /// What the run was asked to measure.
        selection: SelectionDocument,
    },
    /// A phase has begun.
    PhaseStart {
        /// Its name.
        phase: String,
    },
    /// A phase has ended.
    PhaseEnd {
        /// Its name.
        phase: String,
        /// How long it took.
        duration_ms: u64,
    },
    /// One mutant has been judged.
    Mutant {
        /// How many have been delivered, this one included.
        completed: u32,
        /// How many there are.
        total: u32,
        /// What was judged, and what the tests made of it.
        mutant: MutantLine,
    },
    /// One thing that stops the run from being clean.
    Finding {
        /// The finding, exactly as the report will hold it.
        finding: FindingDocument,
    },
    /// The run is over.
    RunEnd {
        /// The code the process will exit with.
        exit_code: u8,
        /// Whether it stopped because it was asked to.
        interrupted: bool,
        /// The columns, as the report will hold them.
        accounting: Accounting,
        /// The score, when the run decided anything.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        score: Option<ScoreDocument>,
        /// Where the report was written, when one was.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        report: Option<String>,
    },
    /// The run failed, and this is what it said.
    Error {
        /// The stable code.
        code: String,
        /// What went wrong.
        message: String,
        /// What to do about it, when the code carries one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remedy: Option<String>,
    },
}

/// One judged mutant, as a stream says it.
///
/// It is what a consumer needs to act the moment a mutant is judged: what was
/// mutated, where, and what the tests made of it. The report holds more —
/// the bytes of the edit, the route, the catalog's own columns — because a
/// report is read afterwards and a stream is read as it arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutantLine {
    /// The dense catalog index the guards name.
    pub index: u32,
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// The rule that produced it.
    pub rule: String,
    /// The family the rule belongs to.
    pub family: String,
    /// The workspace-relative path.
    pub path: String,
    /// The 1-based line of the edit.
    pub line: u32,
    /// The 1-based byte column of the edit.
    pub column: u32,
    /// What the execution says.
    pub outcome: String,
    /// The target that ran, empty when none did.
    pub target: String,
    /// How long every execution of it took together.
    pub duration_ms: u64,
    /// How many tests ran, when the harness said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_run: Option<u32>,
    /// Every test that failed with the mutant active, which is what noticed it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_tests: Vec<String>,
    /// Whether a first timeout was retried serially before the outcome was believed.
    pub retried: bool,
    /// Why it was never executed, when it was not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_run_reason: Option<String>,
    /// The run that established this, when it was not this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
}

impl MutantLine {
    /// What a stream says about one judged mutant of `session`.
    #[must_use]
    pub fn of(session: &crate::session::Session, judged: &crate::run::Judged) -> Self {
        let found = session.catalog().by_index(judged.index);
        let position = found.and_then(|mutant| session.position(mutant));
        Self {
            index: judged.index,
            id: judged.id.clone(),
            display_id: judged.display_id.clone(),
            rule: found.map_or_else(String::new, |mutant| mutant.candidate.rule.name.to_owned()),
            family: found.map_or_else(String::new, |mutant| {
                mutant.candidate.rule.family.name().to_owned()
            }),
            path: found.map_or_else(String::new, |mutant| mutant.candidate.path.clone()),
            line: position.map_or(0, |at| at.line),
            column: position.map_or(0, |at| at.byte_column),
            outcome: judged.outcome.name().to_owned(),
            target: judged.target.clone(),
            duration_ms: u64::try_from(judged.duration.as_millis()).unwrap_or(u64::MAX),
            tests_run: judged.tests_run,
            failed_tests: judged.failed_tests.clone(),
            retried: judged.retried,
            not_run_reason: judged.not_run_reason.map(|reason| reason.name().to_owned()),
            source_run_id: judged.source_run_id.clone(),
        }
    }
}

/// Why a line of a stream could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StreamError {
    /// The line is not JSON, or is not a line of this stream.
    #[error("line {line} is not a line of a run stream: {source}")]
    Unreadable {
        /// The 1-based line number.
        line: u64,
        /// What the reader said.
        #[source]
        source: serde_json::Error,
    },
}

/// One line of a run stream, read back.
///
/// # Errors
/// [`StreamError::Unreadable`] for a line that is not one of this stream's.
pub fn read_line(at: u64, text: &str) -> Result<Line, StreamError> {
    serde_json::from_str(text).map_err(|source| StreamError::Unreadable { line: at, source })
}

/// Every line of a stream, read back in order.
///
/// # Errors
/// The first line that is not one of this stream's, by its number.
pub fn read(text: &str) -> Result<Vec<Line>, StreamError> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(at, line)| {
            read_line(
                u64::try_from(at).unwrap_or(u64::MAX).saturating_add(1),
                line,
            )
        })
        .collect()
}
