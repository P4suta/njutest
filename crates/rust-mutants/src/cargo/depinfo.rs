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
    /// Every file the compiler read for the unit, Rust or not, absolute, sorted, deduplicated: what `include_str!` and `#[doc = include_str!]` embed as well as what was compiled.
    pub inputs: Vec<PathBuf>,
    /// Every environment variable the compiler read for the unit through `env!` or `option_env!`, with the value it read, or nothing where it was unset.
    pub env: std::collections::BTreeMap<String, Option<String>>,
}

/// The dep-info file rustc wrote beside `artifact`: the same stem without the `lib` prefix and with the `.d` extension.
#[must_use]
pub fn dep_info_path(artifact: &Path) -> Option<PathBuf> {
    let name = artifact.file_name()?.to_str()?;
    let stem = match artifact.extension() {
        Some(_) => artifact.file_stem()?.to_str()?,
        None => name,
    };
    let stem = match stem.strip_prefix("lib") {
        Some(stripped) => stripped,
        None => stem,
    };
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
            let Some(after_colon) = index.checked_add(1) else {
                return false;
            };
            ch == ':'
                && rule
                    .get(after_colon..)
                    .is_none_or(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
        })
        .map(|(index, _)| index)
        .ok_or_else(|| {
            CargoError::new(
                CargoErrorKind::DepInfoUnreadable,
                format!("dep-info has no rule: {rule:?}"),
            )
        })?;
    let after_colon = colon.checked_add(1).ok_or_else(|| {
        CargoError::new(
            CargoErrorKind::DepInfoUnreadable,
            "dep-info rule separator position overflowed",
        )
    })?;
    let prerequisites = rule.get(after_colon..).ok_or_else(|| {
        CargoError::new(
            CargoErrorKind::DepInfoUnreadable,
            "dep-info rule separator was not on a UTF-8 boundary",
        )
    })?;
    Ok(split_escaped(prerequisites))
}

/// The environment variables a dep-info file says the compiler read, each with the value it read or nothing where it was unset.
#[must_use]
pub fn env_deps(text: &str) -> std::collections::BTreeMap<String, Option<String>> {
    text.lines()
        .filter_map(|line| line.strip_prefix("# env-dep:"))
        .map(|dependency| match dependency.split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(unescaped(value))),
            None => (dependency.to_owned(), None),
        })
        .collect()
}

/// A value as rustc read it, before it escaped a backslash, a line feed, and a carriage return to keep it on one line.
fn unescaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
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
                    items.push(current);
                    current = String::new();
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

/// The units of a compilation, each with the sources its dep-info names, resolved against `workspace_root` (the directory rustc ran in).
/// Build scripts are left out: they are never mutated.
///
/// # Errors
/// [`CargoErrorKind::DepInfoMissing`] when an artifact's dep-info cannot be read, and [`CargoErrorKind::DepInfoUnreadable`] when it has no rule.
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

/// Every file the compiler read for a unit whose code runs while the build does rather than in a test: a procedural macro, and a build script, compiled for the build and not tested.
///
/// # Errors
/// What [`units_of`] refuses about one such unit's dep-info.
pub fn compile_time_inputs(
    messages: &[Message],
    workspace_root: &Path,
) -> Result<Vec<PathBuf>, CargoError> {
    let mut inputs = Vec::new();
    for message in messages {
        let Message::CompilerArtifact(artifact) = message else {
            continue;
        };
        let runs_in_the_build = artifact.target.is_custom_build()
            || (artifact.target.is_proc_macro() && !artifact.profile.test);
        if !runs_in_the_build || is_uplift(artifact) {
            continue;
        }
        inputs.extend(unit_of(artifact, workspace_root)?.inputs);
    }
    inputs.sort();
    inputs.dedup();
    Ok(inputs)
}

/// What one build script told the compilation of its package's units: configurations, environment, and what to link.
/// None of it is in a dep-info, and every part of it changes what compiles.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Emitted {
    /// The directory it wrote into, which names the run.
    pub out_dir: Option<PathBuf>,
    /// `cargo::rustc-cfg`, sorted.
    pub cfgs: Vec<String>,
    /// `cargo::rustc-env`, sorted.
    pub env: Vec<(String, String)>,
    /// `cargo::rustc-link-lib`, sorted.
    pub linked_libs: Vec<String>,
    /// `cargo::rustc-link-search`, sorted.
    pub linked_paths: Vec<String>,
}

impl Emitted {
    fn of(script: &super::messages::BuildScript) -> Self {
        let sorted = |mut values: Vec<String>| {
            values.sort();
            values
        };
        let mut env = script.env.clone();
        env.sort();
        Self {
            out_dir: script.out_dir.clone(),
            cfgs: sorted(script.cfgs.clone()),
            env,
            linked_libs: sorted(script.linked_libs.clone()),
            linked_paths: sorted(script.linked_paths.clone()),
        }
    }
}

/// What every build script of a compilation emitted, by the id of the package whose units it was emitted for, each package's sorted.
#[must_use]
pub fn emitted_of(messages: &[Message]) -> std::collections::BTreeMap<String, Vec<Emitted>> {
    let mut emitted: std::collections::BTreeMap<String, Vec<Emitted>> =
        std::collections::BTreeMap::new();
    for message in messages {
        if let Message::BuildScriptExecuted(script) = message {
            emitted
                .entry(script.package_id.clone())
                .or_default()
                .push(Emitted::of(script));
        }
    }
    for told in emitted.values_mut() {
        told.sort();
    }
    emitted
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
fn dep_info_candidates(artifact: &Artifact) -> Result<Vec<PathBuf>, CargoError> {
    let mut candidates = Vec::new();
    for file in artifact.filenames.iter().chain(artifact.executable.iter()) {
        let candidate = dep_info_path(file).ok_or_else(|| {
            CargoError::new(
                CargoErrorKind::DepInfoMissing,
                format!(
                    "artifact output {} has no file name for a dep-info",
                    file.display()
                ),
            )
        })?;
        candidates.push(candidate);
    }
    candidates.dedup();
    Ok(candidates)
}

fn unit_of(artifact: &Artifact, workspace_root: &Path) -> Result<Unit, CargoError> {
    let candidates = dep_info_candidates(artifact)?;
    let file = match regular_dep_info(&candidates)? {
        Some(file) => file,
        None => candidates.first().cloned().ok_or_else(|| {
            CargoError::new(
                CargoErrorKind::DepInfoMissing,
                format!(
                    "artifact {} of {} names no file a dep-info could sit beside",
                    artifact.target.name, artifact.package_id
                ),
            )
        })?,
    };
    let text = std::fs::read_to_string(&file).map_err(|source| {
        CargoError::new(
            CargoErrorKind::DepInfoMissing,
            format!("cannot read dep-info {}", file.display()),
        )
        .with_source(source)
    })?;
    let mut inputs: Vec<PathBuf> = parse_dep_info(&text)?
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
    inputs.sort();
    inputs.dedup();
    let sources = inputs
        .iter()
        .filter(|path| is_rust(path))
        .cloned()
        .collect();
    Ok(Unit {
        package_id: artifact.package_id.clone(),
        target: artifact.target.clone(),
        test: artifact.profile.test,
        sources,
        inputs,
        env: env_deps(&text),
    })
}

/// The first existing regular candidate.
/// Missing candidates are expected for Cargo's uplifted copies; an unreadable or irregular one is not silently skipped in favour of a different view of the same compilation.
fn regular_dep_info(candidates: &[PathBuf]) -> Result<Option<PathBuf>, CargoError> {
    for path in candidates {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_file() => return Ok(Some(path.clone())),
            Ok(_irregular_or_link) => {
                return Err(CargoError::new(
                    CargoErrorKind::DepInfoMissing,
                    format!("dep-info {} is not a regular file", path.display()),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(CargoError::new(
                    CargoErrorKind::DepInfoMissing,
                    format!("cannot inspect dep-info {}", path.display()),
                )
                .with_source(source));
            }
        }
    }
    Ok(None)
}
