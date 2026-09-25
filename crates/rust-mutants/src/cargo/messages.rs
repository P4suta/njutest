// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo --message-format=json`: artifacts, compiler messages, and the build's end.

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
    /// A build script ran, and said where it wrote and what it put in the environment.
    BuildScriptExecuted(BuildScript),
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

/// A `build-script-executed` message: what a build script left behind for the units that read it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct BuildScript {
    /// The package whose build script it was.
    pub package_id: String,
    /// The directory it was told to write into, which every unit of the package is told about too.
    #[serde(default)]
    pub out_dir: Option<PathBuf>,
    /// What it put in the environment with `cargo::rustc-env`, in the order it said them.
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// The configurations it set with `cargo::rustc-cfg`, in the order it said them.
    #[serde(default)]
    pub cfgs: Vec<String>,
    /// The libraries it asked to link with `cargo::rustc-link-lib`.
    #[serde(default)]
    pub linked_libs: Vec<String>,
    /// The directories it asked to search with `cargo::rustc-link-search`.
    #[serde(default)]
    pub linked_paths: Vec<String>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
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
    /// The files produced: `.rmeta` for a check, `.rlib` and binaries for a build.
    #[serde(default)]
    pub filenames: Vec<PathBuf>,
    /// The executable, for a binary or a test harness.
    #[serde(default)]
    pub executable: Option<PathBuf>,
    /// Whether the unit was already up to date.
    #[serde(default)]
    pub fresh: bool,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// The profile a unit was compiled under.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Profile {
    /// Whether the unit is a test unit (`--test`).
    #[serde(default)]
    pub test: bool,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
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
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Whether the path a diagnostic names is the file that was written.
#[must_use]
pub fn names_file(reported: &str, path: &str) -> bool {
    let reported = reported.replace('\\', "/");
    reported == path || reported.ends_with(&format!("/{path}"))
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
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
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

/// One span of a diagnostic.
/// Byte offsets are what attribution uses; columns are characters, for people.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DiagnosticSpan {
    /// The file, relative to the directory rustc ran in (the workspace root) unless absolute.
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
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// The `code` object of a diagnostic, reduced to its code.
fn diagnostic_code<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Code {
        code: String,
        /// rustc carries this alongside the stable code; attribution deliberately ignores it.
        explanation: Option<String>,
    }
    Ok(Option::<Code>::deserialize(deserializer)?.map(|code| {
        let Code { code, explanation } = code;
        drop(explanation);
        code
    }))
}

/// Parses every line of a `--message-format=json` stream.
/// Blank lines are skipped; a line that is not a message is an error naming the line.
///
/// # Errors
/// [`CargoErrorKind::MessageUnparsable`].
pub fn parse_messages(stdout: &[u8]) -> Result<Vec<Message>, CargoError> {
    let text = std::str::from_utf8(stdout).map_err(|source| {
        CargoError::new(
            CargoErrorKind::MessageUnparsable,
            "the cargo message stream is not UTF-8",
        )
        .with_source(source)
    })?;
    let mut messages = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_number = index.checked_add(1).ok_or_else(|| {
            CargoError::new(
                CargoErrorKind::MessageUnparsable,
                "the cargo message stream has too many lines to identify one",
            )
        })?;
        messages.push(parse_message(line).map_err(|source| {
            CargoError::new(
                CargoErrorKind::MessageUnparsable,
                format!(
                    "line {} of the message stream is not a message: {:?}",
                    line_number,
                    line.chars().take(80).collect::<String>()
                ),
            )
            .with_source(source)
        })?);
    }
    Ok(messages)
}

fn parse_message(line: &str) -> Result<Message, serde_json::Error> {
    let value: serde_json::Value = crate::strictjson::decode_str(line)?;
    let reason = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| serde::de::Error::custom("no reason"))?
        .to_owned();
    Ok(match reason.as_str() {
        "compiler-artifact" => Message::CompilerArtifact(Artifact::deserialize(value)?),
        "compiler-message" => Message::CompilerMessage(CompilerMessage::deserialize(value)?),
        "build-script-executed" => Message::BuildScriptExecuted(BuildScript::deserialize(value)?),
        "build-finished" => {
            let success = value
                .get("success")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| serde::de::Error::custom("build-finished has no boolean success"))?;
            Message::BuildFinished { success }
        }
        _ => Message::Other { reason },
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_new_cargo_field_is_captured_instead_of_disappearing() {
        let parsed = super::parse_message(
            r#"{"reason":"compiler-artifact","package_id":"demo","target":{"name":"demo","kind":["lib"],"src_path":"/demo/src/lib.rs"},"profile":{"test":false},"future_cargo_field":{"meaning":42}}"#,
        );
        match parsed {
            Ok(super::Message::CompilerArtifact(artifact)) => assert_eq!(
                artifact.external_fields.get("future_cargo_field"),
                Some(&serde_json::json!({ "meaning": 42 }))
            ),
            Ok(_) => panic!("the message had the wrong typed variant"),
            Err(error) => panic!("the message did not decode: {error}"),
        }
    }
}
