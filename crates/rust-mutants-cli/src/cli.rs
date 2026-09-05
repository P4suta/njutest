// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Argument parsing. This module knows the command tree and nothing about
//! executing it.

use std::ffi::OsString;

use clap::Parser;

/// The parsed command line.
#[derive(Debug, Clone, Copy, Parser)]
#[command(
    name = "rust-mutants",
    version = rust_mutants::VERSION,
    about = "Mutation testing for Rust: one instrumented snapshot, every mutant behind a guard",
    long_about = "Mutation testing for Rust and Cargo.\n\n\
        rust-mutants copies the workspace into a disposable snapshot, instruments every \
        compilable mutant of the selected files once, builds the test binaries once, and \
        activates one mutant per test process through an environment variable. The source \
        workspace is never modified.",
    arg_required_else_help = true,
    term_width = 100,
    color = clap::ColorChoice::Never
)]
pub struct Cli {}

/// A command that could not be parsed, or a request to print help or the
/// version, rendered for the stream it belongs on.
#[derive(Debug)]
pub struct Usage {
    /// The text to write, newline-terminated.
    pub text: String,
    /// Whether the text belongs on stderr (a diagnostic) or stdout (help).
    pub to_stderr: bool,
    /// The exit code the process ends with.
    pub exit_code: u8,
}

/// Parses `args`, program name first.
///
/// # Errors
///
/// Returns the rendered usage error, help text, or version text.
pub fn parse<I>(args: I) -> Result<Cli, Usage>
where
    I: IntoIterator<Item = OsString>,
{
    Cli::try_parse_from(args).map_err(|error| {
        let to_stderr = error.use_stderr();
        Usage {
            text: error.render().to_string(),
            to_stderr,
            exit_code: if to_stderr { crate::EXIT_USAGE } else { 0 },
        }
    })
}
