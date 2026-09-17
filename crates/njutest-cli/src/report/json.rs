// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The canonical projection: the whole model, as one JSON document.

use super::{Report, audit};
use crate::error::{self, ErrorCode};

/// Why a report could not be written or read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReportError {
    /// The model could not be serialized, which is an invariant failure.
    #[error("{}: the report could not be written as JSON: {source}", error::REPORT_UNSERIALIZABLE.code)]
    Unserializable {
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// The document is not one this version understands: an unknown field, a missing field, a value of the wrong shape.
    #[error("{}: not an {schema} document: {source}", error::REPORT_UNREADABLE.code, schema = super::SCHEMA)]
    Unreadable {
        /// What serde said, which names the offending field.
        #[source]
        source: serde_json::Error,
    },
    /// The report contradicts itself, so nothing was written.
    #[error(
        "{}: the report contradicts itself and was not written: {}",
        error::REPORT_UNSOUND.code,
        .violations.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    )]
    Unsound {
        /// Every invariant the report broke.
        violations: Vec<audit::Violation>,
    },
}

impl ReportError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unserializable { .. } => error::REPORT_UNSERIALIZABLE,
            Self::Unreadable { .. } => error::REPORT_UNREADABLE,
            Self::Unsound { .. } => error::REPORT_UNSOUND,
        }
    }
}

/// The durable document for `report`, ending in one newline.
///
/// # Errors
/// [`ReportError::Unsound`] when the report fails its own audit, and
/// [`ReportError::Unserializable`] when the model itself cannot be written.
pub fn document(report: &Report) -> Result<String, ReportError> {
    let violations = audit::validate_for_persistence(report);
    if !violations.is_empty() {
        return Err(ReportError::Unsound { violations });
    }
    render(report)
}

/// The same document, without the audit, for a caller that has one reason to look at a report it already knows is broken — a diagnostics bundle, a test of the audit itself.
///
/// # Errors
/// [`ReportError::Unserializable`] when the model cannot be written.
pub fn render(report: &Report) -> Result<String, ReportError> {
    let mut text = serde_json::to_string_pretty(report)
        .map_err(|source| ReportError::Unserializable { source })?;
    text.push('\n');
    Ok(text)
}

/// The same document with its whitespace taken out, for a stream where one line is one report.
///
/// # Errors
/// [`ReportError::Unserializable`], which is an invariant failure rather than
/// anything about the run.
pub fn line(report: &Report) -> Result<String, ReportError> {
    serde_json::to_string(report).map_err(|source| ReportError::Unserializable { source })
}

/// Reads a document this version understands, and refuses anything else.
///
/// # Errors
/// [`ReportError::Unreadable`] for a document with an unknown field, a
/// missing field, or a value of the wrong shape.
pub fn parse(text: &str) -> Result<Report, ReportError> {
    serde_json::from_str(text).map_err(|source| ReportError::Unreadable { source })
}
