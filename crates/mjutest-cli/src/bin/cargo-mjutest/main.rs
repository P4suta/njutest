// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `cargo-mjutest` binary: the same program under the name cargo looks for, so `cargo mjutest` is `mjutest`.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let (cancel, signalled) = mjutest_cli::interruptible();
    let environment = mjutest_cli::cli::Environment {
        working_directory: std::env::current_dir().unwrap_or_else(|_error| ".".into()),
        temp_directory: std::env::temp_dir(),
        cache_directory: mjutest_cli::cli::Environment::cache_directory_of(&vars),
        vars,
        cancel,
    };
    let code = mjutest_cli::run_from(
        std::env::args_os(),
        &environment,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    mjutest_cli::ended(code, &signalled)
}
