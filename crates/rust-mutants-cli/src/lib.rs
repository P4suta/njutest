// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command line of the rust-mutants engine.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod config;
pub mod error;
pub mod report;
pub mod run;
pub mod settings;

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use rust_mutants::runner::Cancel;

/// The exit code of a usage error or an infrastructure failure.
pub const EXIT_USAGE: u8 = 2;

/// Everything the command line needs from the process it runs in.
#[derive(Debug)]
pub struct Environment {
    /// The process environment, which the engine hands to every command and test process it starts.
    pub vars: Vec<(OsString, OsString)>,
    /// The directory snapshots and target directories are created in.
    pub temp_directory: PathBuf,
    /// The working directory, which a command with no `--root` reads.
    pub working_directory: PathBuf,
}

/// Runs the command line described by `args` (program name first) and returns its exit code, writing to the two streams it was given.
pub fn run_from<I>(
    args: I,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            let _written = stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush());
            return ExitCode::from(usage.exit_code);
        }
    };
    let cancel = Cancel::new();
    match app::dispatch(&command.command, environment, stdout, &cancel) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let _written = writeln!(stderr, "rust-mutants: {error}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}
