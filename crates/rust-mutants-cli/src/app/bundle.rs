// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything a run knew, in one directory somebody can send.
//!
//! A person reporting what the engine did to their workspace has to be able to
//! hand over what it saw. What travels is the run's own documents and the names
//! of the environment variables that were set — never a value of one, because a
//! bundle travels and a value that travels with it is a value its owner did not
//! choose to publish.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use rust_mutants::runner::Cancel;

use super::doctor::{Asked, doctor_document};
use super::stored::report_of;
use super::{json_line, locating, trace, write};
use crate::Environment;
use crate::error::CliError;
use crate::report::run as run_report;

/// Which run to gather, and where to put it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Gathering<'a> {
    /// The workspace root. Defaults to the working directory.
    pub(super) root: Option<&'a Path>,
    /// The run, by its identity. The newest when none is named.
    pub(super) run: Option<&'a str>,
    /// Where the bundle goes, when it does not go beside the run.
    pub(super) output: Option<&'a Path>,
}

/// Gathers everything one run established into one directory.
///
/// # Errors
/// [`CliError::ReportMissing`] when no stored run answers to what was asked
/// for, and [`CliError::WriteFailed`] when the bundle cannot be written.
pub(super) fn bundle(
    asked: &Gathering<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    let root = environment.rooted(asked.root);
    let reports = root.join(crate::config::DEFAULT_REPORTS_DIRECTORY);
    let directory = report_of(&reports, asked.run)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CliError::ReportMissing {
            message: format!("no run report is stored under {}", reports.display()),
        })?;
    let run_id = directory
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let bundle = asked.output.map_or_else(
        || directory.join(crate::diagnostics::DIRECTORY_NAME),
        Path::to_path_buf,
    );
    std::fs::create_dir_all(&bundle).map_err(|source| CliError::writing(&bundle, source))?;

    let doctor = json_line(&doctor_document(
        &Asked {
            root: Some(&root),
            packages: &[],
            json: true,
        },
        environment,
        cancel,
    ));
    let toolchain = toolchain_text(&root, environment, cancel);
    let names = crate::diagnostics::environment_names(&environment.vars);
    let parts = gathered(
        &directory,
        &root,
        Wrote {
            doctor: &doctor,
            toolchain: &toolchain,
            names: &names,
        },
    );
    let (held, absent) = crate::diagnostics::gather(&bundle, &parts);
    let document = crate::diagnostics::manifest(&run_id, &directory, held, absent.clone());
    let manifest = bundle.join(crate::diagnostics::MANIFEST_NAME);
    std::fs::write(&manifest, json_line(&document))
        .map_err(|source| CliError::writing(&manifest, source))?;

    let mut text = format!("{}\n", bundle.display());
    for name in &absent {
        let written = writeln!(text, "absent\t{name}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    write(stdout, &text);
    Ok(0)
}

/// What a bundle carries that no run left: this command wrote it for the reader.
#[derive(Debug, Clone, Copy)]
struct Wrote<'a> {
    /// The doctor document, as it stands now.
    doctor: &'a str,
    /// What the toolchain says about itself.
    toolchain: &'a str,
    /// The names of the variables that were set, and none of their values.
    names: &'a str,
}

/// What one bundle is gathered from: what the run left, and what this command wrote for it.
fn gathered<'a>(
    directory: &Path,
    root: &Path,
    wrote: Wrote<'a>,
) -> Vec<(&'static str, crate::diagnostics::Part<'a>)> {
    let Wrote {
        doctor,
        toolchain,
        names,
    } = wrote;
    vec![
        (
            run_report::FILE_NAME,
            crate::diagnostics::Part::File(directory.join(run_report::FILE_NAME)),
        ),
        (
            rust_mutants::report::evidence::CATALOG,
            crate::diagnostics::Part::File(directory.join(rust_mutants::report::evidence::CATALOG)),
        ),
        (
            rust_mutants::report::evidence::REACHED,
            crate::diagnostics::Part::File(directory.join(rust_mutants::report::evidence::REACHED)),
        ),
        (
            trace::RUN_DIRECTORY_NAME,
            crate::diagnostics::Part::Tree(directory.join(trace::RUN_DIRECTORY_NAME)),
        ),
        (
            crate::config::FILE_NAME,
            crate::diagnostics::Part::File(root.join(crate::config::FILE_NAME)),
        ),
        (
            crate::diagnostics::DOCTOR_NAME,
            crate::diagnostics::Part::Text(doctor),
        ),
        (
            crate::diagnostics::TOOLCHAIN_NAME,
            crate::diagnostics::Part::Text(toolchain),
        ),
        (
            crate::diagnostics::ENVIRONMENT_NAME,
            crate::diagnostics::Part::Text(names),
        ),
    ]
}

/// What the toolchain says about itself, for a reader who has a different one.
pub(super) fn toolchain_text(root: &Path, environment: &Environment, cancel: &Cancel) -> String {
    let located = rust_mutants::cargo::Toolchain::locate(&locating(environment), root, cancel);
    match located {
        Ok(found) => format!(
            "cargo: {}\nrustc: {}\nhost: {}\nrust-mutants: {}\n",
            found.cargo_version().summary,
            found.rustc_version().summary,
            found.host(),
            rust_mutants::VERSION
        ),
        Err(error) => format!("{error}\nrust-mutants: {}\n", rust_mutants::VERSION),
    }
}
