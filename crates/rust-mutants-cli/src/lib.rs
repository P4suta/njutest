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

/// The exit code of a run that was interrupted.
pub const EXIT_INTERRUPTED: u8 = 130;

/// The two streams a command writes to.
#[expect(
    missing_debug_implementations,
    reason = "a stream is a handle to the outside; there is nothing to print about one"
)]
pub struct Streams<'a> {
    /// What the command established.
    pub out: &'a mut dyn Write,
    /// What went wrong.
    pub err: &'a mut dyn Write,
}

/// Runs the command line described by `args` (program name first) and returns its exit code, writing to the two streams it was given. `cancel` is raised by whoever owns the process's signals; every command stops at the first place it can and leaves nothing behind.
pub fn run_from<I>(args: I, environment: &Environment, cancel: &Cancel, streams: Streams<'_>) -> u8
where
    I: IntoIterator<Item = OsString>,
{
    let Streams {
        out: stdout,
        err: stderr,
    } = streams;
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            let _written = stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush());
            return usage.exit_code;
        }
    };
    match app::dispatch(&command.command, environment, stdout, cancel) {
        Ok(code) => code,
        Err(error) if cancel.is_cancelled() => {
            let _written = writeln!(stderr, "rust-mutants: {error}");
            EXIT_INTERRUPTED
        }
        Err(error) => {
            let _written = writeln!(stderr, "rust-mutants: {error}");
            EXIT_USAGE
        }
    }
}
