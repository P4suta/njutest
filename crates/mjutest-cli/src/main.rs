// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `mjutest` binary: the composition root.
//!
//! This is the one layer that knows the running process is an mjutest binary
//! on a particular machine. It alone reads the process's arguments, streams,
//! and environment, and it alone will name the executable, the user cache
//! directory, and the temporary directory; everything below it is configured
//! by options.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let environment = mjutest_cli::cli::Environment {
        working_directory: std::env::current_dir().unwrap_or_else(|_error| ".".into()),
        temp_directory: std::env::temp_dir(),
        cache_directory: mjutest_cli::cli::Environment::cache_directory_of(&vars),
        vars,
    };
    mjutest_cli::run_from(
        std::env::args_os(),
        &environment,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
