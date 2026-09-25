// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest guard`: one file as a run measured it, each changed line marked with where it stands.

use std::io::Write;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Guard as Arguments};
use crate::presentation::Sources;
use crate::spec::{SpecError, guarded};

/// Draws the file the arguments name as the run measured it, or says why the run cannot vouch for it.
///
/// It describes and does not judge, so it answers `EXIT_ASSURED` whatever the file holds and `EXIT_ERROR` only when the run or the file could not be read.
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
    let report = match runs::report(&run) {
        Ok(report) => report,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let (specification, path) = match guarded(&report, &arguments.path) {
        Ok(guarded) => guarded,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let sources = match Sources::read(root, &report) {
        Ok(sources) => sources,
        Err(source) => {
            let error = SpecError::Unsound { source };
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let page = match sources.span(&path, 1, u32::MAX) {
        Ok(measured) => {
            crate::presentation::guard::page(&specification, &path, &measured, environment.terminal)
        }
        Err(missing) => crate::presentation::guard::unasked(
            &specification,
            &path,
            missing,
            environment.terminal,
        ),
    };
    stdout.write_all(page.as_bytes())?;
    Ok(EXIT_ASSURED)
}
