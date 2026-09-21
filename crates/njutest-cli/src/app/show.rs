// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest report`: what a completed run concluded.

use std::io::Write;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Format, Report as Arguments};
use crate::report::lines;

#[derive(Debug, thiserror::Error)]
enum ProjectionError {
    #[error(transparent)]
    Store(#[from] crate::app::reports::StoreError),
    #[error("{}: {source}", crate::error::REPORT_UNSOUND.code)]
    Count {
        #[from]
        source: crate::report::CountError,
    },
}

/// One stored run, in the shape whoever asked for it wants.
fn projected(
    shape: Format,
    report: &crate::report::Report,
    stored: &runs::ResolvedRun,
    environment: &Environment,
) -> Result<String, ProjectionError> {
    let root = &environment.working_directory;
    let said = stored.said_document();
    if shape == Format::Lines {
        return Ok(lines::kept(report, said)?);
    }
    let sources = crate::presentation::Sources::read(root, report)?;
    let told = crate::presentation::Told::of(report, &sources, said)?;
    if shape == Format::Agent {
        return Ok(crate::presentation::agent::brief(&told));
    }
    Ok(crate::presentation::human::draw(
        &told,
        environment.terminal,
    ))
}

/// Prints a run's report.
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
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let text = match arguments.format {
        Format::Json => runs::document(&run).map_err(|error| error.to_string()),
        shape @ (Format::Lines | Format::Spec | Format::Human | Format::Agent) => {
            match runs::report(&run) {
                Ok(report) => {
                    projected(shape, &report, &run, environment).map_err(|error| error.to_string())
                }
                Err(error) => Err(error.to_string()),
            }
        }
    };
    match text {
        Ok(text) => {
            stdout.write_all(text.as_bytes())?;
            Ok(EXIT_ASSURED)
        }
        Err(message) => {
            super::diagnose(stderr, &message)?;
            Ok(EXIT_ERROR)
        }
    }
}
