// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `rust-mutants` binary: the composition root, and the only place that reads the process's arguments, streams, environment, and signals.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use rust_mutants_cli::Environment;

fn main() -> ExitCode {
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let (cancel, signalled) = rust_mutants_cli::interruptible();
    let environment = Environment {
        temp_directory: std::env::temp_dir(),
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: std::env::current_dir().unwrap_or_else(|_error| PathBuf::from(".")),
        no_color: Environment::no_color_of(&vars),
        stdout_is_terminal: std::io::IsTerminal::is_terminal(&std::io::stdout()),
        paints: false,
        vars,
    };
    let code = rust_mutants_cli::run_from_compiled(
        std::env::args_os(),
        rust_mutants_cli::Composition::new(
            &environment,
            option_env!("RUST_MUTANTS_COMPILED_CATALOG"),
        ),
        &cancel,
        rust_mutants_cli::Streams {
            out: &mut std::io::stdout().lock(),
            err: &mut std::io::stderr().lock(),
        },
    );
    rust_mutants_cli::ended(code, &signalled)
}
