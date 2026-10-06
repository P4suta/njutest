// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A directory held open and every operation on it named relative to it, so what a caller opens is the object it vouched for and never one a name was pointed at afterwards (ADR 0037).

#[cfg(any(windows, test))]
mod records;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(all(test, windows))]
pub(crate) use windows::tests::make_execution_alias;

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
        let stem = match text.split_once('.') {
            Some((stem, _extension)) => stem,
            None => text,
        };
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
    /// A Windows app execution alias: a reparse point only process creation follows, which no open of it as a file can.
    ExecutionAlias,
    /// Anything else: a link, a device, a pipe.
    Other,
}

/// Which object an entry is: the volume and the object on it, stable across renames, which tells two handles on one object apart from two objects and says nothing about what either holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Identity {
    /// The volume.
    volume: u64,
    /// The object on it.
    object: u128,
}

/// The directories the Windows loader searches before any `PATH` entry, as the operating system names them rather than as an environment says: the system directory, the 16-bit system directory beside it, and the Windows directory.
///
/// # Errors
/// The operating system names no system or Windows directory.
#[cfg(windows)]
pub(crate) fn fixed_search_directories() -> io::Result<[std::path::PathBuf; 3]> {
    let (system, windows) = sys::system_directories()?;
    let sixteen = system
        .parent()
        .map(|parent| parent.join("System"))
        .ok_or_else(|| io::Error::other("the system directory has no parent"))?;
    Ok([system, sixteen, windows])
}

/// The current user's app execution alias directory, `Microsoft\WindowsApps` in the local application data directory the operating system names for the user, rather than one an environment says.
///
/// # Errors
/// The operating system names no local application data directory for the current user.
#[cfg(windows)]
pub(crate) fn execution_alias_directory() -> io::Result<std::path::PathBuf> {
    Ok(sys::local_app_data()?.join("Microsoft").join("WindowsApps"))
}

/// The held Windows object's volume, object and metadata change time, distinct from its writable mtime, as a change stamp records them.
///
/// # Errors
/// The filesystem cannot provide the object's identity or change time.
#[cfg(windows)]
pub(crate) fn change_stamp(file: &File) -> io::Result<(u64, crate::wide::Wide, i64)> {
    let Identity { volume, object } = sys::file_status(file)?.identity;
    Ok((volume, object.into(), sys::change_time(file)?))
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

/// An entry opened once without following it, as what the opened handle turned out to be.
#[derive(Debug)]
pub enum Entry {
    /// A regular file, open for reading.
    File(File),
    /// A directory, held.
    Dir(Dir),
    /// Anything else, closed again unread.
    Other,
}

/// How many directories deep [`Dir::remove_contents`] goes before it refuses, rather than running out of handles on the way down.
pub const REMOVAL_DEPTH: usize = 64;

/// Who may reach into a directory, read from its owner and its permissions.
///
/// On Unix owner-only is exactly read, write and enter for the owner, with no setuid, setgid or sticky bit: a directory made under a setgid parent reads as [`Privacy::Loose`] and is tightened.
/// An access control list can grant what the mode bits do not show, and is not read there.
/// On Windows owner-only is a protected access control list admitting this user with everything, the system, and the administrators, and nobody else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    /// This process's user owns it and nobody else may enter it.
    OwnerOnly,
    /// This process's user owns it, and its permissions are anything but read, write and enter for the owner alone.
    Loose,
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

    /// The entry `name`, opened once without following a link or waiting on a pipe, as what the open handle is; a link is refused or [`Entry::Other`].
    ///
    /// # Errors
    /// It is missing, or cannot be opened or inspected.
    pub fn open_entry(&self, name: Name<'_>) -> io::Result<Entry> {
        let (handle, kind) = sys::open_entry(&self.handle, name)?;
        Ok(match kind {
            Kind::File => Entry::File(handle),
            Kind::Directory => Entry::Dir(Self { handle }),
            Kind::ExecutionAlias | Kind::Other => Entry::Other,
        })
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

    /// Removes everything beneath this directory, leaving it empty and held.
    ///
    /// Only the object that was looked at is removed, so nothing another process put in its place is touched, and a link is removed as a link.
    /// On Unix each entry is renamed aside under a fresh name and removed only if the aside name still holds what was renamed; an entry that changed as it was set aside is put back, and if its name was taken meanwhile it stays under its aside name, which a later emptying removes without the identity check, a residue only a directory its owner alone may enter can afford.
    /// On Windows each entry is removed through the handle it was opened by, which is the object itself.
    /// Entries are handled by the names the platform holds, so one no [`Name`] could spell is removed too, and one gone before it was reached counts as removed.
    /// A tree deeper than [`REMOVAL_DEPTH`] is refused before anything beneath that depth is touched.
    ///
    /// # Errors
    /// An entry cannot be renamed aside, changed identity, or cannot be removed.
    pub fn remove_contents(&self) -> io::Result<()> {
        sys::remove_contents(&self.handle)
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

/// Makes what was written to an open file durable, whether it was opened for reading or for writing.
///
/// # Errors
/// The volume refuses to flush it.
pub fn sync_file(file: &File) -> io::Result<()> {
    sys::sync_file(file)
}

/// The file or directory at `path`, for reading, not following a final link and not waiting on a pipe.
///
/// # Errors
/// It is missing, a link, or cannot be opened.
pub fn open_file_at(path: &Path) -> io::Result<File> {
    sys::open_file_at(path)
}

#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use windows as sys;
