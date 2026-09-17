// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest init`: write the annotated configuration skeleton.

use std::io::Write;

use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Init};
use crate::config;
use crate::error;

/// Writes `.njutest.toml` beside the working directory.
pub fn run(
    arguments: Init,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let path = environment.working_directory.join(config::FILE_NAME);
    if path.exists() && !arguments.force {
        super::diagnose(
            stderr,
            &format!(
                "{}: {} is already there; --force replaces it",
                error::CONFIG_EXISTS.code,
                config::FILE_NAME
            ),
        );
        return EXIT_ERROR;
    }
    match std::fs::write(&path, config::skeleton()) {
        Ok(()) => {
            super::say(stdout, &format!("wrote {}", config::FILE_NAME));
            super::say(
                stdout,
                "every key in it is commented out, because every one has a default: the \
                 file is a place to disagree rather than a thing a run needs",
            );
            super::say(
                stdout,
                "next: `njutest doctor` says whether a run can go ahead here, and \
                 `njutest verify` is the run. It writes under reports/runs",
            );
            EXIT_ASSURED
        }
        Err(source) => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: writing {}: {source}",
                    error::CONFIG_UNREADABLE.code,
                    path.display()
                ),
            );
            EXIT_ERROR
        }
    }
}
