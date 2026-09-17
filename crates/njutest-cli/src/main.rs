// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `njutest` binary: a composition root, and one of the two places that read the process's arguments, streams, environment, and signals.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let (cancel, signalled) = njutest_cli::interruptible();
    let environment = njutest_cli::cli::Environment {
        working_directory: std::env::current_dir().unwrap_or_else(|_error| ".".into()),
        temp_directory: std::env::temp_dir(),
        program: std::env::current_exe()
            .unwrap_or_else(|_error| std::path::PathBuf::from("njutest")),
        cache_directory: njutest_cli::cli::Environment::cache_directory_of(&vars),
        terminal: njutest_cli::presentation::Terminal::of(&njutest_cli::asked(&vars)),
        vars,
        cancel,
    };
    let code = njutest_cli::run_from(
        std::env::args_os(),
        &environment,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    njutest_cli::ended(code, &signalled)
}
