// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A count kept on disk by a program that ends with a crash's exit status of its own, just before the one call that writes.

use std::path::Path;

/// Keeps `count` at `path`, ending the process with status 93 first whenever a perturbation is active.
///
/// # Errors
/// Whatever writing it said.
pub fn save(path: &Path, count: u32) -> std::io::Result<()> {
    if std::env::var_os("RUST_MUTANTS_ACTIVE").is_some_and(|active| !active.is_empty()) {
        std::process::exit(93);
    }
    std::fs::write(path, count.to_string())
}
