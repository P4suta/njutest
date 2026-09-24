// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Looking at a path once and saying what it is, so no reader of evidence decides at its own call site what an I/O failure means.

use std::path::{Path, PathBuf};

use crate::error::{self, ErrorCode};

/// Why looking could not say anything about a path just now, which is never a fact about the path.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SourceReadError {
    /// The process ran out of something every read needs, file descriptors or memory, so what it could not open is not known to be unreadable.
    #[error(
        "{}: reading {} ran out of what every read needs: {source}",
        error::SOURCES_UNREADABLE.code,
        path.display()
    )]
    Exhausted {
        /// What was being read.
        path: PathBuf,
        /// The operating system's refusal.
        #[source]
        source: std::io::Error,
    },
}

impl SourceReadError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Exhausted { .. } => error::SOURCES_UNREADABLE,
        }
    }
}

/// What looking at one path came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed<T> {
    /// It is what was asked for, and this is what it holds.
    Present(T),
    /// Nothing is there.
    Absent,
    /// Something is there that is not what was asked for, or that could not be read.
    Unreadable,
}

/// What one entry of a directory is, told from the entry itself and never from reading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A directory.
    Directory,
    /// A regular file.
    File,
    /// A link, a device, a socket, or anything else that is neither.
    Other,
    /// Its type could not be read.
    Unknown,
}

/// One entry of a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path.
    pub path: PathBuf,
    /// What it is.
    pub kind: Kind,
}

/// The entries of the directory at `path`.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while looking.
pub fn listing(path: &Path) -> Result<Observed<Vec<Entry>>, SourceReadError> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(source) => return failed(path, source),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(source) => {
                exhausted(path, source)?;
                return Ok(Observed::Unreadable);
            }
        };
        let kind = match entry.file_type() {
            Ok(kind) if kind.is_dir() => Kind::Directory,
            Ok(kind) if kind.is_file() => Kind::File,
            Ok(_other) => Kind::Other,
            Err(source) => {
                exhausted(&entry.path(), source)?;
                Kind::Unknown
            }
        };
        found.push(Entry {
            path: entry.path(),
            kind,
        });
    }
    found.sort_by(|one, other| one.path.cmp(&other.path));
    Ok(Observed::Present(found))
}

/// The text of the file at `path`.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
pub fn text(path: &Path) -> Result<Observed<String>, SourceReadError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(match String::from_utf8(bytes) {
            Ok(text) => Observed::Present(text),
            Err(_not_utf8) => Observed::Unreadable,
        }),
        Err(source) => failed(path, source),
    }
}

/// What a failure to look at `path` says: nothing there, or something there that could not be looked at, unless the process ran out.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where `source` is the process or the system out of descriptors or memory.
pub fn failed<T>(path: &Path, source: std::io::Error) -> Result<Observed<T>, SourceReadError> {
    if source.kind() == std::io::ErrorKind::NotFound {
        return Ok(Observed::Absent);
    }
    exhausted(path, source)?;
    Ok(Observed::Unreadable)
}

/// Refuses a failure that is the process running out of descriptors or memory, and lets every other one through as a fact about the path.
fn exhausted(path: &Path, source: std::io::Error) -> Result<(), SourceReadError> {
    if source.kind() == std::io::ErrorKind::OutOfMemory
        || source.raw_os_error().is_some_and(out_of_descriptors)
    {
        return Err(SourceReadError::Exhausted {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// Whether `code` says the process or the system has no descriptor left to open a file with.
#[cfg(unix)]
const fn out_of_descriptors(code: i32) -> bool {
    code == rustix::io::Errno::MFILE.raw_os_error()
        || code == rustix::io::Errno::NFILE.raw_os_error()
}

/// Whether `code` is Windows saying the process has no handle left to open a file with.
#[cfg(windows)]
const fn out_of_descriptors(code: i32) -> bool {
    const ERROR_TOO_MANY_OPEN_FILES: i32 = 4;
    code == ERROR_TOO_MANY_OPEN_FILES
}

/// Whether `code` says no descriptor is left, which this platform does not say.
#[cfg(not(any(unix, windows)))]
const fn out_of_descriptors(_code: i32) -> bool {
    false
}
