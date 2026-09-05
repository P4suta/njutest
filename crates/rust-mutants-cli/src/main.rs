// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `rust-mutants` binary: the composition root, and the only place that
//! reads the process's arguments, streams, and environment.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use rust_mutants_cli::Environment;

fn main() -> ExitCode {
    // The one place this program reads the process it runs in. Everything
    // below takes what it needs as an argument, which is what makes a run
    // reproducible from its inputs (ADR 0001).
    let environment = Environment {
        vars: std::env::vars_os().collect(),
        temp_directory: std::env::temp_dir(),
        working_directory: std::env::current_dir().unwrap_or_else(|_error| PathBuf::from(".")),
    };
    rust_mutants_cli::run_from(
        std::env::args_os(),
        &environment,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
