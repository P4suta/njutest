// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Dep-info: which files a unit really compiled.
//!
//! Syntax alone cannot say. `#[path]`, `include!`, and `cfg` decide which
//! files a crate is made of, and only the compiler knows the answer it
//! took. rustc writes it down beside every artifact as a Makefile rule, and
//! reading that rule is how discovery learns the file set of each unit —
//! and how a file only the test unit compiles becomes a `test-only-file`
//! skip rather than a mutant nothing can run.

use std::path::{Path, PathBuf};

use super::messages::{Artifact, Message};
use super::metadata::Target;
use super::{CargoError, CargoErrorKind};

/// One compiled unit and its sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The package id.
    pub package_id: String,
    /// The target.
    pub target: Target,
    /// Whether this is the test unit of the target.
    pub test: bool,
    /// Every source file the unit compiled, absolute, sorted, deduplicated.
    pub sources: Vec<PathBuf>,
}

/// The dep-info file rustc wrote beside `artifact`: the same stem without
/// the `lib` prefix and with the `.d` extension.
///
/// A test binary has no extension at all on Unix, and its dep-info is its
/// own name with `.d` appended; a library has both a `lib` prefix and an
/// extension, and neither belongs in the dep-info's name.
#[must_use]
pub fn dep_info_path(artifact: &Path) -> Option<PathBuf> {
    let name = artifact.file_name()?.to_str()?;
    let stem = match artifact.extension() {
        Some(_) => artifact.file_stem()?.to_str()?,
        None => name,
    };
    let stem = stem.strip_prefix("lib").unwrap_or(stem);
    Some(artifact.with_file_name(format!("{stem}.d")))
}

/// The prerequisites of the first rule of a dep-info file, with `\ `
/// escapes undone and line continuations joined.
///
/// # Errors
///
/// [`CargoErrorKind::DepInfoUnreadable`] when there is no rule.
pub fn parse_dep_info(text: &str) -> Result<Vec<String>, CargoError> {
    let joined = text.replace("\\\n", " ").replace("\\\r\n", " ");
    let rule = joined
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| CargoError::new(CargoErrorKind::DepInfoUnreadable, "dep-info is empty"))?;
    // The target may hold a drive colon on Windows; the rule's colon is the
    // first one followed by whitespace or the end.
    let colon = rule
        .char_indices()
        .find(|&(index, ch)| {
            ch == ':'
                && rule
                    .get(index.saturating_add(1)..)
                    .is_none_or(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
        })
        .map(|(index, _)| index)
        .ok_or_else(|| {
            CargoError::new(
                CargoErrorKind::DepInfoUnreadable,
                format!("dep-info has no rule: {rule:?}"),
            )
        })?;
    let prerequisites = rule.get(colon.saturating_add(1)..).unwrap_or_default();
    Ok(split_escaped(prerequisites))
}

/// Splits on unescaped whitespace, undoing `\ ` and `\\`.
fn split_escaped(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.peek() {
                Some(' ' | '\\') => {
                    if let Some(escaped) = chars.next() {
                        current.push(escaped);
                    }
                }
                _ => current.push('\\'),
            },
            ' ' | '\t' => {
                if !current.is_empty() {
                    items.push(std::mem::take(&mut current));
                }
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        items.push(current);
    }
    items
}

/// The units of a compilation, each with the sources its dep-info names,
/// resolved against `workspace_root` (the directory rustc ran in). Build
/// scripts are left out: they are never mutated.
///
/// # Errors
///
/// [`CargoErrorKind::DepInfoMissing`] when an artifact's dep-info cannot be
/// read, and [`CargoErrorKind::DepInfoUnreadable`] when it has no rule.
pub fn units_of(messages: &[Message], workspace_root: &Path) -> Result<Vec<Unit>, CargoError> {
    let mut units = Vec::new();
    for message in messages {
        let Message::CompilerArtifact(artifact) = message else {
            continue;
        };
        if artifact.target.is_custom_build() {
            continue;
        }
        units.push(unit_of(artifact, workspace_root)?);
    }
    Ok(units)
}

fn unit_of(artifact: &Artifact, workspace_root: &Path) -> Result<Unit, CargoError> {
    let file = artifact
        .filenames
        .first()
        .and_then(|first| dep_info_path(first))
        .ok_or_else(|| {
            CargoError::new(
                CargoErrorKind::DepInfoMissing,
                format!(
                    "artifact {} of {} names no file a dep-info could sit beside",
                    artifact.target.name, artifact.package_id
                ),
            )
        })?;
    let text = std::fs::read_to_string(&file).map_err(|source| {
        CargoError::new(
            CargoErrorKind::DepInfoMissing,
            format!("cannot read dep-info {}", file.display()),
        )
        .with_source(source)
    })?;
    let mut sources: Vec<PathBuf> = parse_dep_info(&text)?
        .into_iter()
        .map(|path| {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            }
        })
        .collect();
    sources.sort();
    sources.dedup();
    Ok(Unit {
        package_id: artifact.package_id.clone(),
        target: artifact.target.clone(),
        test: artifact.profile.test,
        sources,
    })
}
