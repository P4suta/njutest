// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo xtask <gate>`: the repository gates, runnable locally and in CI.

#![forbid(unsafe_code)]

use std::io::Write as _;
use std::process::ExitCode;

pub(crate) fn main() -> ExitCode {
    let cargo = match std::env::var_os("CARGO") {
        Some(configured) => configured,
        None => std::ffi::OsString::from("cargo"),
    };
    let environment: Vec<_> = std::env::vars_os().collect();
    let directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => return unreadable("the current directory", &error),
    };
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return unreadable("the running program's path", &error),
    };
    let process = xtask::Process {
        cargo: &cargo,
        environment: &environment,
        directory: &directory,
        executable: &executable,
    };
    xtask::run_from(
        std::env::args_os(),
        &process,
        &mut xtask::Streams {
            input: &mut std::io::stdin().lock(),
            output: &mut std::io::stdout().lock(),
            errors: &mut std::io::stderr().lock(),
        },
    )
}

/// Says which part of the process could not be read, and fails.
fn unreadable(what: &str, error: &std::io::Error) -> ExitCode {
    match writeln!(
        std::io::stderr().lock(),
        "xtask: {what} cannot be read: {error}"
    ) {
        Ok(()) | Err(_) => ExitCode::FAILURE,
    }
}
