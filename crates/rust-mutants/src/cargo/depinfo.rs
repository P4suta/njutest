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

/// Everything one compilation read: every file a unit's dep-info names, build scripts included and whatever the extension, every environment variable rustc recorded reading, and what every build script told the units it builds for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    /// Every file read, absolute, sorted, deduplicated.
    pub files: Vec<PathBuf>,
    /// Every environment variable read at compile time, sorted by name.
    pub env: Vec<EnvDep>,
    /// What each build script whose output the compilation used emitted, sorted.
    pub emitted: Vec<Emitted>,
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

/// One environment variable a compilation read through `env!` or `option_env!`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EnvDep {
    /// Its name.
    pub name: String,
    /// Its value as rustc recorded it, absent where it was unset.
    pub value: Option<String>,
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

/// One compiled unit, build scripts included, and everything its own dep-info says it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInputs {
    /// The package id.
    pub package_id: String,
    /// The target.
    pub target: Target,
    /// Whether this is the test unit of the target.
    pub test: bool,
    /// What its compilation read.
    pub inputs: Inputs,
}

/// Every unit of a compilation, build scripts included, with every file its dep-info names whatever the extension, every variable it recorded reading, and what its package's build script emitted for it.
///
/// # Errors
/// The dep-info errors of [`units_of`].
pub fn unit_inputs_of(
    messages: &[Message],
    workspace_root: &Path,
) -> Result<Vec<UnitInputs>, CargoError> {
    let mut emitted: std::collections::BTreeMap<&str, Vec<Emitted>> =
        std::collections::BTreeMap::new();
    for message in messages {
        if let Message::BuildScriptExecuted(script) = message {
            emitted
                .entry(script.package_id.as_str())
                .or_default()
                .push(Emitted::of(script));
        }
    }
    let mut units = Vec::new();
    for message in messages {
        let Message::CompilerArtifact(artifact) = message else {
            continue;
        };
        if !artifact.target.is_custom_build() && is_uplift(artifact) {
            continue;
        }
        let unit = unit_of(artifact, workspace_root)?;
        let mut told = if artifact.target.is_custom_build() {
            Vec::new()
        } else {
            emitted
                .get(artifact.package_id.as_str())
                .cloned()
                .unwrap_or_default()
        };
        told.sort();
        units.push(UnitInputs {
            package_id: unit.package_id,
            target: unit.target,
            test: unit.test,
            inputs: Inputs {
                files: unit.inputs,
                env: unit
                    .env
                    .into_iter()
                    .map(|(name, value)| EnvDep { name, value })
                    .collect(),
                emitted: told,
            },
        });
    }
    Ok(units)
}

/// Everything a compilation read, from every artifact's dep-info, build scripts included.
///
/// # Errors
/// The dep-info errors of [`units_of`].
pub fn inputs_of(messages: &[Message], workspace_root: &Path) -> Result<Inputs, CargoError> {
    let mut files = Vec::new();
    let mut env = Vec::new();
    let mut emitted = Vec::new();
    for unit in unit_inputs_of(messages, workspace_root)? {
        files.extend(unit.inputs.files);
        env.extend(unit.inputs.env);
        emitted.extend(unit.inputs.emitted);
    }
    files.sort();
    files.dedup();
    env.sort();
    env.dedup();
    emitted.sort();
    emitted.dedup();
    Ok(Inputs {
        files,
        env,
        emitted,
    })
}

/// `path` as a dep-info names it, resolved against the directory rustc ran in.
fn absolute(workspace_root: &Path, path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    }
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
        if !runs_in_the_build || (!artifact.target.is_custom_build() && is_uplift(artifact)) {
            continue;
        }
        inputs.extend(unit_of(artifact, workspace_root)?.inputs);
    }
    inputs.sort();
    inputs.dedup();
    Ok(inputs)
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
        if artifact.target.is_custom_build()
            && let Some(hashed) = build_script_dep_info(file, &artifact.target.name)
        {
            candidates.push(hashed);
        }
    }
    candidates.dedup();
    Ok(candidates)
}

/// Where rustc left a build script's dep-info: cargo names the program `build-script-build` in a directory ending in the unit's hash, and rustc wrote `build_script_build-<hash>.d` beside it.
fn build_script_dep_info(program: &Path, target: &str) -> Option<PathBuf> {
    let directory = program.parent()?;
    let hash = directory.file_name()?.to_str()?.rsplit_once('-')?.1;
    Some(directory.join(format!("{}-{hash}.d", target.replace('-', "_"))))
}

fn unit_of(artifact: &Artifact, workspace_root: &Path) -> Result<Unit, CargoError> {
    let text = dep_info_of(artifact)?;
    let mut inputs: Vec<PathBuf> = parse_dep_info(&text)?
        .into_iter()
        .map(|path| absolute(workspace_root, path))
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

/// The text of the dep-info rustc wrote for `artifact`.
fn dep_info_of(artifact: &Artifact) -> Result<String, CargoError> {
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
    std::fs::read_to_string(&file).map_err(|source| {
        CargoError::new(
            CargoErrorKind::DepInfoMissing,
            format!("cannot read dep-info {}", file.display()),
        )
        .with_source(source)
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
