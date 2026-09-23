// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each command does, once the command line has been understood.

pub mod accept;
pub mod cache;
pub mod diagnostics;
pub mod doctor;
pub mod explain;
pub mod fix;
pub mod init;
pub mod lsp;
pub mod merge;
pub mod plan;
pub mod replay;
pub mod reports;
pub mod review;
pub mod runs;
pub mod show;
pub mod trace;
pub mod verify;
pub mod watch;
pub mod why;

use std::io::Write;

use crate::cli::{Command, EXIT_ASSURED, EXIT_ERROR, Environment, PROGRAM, Request};

/// Runs what the command line asked for and answers with the exit code.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    request: &Request,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    match &request.command {
        Command::Init(arguments) => init::run(*arguments, environment, stdout, stderr),
        Command::Cache(arguments) => cache::run(arguments, environment, stdout, stderr),
        Command::Merge(arguments) => merge::run(arguments, stdout, stderr),
        Command::Watch(arguments) => watch::run(arguments, environment, stdout, stderr),
        Command::Lsp(arguments) => Ok(lsp::run(arguments, environment)),
        Command::Doctor(arguments) => {
            doctor::run(*arguments, environment, stdout, stderr).map(exit)
        }
        Command::Verify(arguments) => verify::run(arguments, environment, stdout, stderr),
        Command::Plan(arguments) => plan::run(arguments, environment, stdout, stderr).map(exit),
        Command::Report(arguments) => show::run(arguments, environment, stdout, stderr),
        Command::Explain(arguments) => explain::run(arguments, environment, stdout, stderr),
        Command::Why(arguments) => why::run(arguments, environment, stdout, stderr),
        Command::Accept(arguments) => accept::run(arguments, environment, stdout, stderr),
        Command::Review(arguments) => review::run(arguments, environment, stdout, stderr),
        Command::Fix(arguments) => fix::run(arguments, environment, stdout, stderr),
        Command::Replay(arguments) => replay::run(arguments, environment, stdout, stderr),
        Command::Trace { command } => trace::run(command, environment, stdout, stderr),
        Command::Diagnostics(arguments) => diagnostics::run(arguments, environment, stdout, stderr),
    }
}

/// A command boundary before it is translated to the process exit policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Completion {
    /// The command established what it was asked to.
    Assured,
    /// The command diagnosed why it could not.
    Error,
}

/// Turns a command's boundary into the process policy.
const fn exit(result: Completion) -> u8 {
    match result {
        Completion::Assured => EXIT_ASSURED,
        Completion::Error => EXIT_ERROR,
    }
}

/// Writes one diagnostic the way every njutest diagnostic is written: the program name, then what happened.
///
/// # Errors
/// Returns the diagnostic stream's write failure.
pub fn diagnose(stderr: &mut dyn Write, message: &str) -> std::io::Result<()> {
    writeln!(stderr, "{PROGRAM}: {message}")
}

/// Writes one failure and the next step it carries.
///
/// # Errors
/// Returns the diagnostic stream's write failure.
pub fn complain(
    stderr: &mut dyn Write,
    error: &impl std::fmt::Display,
    code: crate::error::ErrorCode,
) -> std::io::Result<()> {
    diagnose(stderr, &error.to_string())?;
    writeln!(stderr, "        try: {}", code.remedy)
}

/// Writes one line of output.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn say(stdout: &mut dyn Write, line: &str) -> std::io::Result<()> {
    writeln!(stdout, "{line}")
}
