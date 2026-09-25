// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest diagnostics`: everything about one run, in one directory.

use std::io::Write;
use std::path::Path;

use crate::app::{reports, runs};
use crate::cli::{Diagnostics as Arguments, EXIT_ASSURED, EXIT_ERROR, Environment};

/// The document that says what the bundle holds.
pub const MANIFEST_NAME: &str = "bundle.json";

#[derive(Debug, thiserror::Error)]
enum CarryError {
    #[error(transparent)]
    Run(#[from] runs::RunError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Bundles one run.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, Some(&arguments.run)) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let bundle = root.join(".njutest/diagnostics").join(run.id().as_str());
    if let Err(error) = std::fs::create_dir_all(&bundle) {
        super::diagnose(
            stderr,
            &format!(
                "{}: making {}: {error}",
                crate::error::REPORT_NOT_KEPT.code,
                bundle.display()
            ),
        )?;
        return Ok(EXIT_ERROR);
    }

    let (held, absent) = match carry_run(root, &run, &bundle) {
        Ok(parts) => parts,
        Err(error) => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: assembling {}: {error}",
                    crate::error::REPORT_NOT_KEPT.code,
                    bundle.display()
                ),
            )?;
            return Ok(EXIT_ERROR);
        }
    };

    let manifest = serde_json::json!({
        "schema": "njutest-diagnostics-v1",
        "run_id": run.id(),
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
        )?;
        return Ok(EXIT_ERROR);
    }
    super::say(stdout, &bundle.display().to_string())?;
    for name in &absent {
        super::say(stdout, &format!("absent\t{name}"))?;
    }
    Ok(EXIT_ASSURED)
}

/// Carries every optional part, distinguishing an absent part from a part that could not be read.
fn carry_run(
    root: &Path,
    run: &runs::ResolvedRun,
    bundle: &Path,
) -> Result<(Vec<String>, Vec<String>), CarryError> {
    let mut held = Vec::new();
    let mut absent = Vec::new();
    for (file, name) in [
        (reports::StoredFile::Document, reports::DOCUMENT_NAME),
        (reports::StoredFile::Lines, crate::report::lines::FILE_NAME),
    ] {
        record(
            carry_stored(run, file, &bundle.join(name))?,
            name,
            &mut held,
            &mut absent,
        );
    }
    let recording = runs::recording(root, run.id());
    record(
        copy(
            &recording.join(crate::trace::FILE_NAME),
            &bundle.join(crate::trace::FILE_NAME),
        )?,
        crate::trace::FILE_NAME,
        &mut held,
        &mut absent,
    );
    let outputs = recording.join(crate::trace::OUTPUT_DIRECTORY_NAME);
    record(
        copy_tree(&outputs, &bundle.join(crate::trace::OUTPUT_DIRECTORY_NAME))?,
        crate::trace::OUTPUT_DIRECTORY_NAME,
        &mut held,
        &mut absent,
    );
    Ok((held, absent))
}

fn carry_stored(
    run: &runs::ResolvedRun,
    file: reports::StoredFile,
    destination: &Path,
) -> Result<bool, CarryError> {
    let Some(text) = run.read(file)? else {
        return Ok(false);
    };
    rust_mutants::replace::file(destination, text.as_bytes()).map_err(|error| error.source)?;
    Ok(true)
}

/// Notes whether one part of the bundle is there.
pub fn record(present: bool, name: &str, held: &mut Vec<String>, absent: &mut Vec<String>) {
    if present {
        held.push(name.to_owned());
    } else {
        absent.push(name.to_owned());
    }
}

/// Copies one file, answering whether it was there.
///
/// # Errors
/// The source existed but could not be inspected or copied.
pub fn copy(from: &Path, to: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(from) {
        Ok(metadata) if metadata.file_type().is_file() => {
            std::fs::copy(from, to)?;
            Ok(true)
        }
        Ok(_) => Err(std::io::Error::other(format!(
            "{} is not a regular file",
            from.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Copies a directory, answering whether it was there and held anything.
///
/// # Errors
/// The source existed but could not be inspected or copied, or a partial destination could not be removed after the copy failed.
pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<bool> {
    let entries = match std::fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let copied = match copy_tree_entries(entries, to) {
        Ok(copied) => copied,
        Err(copy_error) => {
            return match rust_mutants::tempowner::remove_tree(to) {
                Ok(()) => Err(copy_error),
                Err(cleanup_error) if cleanup_error.kind() == std::io::ErrorKind::NotFound => {
                    Err(copy_error)
                }
                Err(cleanup_error) => Err(std::io::Error::other(format!(
                    "copy failed ({copy_error}); removing the partial {} also failed ({cleanup_error})",
                    to.display()
                ))),
            };
        }
    };
    Ok(copied)
}

/// Carries a tree, failing the whole copy when any entry cannot be inspected or copied.
fn copy_tree_into(from: &Path, to: &Path) -> std::io::Result<bool> {
    let entries = std::fs::read_dir(from)?;
    copy_tree_entries(entries, to)
}

fn copy_tree_entries(entries: std::fs::ReadDir, to: &Path) -> std::io::Result<bool> {
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
