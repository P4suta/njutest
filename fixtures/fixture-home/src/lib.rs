// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library that keeps a setting under the home directory, as a command-line tool keeps its configuration.

use std::path::PathBuf;

/// Where the setting lives: under the home directory the process was given.
#[must_use]
pub fn setting_path() -> Option<PathBuf> {
    Some(std::env::home_dir()?.join(".fixture-home").join("setting"))
}

/// Keeps `value` as the setting.
///
/// # Errors
/// There is no home directory, or the setting could not be written.
pub fn remember(value: &str) -> std::io::Result<()> {
    let path = setting_path().ok_or_else(|| std::io::Error::other("no home"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, value)
}

/// The setting as it was kept.
///
/// # Errors
/// There is no home directory, or the setting could not be read.
pub fn recall() -> std::io::Result<String> {
    let path = setting_path().ok_or_else(|| std::io::Error::other("no home"))?;
    std::fs::read_to_string(path)
}
