// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest spec`: what a run established each item of the source pins, and what it leaves free.

use std::io::Write;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Spec as Arguments};
use crate::spec::{Subject, specified};

/// Prints what a run established about every change it made to the items the subject names.
///
/// It describes and does not judge, so it answers `EXIT_ASSURED` whatever the specification says and `EXIT_ERROR` only when it could not be read.
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
    let specification = match specified(&report, &Subject::parse(arguments.subject.as_deref())) {
        Ok(specification) => specification,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    stdout.write_all(
        crate::presentation::spec::page(&specification, environment.terminal).as_bytes(),
    )?;
    Ok(EXIT_ASSURED)
}
