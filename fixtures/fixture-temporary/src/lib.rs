// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library that keeps a scratch file where `TMPDIR` names, or in the standard library's temporary directory.

use std::path::PathBuf;

/// Where a scratch file is kept: where `TMPDIR` names when `named` is true, and in the standard library's temporary directory otherwise.
#[must_use]
pub fn scratch(named: bool) -> PathBuf {
    if named {
        std::env::var_os("TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from)
    } else {
        std::env::temp_dir()
    }
}

/// Keeps `text` in a file called `name` in the scratch directory `named` chooses, reads it back and removes it, and answers what it read.
///
/// # Errors
/// The file could not be written, read or removed.
pub fn round_trip(name: &str, text: &str, named: bool) -> std::io::Result<String> {
    let path = scratch(named).join(name);
    std::fs::write(&path, text)?;
    let read = std::fs::read_to_string(&path)?;
    std::fs::remove_file(&path)?;
    Ok(read)
}

/// The number after `value`, as text.
#[must_use]
pub fn next(value: u32) -> String {
    (value + 1).to_string()
}
