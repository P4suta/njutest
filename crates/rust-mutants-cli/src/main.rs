// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `rust-mutants` binary: the composition root, and the only place that
//! reads the process's arguments, streams, and environment.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    rust_mutants_cli::run_from(
        std::env::args_os(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
