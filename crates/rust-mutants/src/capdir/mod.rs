// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A directory held open and every operation on it named relative to it, so what a caller opens is the object it vouched for and never one a name was pointed at afterwards (ADR 0037).

#[cfg(unix)]
mod unix;

use std::fs::File;
use std::io;
use std::path::Path;

use crate::error::{self, ErrorCode};

/// One path component a directory can hold, refused if it could name anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Name<'a>(&'a str);

/// Why a string is not one component.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NameError {
    /// It is not one component on every platform the store runs on.
    #[error("{}: {name:?} is not one path component: {why}", error::CAPDIR_NAME_REFUSED.code)]
    Refused {
        /// The text refused.
        name: String,
        /// Which rule it broke.
        why: &'static str,
    },
}

impl NameError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Refused { .. } => error::CAPDIR_NAME_REFUSED,
        }
    }
}

/// The device names Windows reserves in every directory, whatever follows the dot.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

impl<'a> Name<'a> {
    /// `text` as one component.
    ///
    /// # Errors
    /// [`NameError::Refused`] for an empty name, `.` or `..`, a separator, `:` or NUL, a trailing dot or space, or a reserved device name.
    pub fn new(text: &'a str) -> Result<Self, NameError> {
        let refused = |why| NameError::Refused {
            name: text.to_owned(),
            why,
        };
        if text.is_empty() {
            return Err(refused("it is empty"));
        }
        if matches!(text, "." | "..") {
            return Err(refused("it names this directory or its parent"));
        }
        if text.contains(['/', '\\', ':', '\0']) {
            return Err(refused("it holds a separator, a stream marker or NUL"));
        }
        if text.ends_with(['.', ' ']) {
            return Err(refused("Windows drops a trailing dot or space"));
        }
        let stem = text.split('.').next().unwrap_or(text);
        if RESERVED
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(stem))
        {
            return Err(refused("Windows reserves it as a device"));
        }
        Ok(Self(text))
    }

    /// The component as written.
    #[must_use]
    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

/// What kind of object an entry is, without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// Anything else: a link, a device, a pipe.
    Other,
}

/// Which object an entry is: the volume and the object on it, stable across renames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity {
    /// The volume.
    pub volume: u64,
    /// The object on it.
    pub object: u128,
}

/// What a directory entry is, read without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// Which object.
    pub identity: Identity,
    /// What kind.
    pub kind: Kind,
    /// How many bytes a file holds.
    pub len: u64,
}

/// Who may reach into a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    /// This process's user owns it and nobody else may enter it.
    OwnerOnly,
    /// This process's user owns it and somebody else may enter it.
    Wider,
    /// Another user owns it.
    ForeignOwner,
}

/// A directory held open, relative to which every operation is named.
#[derive(Debug)]
pub struct Dir {
    handle: File,
}

impl Dir {
    /// Opens the directory at `path` without following a final link.
    ///
    /// # Errors
    /// The path is not a directory, is a link, or cannot be opened.
    pub fn open(path: &Path) -> io::Result<Self> {
        sys::open(path).map(|handle| Self { handle })
    }

    /// A second handle on the same directory.
    ///
    /// # Errors
    /// The handle could not be duplicated.
    pub fn try_clone(&self) -> io::Result<Self> {
        self.handle.try_clone().map(|handle| Self { handle })
    }

    /// The child directory `name`, not following a link.
    ///
    /// # Errors
    /// It is missing, a link, or not a directory.
    pub fn open_dir(&self, name: Name<'_>) -> io::Result<Self> {
        sys::open_dir(&self.handle, name).map(|handle| Self { handle })
    }

    /// The child file `name`, for reading, not following a link and not waiting on a pipe.
    ///
    /// # Errors
    /// It is missing, a link, or cannot be opened.
    pub fn open_file(&self, name: Name<'_>) -> io::Result<File> {
        sys::open_file(&self.handle, name)
    }

    /// A new file `name`, for writing, readable by its owner alone; an entry already there, of any kind, refuses it.
    ///
    /// # Errors
    /// The name is taken, or the file cannot be made.
    pub fn create_file(&self, name: Name<'_>) -> io::Result<File> {
        sys::create_file(&self.handle, name)
    }

    /// A new directory `name` that only its owner may enter, opened as it was made; an entry already there refuses it.
    ///
    /// # Errors
    /// The name is taken, or the directory cannot be made or opened.
    pub fn create_private_dir_exclusive(&self, name: Name<'_>) -> io::Result<Self> {
        sys::create_private_dir(&self.handle, name).map(|handle| Self { handle })
    }

    /// What this directory is.
    ///
    /// # Errors
    /// It cannot be inspected.
    pub fn status(&self) -> io::Result<Status> {
        file_status(&self.handle)
    }

    /// What the entry `name` is, without following it, or nothing when there is none.
    ///
    /// # Errors
    /// It cannot be inspected for any reason but its absence.
    pub fn status_at(&self, name: Name<'_>) -> io::Result<Option<Status>> {
        sys::status_at(&self.handle, name)
    }

    /// Moves `from` here to `to` in `into`, refusing if `to` is taken.
    ///
    /// # Errors
    /// The target is taken, or the rename fails.
    pub fn rename_noreplace(&self, from: Name<'_>, into: &Self, to: Name<'_>) -> io::Result<()> {
        sys::rename_noreplace(&self.handle, from, &into.handle, to)
    }

    /// Moves `from` here to `to` in `into`, replacing a file already there.
    ///
    /// # Errors
    /// The rename fails.
    pub fn rename_replace(&self, from: Name<'_>, into: &Self, to: Name<'_>) -> io::Result<()> {
        sys::rename_replace(&self.handle, from, &into.handle, to)
    }

    /// Removes the file `name`.
    ///
    /// # Errors
    /// It is missing, a directory, or cannot be removed.
    pub fn remove_file(&self, name: Name<'_>) -> io::Result<()> {
        sys::remove(&self.handle, name, false)
    }

    /// Removes the empty directory `name`.
    ///
    /// # Errors
    /// It is missing, not empty, or cannot be removed.
    pub fn remove_dir(&self, name: Name<'_>) -> io::Result<()> {
        sys::remove(&self.handle, name, true)
    }

    /// Makes what was renamed into or out of this directory durable.
    ///
    /// # Errors
    /// The volume refuses to flush it.
    pub fn sync(&self) -> io::Result<()> {
        sys::sync(&self.handle)
    }

    /// The names this directory holds, without `.` and `..`.
    ///
    /// # Errors
    /// It cannot be listed, or a name is not UTF-8.
    pub fn entries(&self) -> io::Result<Vec<String>> {
        sys::entries(&self.handle)
    }

    /// Who may reach into this directory.
    ///
    /// # Errors
    /// Its owner or permissions cannot be read.
    pub fn privacy(&self) -> io::Result<Privacy> {
        sys::privacy(&self.handle)
    }

    /// Takes every access but its owner's away from this directory.
    ///
    /// # Errors
    /// Its permissions cannot be changed.
    pub fn restrict_to_owner(&self) -> io::Result<()> {
        sys::restrict_to_owner(&self.handle)
    }
}

/// What an open file is.
///
/// # Errors
/// It cannot be inspected.
pub fn file_status(file: &File) -> io::Result<Status> {
    sys::file_status(file)
}

#[cfg(unix)]
use unix as sys;

#[cfg(not(unix))]
mod sys {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    use super::{Name, Privacy, Status};

    fn unsupported<T>() -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this platform has no capability directory yet",
        ))
    }

    pub(super) fn open(_path: &Path) -> io::Result<File> {
        unsupported()
    }
    pub(super) fn open_dir(_dir: &File, _name: Name<'_>) -> io::Result<File> {
        unsupported()
    }
    pub(super) fn open_file(_dir: &File, _name: Name<'_>) -> io::Result<File> {
        unsupported()
    }
    pub(super) fn create_file(_dir: &File, _name: Name<'_>) -> io::Result<File> {
        unsupported()
    }
    pub(super) fn create_private_dir(_dir: &File, _name: Name<'_>) -> io::Result<File> {
        unsupported()
    }
    pub(super) fn status_at(_dir: &File, _name: Name<'_>) -> io::Result<Option<Status>> {
        unsupported()
    }
    pub(super) fn rename_noreplace(
        _from_dir: &File,
        _from: Name<'_>,
        _to_dir: &File,
        _to: Name<'_>,
    ) -> io::Result<()> {
        unsupported()
    }
    pub(super) fn rename_replace(
        _from_dir: &File,
        _from: Name<'_>,
        _to_dir: &File,
        _to: Name<'_>,
    ) -> io::Result<()> {
        unsupported()
    }
    pub(super) fn remove(_dir: &File, _name: Name<'_>, _directory: bool) -> io::Result<()> {
        unsupported()
    }
    pub(super) fn sync(_dir: &File) -> io::Result<()> {
        unsupported()
    }
    pub(super) fn entries(_dir: &File) -> io::Result<Vec<String>> {
        unsupported()
    }
    pub(super) fn privacy(_dir: &File) -> io::Result<Privacy> {
        unsupported()
    }
    pub(super) fn restrict_to_owner(_dir: &File) -> io::Result<()> {
        unsupported()
    }
    pub(super) fn file_status(_file: &File) -> io::Result<Status> {
        unsupported()
    }
}
