// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one spelling of a key in a path: a short prefix naming the place whose record keeps the whole key.

use std::io;
use std::path::Path;

/// How many leading hex digits of a key name the directory or record that keeps it.
pub const NAME_LENGTH: usize = 16;

/// The file in a directory a key's prefix names that holds the whole key.
pub const RECORD_NAME: &str = "key";

/// The first [`NAME_LENGTH`] digits of `key`, which is all of it a path ever spells.
///
/// # Errors
/// `key` is shorter than a name or holds anything but lowercase hex digits.
pub fn name(key: &str) -> io::Result<&str> {
    match key.get(..NAME_LENGTH) {
        Some(name)
            if key
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) =>
        {
            Ok(name)
        }
        Some(_) | None => Err(io::Error::other(format!(
            "{key:?} is not a lowercase hex key of at least {NAME_LENGTH} digits"
        ))),
    }
}

/// Whether `text` is exactly a name [`name`] spells.
#[must_use]
pub fn names(text: &str) -> bool {
    text.len() == NAME_LENGTH && name(text).is_ok()
}

/// What a directory a key's prefix names holds for that key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// Its record keeps the key, written now where it was absent.
    Bound,
    /// Its record keeps another key that shares the name.
    Another,
}

/// Keeps the whole of `key` in the record of `directory`, whose exclusive lease the caller holds, or says that the record keeps another key.
///
/// # Errors
/// `key` is not a key, or the record could not be read or written.
pub fn binding(directory: &Path, key: &str) -> io::Result<Binding> {
    name(key)?;
    let record = directory.join(RECORD_NAME);
    match std::fs::read(&record) {
        Ok(held) if held == key.as_bytes() => Ok(Binding::Bound),
        Ok(_another) => Ok(Binding::Another),
        Err(absent) if absent.kind() == io::ErrorKind::NotFound => {
            crate::replace::file(&record, key.as_bytes()).map_err(|failure| failure.source)?;
            Ok(Binding::Bound)
        }
        Err(source) => Err(source),
    }
}

/// [`binding`], refusing a directory whose record keeps another key.
///
/// # Errors
/// Those of [`binding`], and a record that keeps another key.
pub fn bind(directory: &Path, key: &str) -> io::Result<()> {
    match binding(directory, key)? {
        Binding::Bound => Ok(()),
        Binding::Another => Err(io::Error::other(format!(
            "{} keeps another key named {}",
            directory.display(),
            name(key)?
        ))),
    }
}

#[cfg(test)]
mod tests;
