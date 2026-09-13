// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo xtask <gate>`: the repository gates, runnable locally and in CI.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    xtask::run_from(
        std::env::args_os(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
