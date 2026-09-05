// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command line of the rust-mutants engine.
//!
//! [`run_from`] is the whole surface: it takes the argument vector and the
//! two output streams as arguments so a test can drive it without a process,
//! and returns the exit code `main` hands to the operating system.

#![forbid(unsafe_code)]

pub mod cli;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

/// The exit code of a usage error or an infrastructure failure.
pub const EXIT_USAGE: u8 = 2;

/// Runs the command line described by `args` (program name first) and
/// returns its exit code, writing to the two streams it was given.
///
/// Exit codes: `0` success, `1` an unexpected survivor, `2` a usage error or
/// an infrastructure failure, `130` interrupted.
pub fn run_from<I>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    match cli::parse(args) {
        Ok(_command) => ExitCode::SUCCESS,
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            // A closed stream is the reader's choice, not a failure of ours.
            let _written = stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush());
            ExitCode::from(usage.exit_code)
        }
    }
}
