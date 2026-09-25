// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calls that can fail, some of whose failures the tests would see and some they would not.

use std::path::Path;

/// The text of the file at `path`.
///
/// # Errors
/// Whatever reading it said.
pub fn load(path: &Path) -> std::io::Result<String> {
    let text = std::fs::read_to_string(path)?;
    Ok(text)
}

/// The number `text` spells.
///
/// # Errors
/// Whatever parsing it said.
pub fn number(text: &str) -> Result<u8, std::num::ParseIntError> {
    let parsed = text.trim().parse::<u8>()?;
    Ok(parsed)
}

/// How long the file at `path` is, where nobody checks whether reading it worked.
#[must_use]
pub fn length(path: &Path) -> usize {
    measured(path).unwrap_or_default()
}

fn measured(path: &Path) -> std::io::Result<usize> {
    let text = std::fs::read_to_string(path)?;
    Ok(text.len())
}

/// A failure only this crate can make.
#[derive(Debug, PartialEq, Eq)]
pub struct Refused;

/// Seven, through an error type the engine cannot make.
///
/// # Errors
/// Never, as written.
pub fn ours() -> Result<u8, Refused> {
    let seven = inner()?;
    Ok(seven)
}

fn inner() -> Result<u8, Refused> {
    Ok(7)
}

/// The value inside, through an `Option`.
#[must_use]
pub fn maybe(value: Option<u8>) -> Option<u8> {
    let inside = value?;
    Some(inside)
}
