// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Clippy for the independent fuzz workspace, derived from the root lint policy.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use thiserror::Error;

/// Why the fuzz workspace could not be checked under the root policy.
#[derive(Debug, Error)]
pub enum FuzzClippyError {
    /// The root manifest could not be read.
    #[error("{path}: {source}")]
    Read {
        /// The unreadable manifest.
        path: String,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// The root manifest was not TOML.
    #[error("{path}: {source}")]
    Parse {
        /// The malformed manifest.
        path: String,
        /// The TOML failure.
        source: toml::de::Error,
    },
    /// The root manifest did not carry the lint table this command derives.
    #[error("Cargo.toml has no [workspace.lints.{group}] table")]
    Missing {
        /// The absent lint group.
        group: &'static str,
    },
    /// One lint entry was neither a level nor Cargo's level/priority table.
    #[error("workspace lint {name:?} has no string level")]
    Level {
        /// The malformed lint name.
        name: String,
    },
    /// Cargo could not be started.
    #[error("could not start cargo clippy for fuzz/Cargo.toml: {0}")]
    Start(std::io::Error),
    /// Clippy refused at least one fuzz target.
    #[error("cargo clippy refused the fuzz workspace")]
    Refused,
}

impl crate::error::Coded for FuzzClippyError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Read { .. } | Self::Parse { .. } | Self::Missing { .. } | Self::Level { .. } => {
                crate::error::XtCode::FuzzPolicy
            }
            Self::Start(..) => crate::error::XtCode::FuzzCargo,
            Self::Refused => crate::error::XtCode::GateRefused,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Lint {
    priority: i64,
    name: String,
    level: String,
}

/// The rustc/Clippy flags represented by the root workspace lint policy.
///
/// This is deliberately derived rather than copied: changing `Cargo.toml` changes both workspaces' next Clippy invocation.
/// # Errors
///
/// Returns a typed error when the workspace lint table is absent or malformed.
pub fn flags(manifest: &str) -> Result<Vec<String>, FuzzClippyError> {
    let document = manifest
        .parse::<toml::Table>()
        .map_err(|source| FuzzClippyError::Parse {
            path: "Cargo.toml".to_owned(),
            source,
        })?;
    let workspace = document
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("lints"))
        .and_then(toml::Value::as_table);
    let mut lints = Vec::new();
    for (group, prefix) in [("rust", ""), ("clippy", "clippy::")] {
        let table = workspace
            .and_then(|workspace| workspace.get(group))
            .and_then(toml::Value::as_table)
            .ok_or(FuzzClippyError::Missing { group })?;
        for (name, setting) in table {
            let (level, priority) = match setting {
                toml::Value::String(level) => (level.clone(), 0),
                toml::Value::Table(setting) => {
                    let level = setting
                        .get("level")
                        .and_then(toml::Value::as_str)
                        .ok_or_else(|| FuzzClippyError::Level { name: name.clone() })?;
                    let priority = lint_priority(setting);
                    (level.to_owned(), priority)
                }
                _ => return Err(FuzzClippyError::Level { name: name.clone() }),
            };
            lints.push(Lint {
                priority,
                name: format!("{prefix}{}", name.replace('_', "-")),
                level,
            });
        }
    }
    lints.sort();
    let mut flags = Vec::new();
    for lint in lints {
        let level = match lint.level.as_str() {
            "allow" => "-A",
            "warn" => "-W",
            "deny" => "-D",
            "forbid" => "-F",
            _ => return Err(FuzzClippyError::Level { name: lint.name }),
        };
        flags.push(level.to_owned());
        flags.push(lint.name);
    }
    flags.push("-D".to_owned());
    flags.push("warnings".to_owned());
    Ok(flags)
}

fn lint_priority(setting: &toml::Table) -> i64 {
    let Some(priority) = setting.get("priority").and_then(toml::Value::as_integer) else {
        return 0;
    };
    priority
}

/// Runs `cargo` Clippy over every fuzz target under the root workspace lint policy.
/// The composition root supplies the exact cargo program.
///
/// # Errors
///
/// Returns a typed error when the root policy cannot be read or when the fuzz workspace does not satisfy it.
pub fn check(root: &Path, cargo: &OsStr) -> Result<String, FuzzClippyError> {
    let root_manifest = root.join("Cargo.toml");
    let manifest =
        std::fs::read_to_string(&root_manifest).map_err(|source| FuzzClippyError::Read {
            path: root_manifest.display().to_string(),
            source,
        })?;
    let flags = flags(&manifest)?;
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .args([
            "clippy",
            "--manifest-path",
            "fuzz/Cargo.toml",
            "--locked",
            "--all-targets",
            "--all-features",
            "--",
        ])
        .args(flags);
    let status = command.status().map_err(FuzzClippyError::Start)?;
    if !status.success() {
        return Err(FuzzClippyError::Refused);
    }
    Ok("fuzz-clippy: every target satisfies the root workspace lint policy".to_owned())
}
