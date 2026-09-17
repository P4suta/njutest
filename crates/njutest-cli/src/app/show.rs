// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest report`: what a completed run concluded.

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
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };
    let text = match arguments.format {
        Format::Json => runs::document(root, &run).map_err(|error| error.to_string()),
        Format::Lines => runs::report(root, &run)
            .map(|report| {
                lines::kept(
                    &report,
                    &crate::app::reports::Store::read(root)
                        .run(&run)
                        .join(crate::app::reports::DOCUMENT_NAME),
                )
            })
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
