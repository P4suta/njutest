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
    mjutest_cli::run_from(
        std::env::args_os(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
