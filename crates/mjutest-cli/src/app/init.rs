// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest init`: write the annotated configuration skeleton.
//!
//! The skeleton is the two active defaults and every other section as
//! commented guidance, and a test loads the untouched file and asserts it is
//! exactly `Config::default()` — so what `init` writes can never drift from
//! what configuring nothing does.

use std::io::Write;

use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Init};
use crate::config;
use crate::error;

/// Writes `.mjutest.toml` beside the working directory.
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
