// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run as it happens, one JSON object per line, for a program rather than a person.

use serde::{Deserialize, Serialize};

use super::catalog::SelectionDocument;
use super::run::{Accounting, FindingDocument, ScoreDocument};
use crate::outcome::Outcome;
use crate::run::NotRunReason;

/// The document type every line of a run stream carries.
pub const SCHEMA: &str = "rust-mutants-run-stream-v1";

/// One line of a run stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
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
        #[serde(deserialize_with = "crate::strictjson::required_option")]
        score: Option<ScoreDocument>,
        /// Where the report was written, when one was.
        #[serde(deserialize_with = "crate::strictjson::required_option")]
        report: Option<String>,
    },
    /// The run failed, and this is what it said.
    Error {
        /// The stable code.
        code: String,
        /// What went wrong.
        message: String,
        /// What to do about it, when the code carries one.
        #[serde(deserialize_with = "crate::strictjson::required_option")]
        remedy: Option<String>,
    },
}

/// One judged mutant, as a stream says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub outcome: Outcome,
    /// The verified runtime notice when this execution reached its step limit.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub step_notice: Option<crate::execute::StepLimitNotice>,
    /// The target that ran, empty when none did.
    pub target: String,
    /// How long every execution of it took together.
    pub duration_ms: u64,
    /// How many tests ran, when the harness said.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub tests_run: Option<u32>,
    /// Every test that failed with the mutant active, which is what noticed it.
    pub failed_tests: Vec<String>,
    /// Whether a first timeout was retried serially before the outcome was believed.
    pub retried: bool,
    /// Whether the harness had already answered when the clock ended the process.
    pub lingered: bool,
    /// Why it was never executed, when it was not.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub not_run_reason: Option<NotRunReason>,
    /// The run that established this, when it was not this one.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub source_run_id: Option<String>,
}

impl MutantLine {
    /// What a stream says about one judged mutant of `session`.
    ///
    /// # Errors
    /// Returns an exact-projection error when the execution duration does not fit the stream schema's millisecond field.
    pub fn of(
        session: &crate::session::Session,
        judged: &crate::run::Judged,
    ) -> Result<Self, crate::workspace::SessionError> {
        let found = session.catalog().by_index(judged.index);
        let position = found.and_then(|mutant| session.position(mutant));
        Ok(Self {
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
            outcome: judged.outcome,
            step_notice: judged.step_notice.clone(),
            target: judged.target.clone(),
            duration_ms: u64::try_from(judged.duration.as_millis()).map_err(|_overflow| {
                crate::workspace::SessionError::DurationMillisOverflow {
                    duration: judged.duration,
                }
            })?,
            tests_run: judged.tests_run,
            failed_tests: judged.failed_tests.clone(),
            retried: judged.retried,
            lingered: judged.lingered,
            not_run_reason: judged.not_run_reason,
            source_run_id: judged.source_run_id.clone(),
        })
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
    /// The input has more lines than the stream schema can number exactly.
    #[error("zero-based input line {zero_based} does not fit a one-based u64 line number")]
    LineNumberOverflow {
        /// The exact zero-based position reported by the iterator.
        zero_based: usize,
    },
}

/// One line of a run stream, read back.
///
/// # Errors
/// [`StreamError::Unreadable`] for a line that is not one of this stream's.
pub fn read_line(at: u64, text: &str) -> Result<Line, StreamError> {
    let value = crate::strictjson::from_str(text)
        .map_err(|source| StreamError::Unreadable { line: at, source })?;
    Line::deserialize(value).map_err(|source| StreamError::Unreadable { line: at, source })
}

/// Every line of a stream, read back in order.
///
/// # Errors
/// The first line that is not one of this stream's, by its number, or an input too large for the stream's exact line-number field.
pub fn read(text: &str) -> Result<Vec<Line>, StreamError> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(at, line)| {
            let zero_based = match u64::try_from(at) {
                Ok(zero_based) => zero_based,
                Err(_overflow) => {
                    return Err(StreamError::LineNumberOverflow { zero_based: at });
                }
            };
            let line_number = zero_based
                .checked_add(1)
                .ok_or(StreamError::LineNumberOverflow { zero_based: at })?;
            read_line(line_number, line)
        })
        .collect()
}
