// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo --message-format=json`: artifacts, compiler messages, and the
//! build's end.

use std::path::PathBuf;

use serde::Deserialize;

use super::metadata::Target;
use super::{CargoError, CargoErrorKind};

/// One line of `--message-format=json`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Message {
    /// A unit was compiled (or was fresh).
    CompilerArtifact(Artifact),
    /// rustc said something about a unit.
    CompilerMessage(CompilerMessage),
    /// A build script ran.
    BuildScriptExecuted,
    /// The build ended.
    BuildFinished {
        /// Whether every unit succeeded.
        success: bool,
    },
    /// A reason this engine does not know.
    Other {
        /// The reason.
        reason: String,
    },
}

/// A `compiler-artifact` message.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    /// The package id.
    pub package_id: String,
    /// The target.
    pub target: Target,
    /// The profile.
    pub profile: Profile,
    /// The files produced: `.rmeta` for a check, `.rlib` and binaries for
    /// a build.
    #[serde(default)]
    pub filenames: Vec<PathBuf>,
    /// The executable, for a binary or a test harness.
    #[serde(default)]
    pub executable: Option<PathBuf>,
    /// Whether the unit was already up to date.
    #[serde(default)]
    pub fresh: bool,
}

/// The profile a unit was compiled under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Profile {
    /// Whether the unit is a test unit (`--test`).
    #[serde(default)]
    pub test: bool,
}

/// A `compiler-message` message.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CompilerMessage {
    /// The package id.
    pub package_id: String,
    /// The target.
    pub target: Target,
    /// What rustc said.
    pub message: Diagnostic,
}

/// One rustc diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Diagnostic {
    /// The message.
    pub message: String,
    /// The error code, e.g. `E0369`.
    #[serde(default, deserialize_with = "diagnostic_code")]
    pub code: Option<String>,
    /// `error`, `warning`, `note`, `help`, or `failure-note`.
    pub level: String,
    /// The spans, primary and secondary.
    #[serde(default)]
    pub spans: Vec<DiagnosticSpan>,
    /// The attached notes and helps.
    #[serde(default)]
    pub children: Vec<Self>,
    /// The human rendering, on top-level diagnostics.
    #[serde(default)]
    pub rendered: Option<String>,
}

impl Diagnostic {
    /// Whether the level is `error`.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.level == "error"
    }

    /// The primary span, if any.
    #[must_use]
    pub fn primary_span(&self) -> Option<&DiagnosticSpan> {
        self.spans.iter().find(|span| span.is_primary)
    }
}

/// One span of a diagnostic. Byte offsets are what attribution uses;
/// columns are characters, for people.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DiagnosticSpan {
    /// The file, relative to the directory rustc ran in (the workspace
    /// root) unless absolute.
    pub file_name: String,
    /// The first byte.
    pub byte_start: u32,
    /// One past the last byte.
    pub byte_end: u32,
    /// The 1-based first line.
    pub line_start: u32,
    /// The 1-based last line.
    pub line_end: u32,
    /// The 1-based first character column.
    pub column_start: u32,
    /// The 1-based column one past the last character.
    pub column_end: u32,
    /// Whether this is the span the diagnostic is about.
    pub is_primary: bool,
    /// The label, if any.
    #[serde(default)]
    pub label: Option<String>,
}

/// The `code` object of a diagnostic, reduced to its code.
fn diagnostic_code<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    struct Code {
        code: String,
    }
    Ok(Option::<Code>::deserialize(deserializer)?.map(|code| code.code))
}

/// Parses every line of a `--message-format=json` stream. Blank lines are
/// skipped; a line that is not a message is an error naming the line.
///
/// # Errors
///
/// [`CargoErrorKind::MessageUnparsable`].
pub fn parse_messages(stdout: &[u8]) -> Result<Vec<Message>, CargoError> {
    let text = String::from_utf8_lossy(stdout);
    let mut messages = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        messages.push(parse_message(line).map_err(|source| {
            CargoError::new(
                CargoErrorKind::MessageUnparsable,
                format!(
                    "line {} of the message stream is not a message: {:?}",
                    index.saturating_add(1),
                    line.chars().take(80).collect::<String>()
                ),
            )
            .with_source(source)
        })?);
    }
    Ok(messages)
}

fn parse_message(line: &str) -> Result<Message, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_str(line)?;
    let reason = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| serde::de::Error::custom("no reason"))?
        .to_owned();
    Ok(match reason.as_str() {
        "compiler-artifact" => Message::CompilerArtifact(Artifact::deserialize(value)?),
        "compiler-message" => Message::CompilerMessage(CompilerMessage::deserialize(value)?),
        "build-script-executed" => Message::BuildScriptExecuted,
        "build-finished" => Message::BuildFinished {
            success: value
                .get("success")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        },
        _ => Message::Other { reason },
    })
}
