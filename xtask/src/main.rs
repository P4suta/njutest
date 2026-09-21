// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo xtask <gate>`: the repository gates, runnable locally and in CI.

#![forbid(unsafe_code)]

use std::process::ExitCode;

pub(crate) fn main() -> ExitCode {
    let cargo = match std::env::var_os("CARGO") {
        Some(configured) => configured,
        None => std::ffi::OsString::from("cargo"),
    };
    xtask::run_from(
        std::env::args_os(),
        &cargo,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}
