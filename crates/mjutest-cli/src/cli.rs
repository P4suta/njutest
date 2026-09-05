// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Argument parsing and the exit codes. This module knows the command tree
//! and the verdict-to-exit-code mapping, and nothing about executing a run.

use clap::Parser;
use std::ffi::OsString;

/// `ASSURED`, `CHANGE_ASSURED`, `SCOPE_ASSURED`, `RESOLVED`, or `COMPLETED`.
pub const EXIT_ASSURED: u8 = 0;
/// `DEFECT` or `REPRODUCED`.
pub const EXIT_DEFECT: u8 = 1;
/// `INSUFFICIENT`.
pub const EXIT_INSUFFICIENT: u8 = 2;
/// `ERROR`, invalid input, or an infrastructure failure.
pub const EXIT_ERROR: u8 = 3;
/// Interrupted (SIGINT).
pub const EXIT_INTERRUPTED: u8 = 130;
/// Terminated (SIGTERM).
pub const EXIT_TERMINATED: u8 = 143;

/// The program name every diagnostic line starts with.
pub const PROGRAM: &str = "mjutest";

/// The parsed command line.
#[derive(Debug, Clone, Copy, Parser)]
#[command(
    name = "mjutest",
    version = crate::VERSION,
    about = "An audit-oriented assurance runner for Rust",
    long_about = "An audit-oriented assurance runner for Rust.\n\n\
        mjutest connects ordinary Cargo tests with coverage routing, mutation testing through \
        rust-mutants, paired kill confirmation, a soundness phase, targeted fuzzing, explicit \
        integration resources, and reviewable repair candidates. It reports a verdict — \
        ASSURED, DEFECT, INSUFFICIENT, ERROR — and never a percentage.",
    after_help = "Exit codes:\n  \
        0    ASSURED, CHANGE_ASSURED, SCOPE_ASSURED, RESOLVED, or COMPLETED\n  \
        1    DEFECT or REPRODUCED\n  \
        2    INSUFFICIENT\n  \
        3    ERROR, invalid input, or an infrastructure failure\n  \
        130  interrupted\n  \
        143  terminated",
    term_width = 100,
    color = clap::ColorChoice::Never
)]
pub struct Request {}

/// A command line that could not be parsed, or a request to print help or the
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

/// Parses `args`, program name first. A bare invocation is the help text, as
/// it is in goatest.
///
/// # Errors
///
/// Returns the rendered usage error, help text, or version text.
pub fn parse<I>(args: I) -> Result<Request, Usage>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().collect();
    if args.len() <= 1 {
        args.push(OsString::from("--help"));
    }
    Request::try_parse_from(args).map_err(|error| {
        if error.use_stderr() {
            Usage {
                text: diagnose(&error.render().to_string()),
                to_stderr: true,
                exit_code: EXIT_ERROR,
            }
        } else {
            Usage {
                text: error.render().to_string(),
                to_stderr: false,
                exit_code: EXIT_ASSURED,
            }
        }
    })
}

/// Renders a diagnostic the way every mjutest diagnostic is rendered: one
/// `mjutest: ` prefix, a lowercase message, and the usage that follows it.
fn diagnose(rendered: &str) -> String {
    let message = rendered.strip_prefix("error: ").unwrap_or(rendered);
    format!("{PROGRAM}: {message}")
}
