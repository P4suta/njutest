// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `rust-mutants diagnostics`: everything one run established, in one directory, for a bug report.
//!
//! What a person can attach to an issue is what a reader can re-decide the run
//! from: the report, the catalog, the measurement, the probe logs, the
//! recording, and the state of the machine it ran on. What is absent is named
//! rather than passed over, because a reader who does not know a file is
//! missing reads its absence as a run that had nothing to say.
//!
//! No environment variable's value is ever written. A bundle travels, and a
//! value that travels with it is a value its owner did not choose to publish.

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
/// Nothing here fails the command: a part that could not be read is a part a
/// reader is told is absent, which is what a bundle is for.
#[must_use]
pub fn gather(bundle: &Path, parts: &[(&str, Part<'_>)]) -> (Vec<String>, Vec<String>) {
    let mut held = Vec::new();
    let mut absent = Vec::new();
    for (name, part) in parts {
        let target = bundle.join(name);
        let done = match part {
            Part::File(from) => copy(from, &target),
            Part::Tree(from) => copy_tree(from, &target),
            Part::Text(text) => std::fs::write(&target, text).is_ok(),
        };
        if done {
            held.push((*name).to_owned());
        } else {
            absent.push((*name).to_owned());
        }
    }
    (held, absent)
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
pub fn environment_names(vars: &[(std::ffi::OsString, std::ffi::OsString)]) -> String {
    let mut names: Vec<String> = vars
        .iter()
        .map(|(name, _value)| name.to_string_lossy().into_owned())
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
fn copy(from: &Path, to: &PathBuf) -> bool {
    if let Some(parent) = to.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    std::fs::copy(from, to).is_ok()
}

/// Copies a directory, answering whether it was there and held anything.
fn copy_tree(from: &Path, to: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(from) else {
        return false;
    };
    if std::fs::create_dir_all(to).is_err() {
        return false;
    }
    let mut copied = false;
    for entry in entries.flatten() {
        let target = to.join(entry.file_name());
        let done = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            copy_tree(&entry.path(), &target)
        } else {
            copy(&entry.path(), &target)
        };
        copied = copied || done;
    }
    if !copied {
        let _removed = std::fs::remove_dir_all(to);
    }
    copied
}
