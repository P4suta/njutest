// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Dep-info: which files a unit really compiled.

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

/// The dep-info file rustc wrote beside `artifact`: the same stem without the `lib` prefix and with the `.d` extension.
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

/// The prerequisites of the first rule of a dep-info file, with `\ ` escapes undone and line continuations joined.
///
/// # Errors
/// [`CargoErrorKind::DepInfoUnreadable`] when there is no rule.
pub fn parse_dep_info(text: &str) -> Result<Vec<String>, CargoError> {
    let joined = text.replace("\\\n", " ").replace("\\\r\n", " ");
    let rule = joined
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| CargoError::new(CargoErrorKind::DepInfoUnreadable, "dep-info is empty"))?;
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

/// Whether the file is one this engine reads as Rust.
fn is_rust(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rs")
}

/// The units of a compilation, each with the sources its dep-info names, resolved against `workspace_root` (the directory rustc ran in). Build scripts are left out: they are never mutated.
///
/// # Errors
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
        if is_uplift(artifact) {
            continue;
        }
        units.push(unit_of(artifact, workspace_root)?);
    }
    Ok(units)
}

/// Whether this artifact is cargo's uplifted copy of a unit rather than the unit itself.
fn is_uplift(artifact: &Artifact) -> bool {
    artifact.filenames.iter().all(|file| {
        file.parent()
            .and_then(Path::file_name)
            .is_none_or(|directory| directory != "deps")
    })
}

/// Every place this artifact's dep-info could sit: cargo puts it beside the hashed file in `deps/` and, for a binary it uplifts, beside the copy too.
fn dep_info_candidates(artifact: &Artifact) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = artifact
        .filenames
        .iter()
        .chain(artifact.executable.iter())
        .filter_map(|file| dep_info_path(file))
        .collect();
    candidates.dedup();
    candidates
}

fn unit_of(artifact: &Artifact, workspace_root: &Path) -> Result<Unit, CargoError> {
    let candidates = dep_info_candidates(artifact);
    let file = candidates
        .iter()
        .find(|path| path.is_file())
        .or_else(|| candidates.first())
        .ok_or_else(|| {
            CargoError::new(
                CargoErrorKind::DepInfoMissing,
                format!(
                    "artifact {} of {} names no file a dep-info could sit beside",
                    artifact.target.name, artifact.package_id
                ),
            )
        })?;
    let text = std::fs::read_to_string(file).map_err(|source| {
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
        .filter(|path| is_rust(path))
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
