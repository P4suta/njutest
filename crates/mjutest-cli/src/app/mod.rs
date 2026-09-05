// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each command does, once the command line has been understood.
//!
//! Every command here takes the machine as an argument ([`Environment`]) and
//! writes to the two streams it was handed, so a test drives one exactly as
//! the binary does and reads back what a person would have seen.

pub mod diagnostics;
pub mod doctor;
pub mod init;
pub mod plan;
pub mod reports;
pub mod runs;
pub mod show;
pub mod trace;
pub mod verify;

use std::io::Write;

use crate::cli::{Command, Environment, PROGRAM, Request};

/// Runs what the command line asked for and answers with the exit code.
pub fn run(
    request: &Request,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    match &request.command {
        Command::Init(arguments) => init::run(*arguments, environment, stdout, stderr),
        Command::Doctor(arguments) => doctor::run(*arguments, environment, stdout, stderr),
        Command::Verify(arguments) => verify::run(arguments, environment, stdout, stderr),
        Command::Plan(arguments) => plan::run(arguments, environment, stdout, stderr),
        Command::Report(arguments) => show::run(arguments, environment, stdout, stderr),
        Command::Trace { command } => trace::run(command, environment, stdout, stderr),
        Command::Diagnostics(arguments) => diagnostics::run(arguments, environment, stdout, stderr),
    }
}

/// Writes one diagnostic the way every mjutest diagnostic is written: the
/// program name, then what happened.
///
/// A closed stream is the reader's choice, not a failure of ours.
pub fn diagnose(stderr: &mut dyn Write, message: &str) {
    let _written = writeln!(stderr, "{PROGRAM}: {message}");
}

/// Writes one line of output.
pub fn say(stdout: &mut dyn Write, line: &str) {
    let _written = writeln!(stdout, "{line}");
}
