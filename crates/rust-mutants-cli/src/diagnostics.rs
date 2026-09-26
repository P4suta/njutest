// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `rust-mutants diagnostics`: everything one run established, in one directory, for a bug report.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The document that says what the bundle holds.
pub const MANIFEST_NAME: &str = "bundle.json";

/// The directory the bundle is written into, beside the run.
pub const DIRECTORY_NAME: &str = "diagnostics";

/// The doctor document the bundle carries.
pub const DOCTOR_NAME: &str = "doctor-v1.json";

/// What the toolchain said about itself.
pub const TOOLCHAIN_NAME: &str = "toolchain.txt";

/// The names of the variables that were set, and none of their values.
pub const ENVIRONMENT_NAME: &str = "environment.txt";

/// Names the shape of the manifest.
pub const DOCUMENT_TYPE: &str = "rust-mutants/diagnostics";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 1;

/// What one bundle holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleDocument {
    /// [`DOCUMENT_TYPE`].
    pub document_type: String,
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The engine that gathered it.
    pub tool_version: String,
    /// The run it is about.
    pub run_id: String,
    /// The run directory it was gathered from.
    pub run_directory: String,
    /// What is in the bundle, in the order it was looked for.
    pub held: Vec<String>,
    /// What the run did not leave, named so a reader knows it is not there.
    pub absent: Vec<String>,
}

/// One thing a bundle holds, and where it came from.
#[derive(Debug, Clone)]
pub enum Part<'a> {
    /// A file copied verbatim.
    File(PathBuf),
    /// A directory copied whole.
    Tree(PathBuf),
    /// Text this command wrote itself.
    Text(&'a str),
}

/// Gathers `parts` into `bundle`, answering with what it holds and what was not there.
///
/// # Errors
/// Returns the first filesystem failure; a partial bundle is never reported as complete.
pub fn gather(
    bundle: &Path,
    parts: &[(&str, Part<'_>)],
) -> std::io::Result<(Vec<String>, Vec<String>)> {
    let mut held = Vec::new();
    let mut absent = Vec::new();
    for (name, part) in parts {
        let target = bundle.join(name);
        let done = match part {
            Part::File(from) => copy(from, &target)?,
            Part::Tree(from) => copy_tree(from, &target)?,
            Part::Text(text) => {
                std::fs::write(&target, text)?;
                true
            }
        };
        if done {
            held.push((*name).to_owned());
        } else {
            absent.push((*name).to_owned());
        }
    }
    Ok((held, absent))
}

/// The manifest of a gathered bundle.
#[must_use]
pub fn manifest(
    run_id: &str,
    run_directory: &Path,
    held: Vec<String>,
    absent: Vec<String>,
) -> BundleDocument {
    BundleDocument {
        document_type: DOCUMENT_TYPE.to_owned(),
        schema_version: SCHEMA_VERSION,
        tool_version: rust_mutants::VERSION.to_owned(),
        run_id: run_id.to_owned(),
        run_directory: run_directory.display().to_string(),
        held,
        absent,
    }
}

/// The names of every variable that is set, one per line, and no value of any of them.
#[must_use]
pub fn environment_names(vars: &rust_mutants::vars::Variables) -> String {
    let mut names: Vec<String> = vars
        .for_process()
        .map(|(name, _value)| match name.to_str() {
            Some(text) => text.to_owned(),
            None => rust_mutants::telling::LosslessBytes::new(name.as_encoded_bytes()).to_string(),
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut text =
        String::from("The names of the variables that were set. No value is recorded.\n");
    for name in names {
        text.push_str(&name);
        text.push('\n');
    }
    text
}

/// Copies one file, answering whether it was there.
fn copy(from: &Path, to: &Path) -> std::io::Result<bool> {
    if let Some(parent) = to.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return Err(error);
    }
    match std::fs::copy(from, to) {
        Ok(_bytes) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Copies a directory, answering whether it was there and held anything.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<bool> {
    let entries = match std::fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let entries = entries.collect::<std::io::Result<Vec<_>>>()?;
    std::fs::create_dir_all(to)?;
    let mut copied = false;
    for entry in entries {
        let target = to.join(entry.file_name());
        let done = match entry.file_type() {
            Ok(kind) if kind.is_dir() => copy_tree(&entry.path(), &target)?,
            Ok(kind) if kind.is_file() => copy(&entry.path(), &target)?,
            Ok(_) => false,
            Err(error) => return Err(error),
        };
        if !done {
            discard_tree(to)?;
            return Ok(false);
        }
        copied = true;
    }
    if !copied {
        discard_tree(to)?;
    }
    Ok(copied)
}

fn discard_tree(path: &Path) -> std::io::Result<()> {
    let reclaimed = rust_mutants::reclaim::all([path]);
    if reclaimed.left().is_empty() {
        return Ok(());
    }
    let reason = reclaimed
        .refused
        .into_iter()
        .map(|(path, why)| format!("{}: {why}", path.display()))
        .chain(
            reclaimed
                .unreached
                .into_iter()
                .map(|path| format!("{}: cleanup budget exhausted", path.display())),
        )
        .collect::<Vec<_>>()
        .join("; ");
    Err(std::io::Error::other(reason))
}
