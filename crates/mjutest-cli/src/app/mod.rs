// SPDX-FileCopyrightText: 2026 mjutest contributors
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
pub mod runs;
pub mod show;
pub mod trace;
pub mod verify;
pub mod watch;

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
        Command::Cache(arguments) => cache::run(arguments, environment, stdout, stderr),
        Command::Merge(arguments) => merge::run(arguments, stdout, stderr),
        Command::Watch(arguments) => watch::run(arguments, environment, stdout, stderr),
        Command::Lsp(arguments) => lsp::run(arguments, environment),
        Command::Doctor(arguments) => doctor::run(*arguments, environment, stdout, stderr),
        Command::Verify(arguments) => verify::run(arguments, environment, stdout, stderr),
        Command::Plan(arguments) => plan::run(arguments, environment, stdout, stderr),
        Command::Report(arguments) => show::run(arguments, environment, stdout, stderr),
        Command::Explain(arguments) => explain::run(arguments, environment, stdout, stderr),
        Command::Accept(arguments) => accept::run(arguments, environment, stdout, stderr),
        Command::Fix(arguments) => fix::run(arguments, environment, stdout, stderr),
        Command::Replay(arguments) => replay::run(arguments, environment, stdout, stderr),
        Command::Trace { command } => trace::run(command, environment, stdout, stderr),
        Command::Diagnostics(arguments) => diagnostics::run(arguments, environment, stdout, stderr),
    }
}

/// Writes one diagnostic the way every mjutest diagnostic is written: the program name, then what happened.
pub fn diagnose(stderr: &mut dyn Write, message: &str) {
    let _written = writeln!(stderr, "{PROGRAM}: {message}");
}

/// Writes one line of output.
pub fn say(stdout: &mut dyn Write, line: &str) {
    let _written = writeln!(stdout, "{line}");
}
