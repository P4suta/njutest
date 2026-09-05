// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest diagnostics`: everything about one run, in one directory.

use std::io::Write;
use std::path::{Path, PathBuf};

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
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let bundle = root.join(".mjutest/diagnostics").join(&run);
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
        "schema": "mjutest-diagnostics-v1",
        "run_id": run,
        "held": held,
        "absent": absent,
    });
    let mut text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    text.push('\n');
    if let Err(error) = std::fs::write(bundle.join(MANIFEST_NAME), text) {
        super::diagnose(
            stderr,
            &format!(
                "{}: writing the bundle manifest: {error}",
                crate::error::REPORT_NOT_KEPT.code
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
fn record(present: bool, name: &str, held: &mut Vec<String>, absent: &mut Vec<String>) {
    if present {
        held.push(name.to_owned());
    } else {
        absent.push(name.to_owned());
    }
}

/// Copies one file, answering whether it was there.
fn copy(from: &Path, to: &PathBuf) -> bool {
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
    copied
}
