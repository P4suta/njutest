// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest diagnostics`: everything about one run, in one directory.

use std::io::Write;
use std::path::Path;

use crate::app::{reports, runs};
use crate::cli::{Diagnostics as Arguments, EXIT_ASSURED, EXIT_ERROR, Environment};

/// The document that says what the bundle holds.
pub const MANIFEST_NAME: &str = "bundle.json";

/// Bundles one run.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, Some(&arguments.run)) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };
    let bundle = root.join(".njutest/diagnostics").join(&run);
    if let Err(error) = std::fs::create_dir_all(&bundle) {
        super::diagnose(
            stderr,
            &format!(
                "{}: making {}: {error}",
                crate::error::REPORT_NOT_KEPT.code,
                bundle.display()
            ),
        );
        return EXIT_ERROR;
    }

    let mut held = Vec::new();
    let mut absent = Vec::new();
    let directory = runs::directory(root, &run);
    for name in [reports::DOCUMENT_NAME, crate::report::lines::FILE_NAME] {
        record(
            copy(&directory.join(name), &bundle.join(name)),
            name,
            &mut held,
            &mut absent,
        );
    }
    let recording = runs::recording(root, &run);
    record(
        copy(
            &recording.join(crate::trace::FILE_NAME),
            &bundle.join(crate::trace::FILE_NAME),
        ),
        crate::trace::FILE_NAME,
        &mut held,
        &mut absent,
    );
    let outputs = recording.join(crate::trace::OUTPUT_DIRECTORY_NAME);
    record(
        copy_tree(&outputs, &bundle.join(crate::trace::OUTPUT_DIRECTORY_NAME)),
        crate::trace::OUTPUT_DIRECTORY_NAME,
        &mut held,
        &mut absent,
    );

    let manifest = serde_json::json!({
        "schema": "njutest-diagnostics-v1",
        "run_id": run,
        "held": held,
        "absent": absent,
    });
    let mut text = manifest.to_string();
    text.push('\n');
    if let Err(error) = rust_mutants::replace::file(&bundle.join(MANIFEST_NAME), text.as_bytes()) {
        super::diagnose(
            stderr,
            &format!(
                "{}: writing the bundle manifest through {}: {}",
                crate::error::REPORT_NOT_KEPT.code,
                error.path.display(),
                error.source,
            ),
        );
        return EXIT_ERROR;
    }
    super::say(stdout, &bundle.display().to_string());
    for name in &absent {
        super::say(stdout, &format!("absent\t{name}"));
    }
    EXIT_ASSURED
}

/// Notes whether one part of the bundle is there.
///
/// A bundle is what somebody sends when a run went wrong, so what is missing
/// from it is as much of the answer as what is in it: a part nobody listed
/// either way is one the reader assumes was never asked for.
pub fn record(present: bool, name: &str, held: &mut Vec<String>, absent: &mut Vec<String>) {
    if present {
        held.push(name.to_owned());
    } else {
        absent.push(name.to_owned());
    }
}

/// Copies one file, answering whether it was there.
#[must_use]
pub fn copy(from: &Path, to: &Path) -> bool {
    std::fs::copy(from, to).is_ok()
}

/// Copies a directory, answering whether it was there and held anything.
///
/// A directory that is there and empty held nothing, and saying it was there
/// would put a name in the bundle with nothing behind it. Nothing is left
/// behind either: a directory named in the bundle and named absent in the
/// manifest is two answers to one question, and the one a person opening the
/// bundle reads first is the directory.
#[must_use]
pub fn copy_tree(from: &Path, to: &Path) -> bool {
    let copied = copy_tree_into(from, to).unwrap_or(false);
    if !copied {
        drop(std::fs::remove_dir_all(to));
    }
    copied
}

/// Carries a tree, failing the whole copy when any entry cannot be inspected or copied.
fn copy_tree_into(from: &Path, to: &Path) -> std::io::Result<bool> {
    let entries = std::fs::read_dir(from)?;
    std::fs::create_dir_all(to)?;
    let mut copied = false;
    for entry in entries {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let done = if entry.file_type()?.is_dir() {
            copy_tree_into(&entry.path(), &target)?
        } else {
            std::fs::copy(entry.path(), target)?;
            true
        };
        copied |= done;
    }
    if !copied {
        std::fs::remove_dir(to)?;
    }
    Ok(copied)
}
