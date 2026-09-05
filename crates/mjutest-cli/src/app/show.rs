// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest report`: what a completed run concluded.
//!
//! The JSON form is the bytes the run wrote, read from the file and passed
//! through unchanged. Re-serializing the model would be almost the same
//! document, and "almost" is exactly what a reader comparing a piped report
//! with the stored one cannot check.

use std::io::Write;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Format, Report as Arguments};
use crate::report::lines;

/// Prints a run's report.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let text = match arguments.format {
        Format::Json => runs::document(root, &run).map_err(|error| error.to_string()),
        Format::Lines => runs::report(root, &run)
            .map(|report| lines::stream(&report))
            .map_err(|error| error.to_string()),
    };
    match text {
        Ok(text) => {
            let _written = stdout.write_all(text.as_bytes());
            EXIT_ASSURED
        }
        Err(message) => {
            super::diagnose(stderr, &message);
            EXIT_ERROR
        }
    }
}
