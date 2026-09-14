// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One spelling for a directory, whatever the platform calls it.
//!
//! A tree is reached through symbolic links, short names, and relative parts,
//! and the same directory named two ways is two directories to anything that
//! compares names. Resolving it is what [`std::fs::canonicalize`] is for, and
//! CONTRIBUTING.md asks for it before any comparison.
//!
//! What it returns on Windows is the extended-length form, `\\?\C:\…`, and the
//! rest of a run's paths do not come from there: cargo prints a manifest path
//! plainly, and a person types one plainly. A run that canonicalized some of
//! its paths and not others would hold two spellings of one place, report the
//! prefix to a reader who never wrote it, and — because an extended-length
//! path turns off the normalisation that lets a forward slash stand for a
//! separator — fail to find a file whose name it had just built. So the prefix
//! comes off again where the path is an ordinary one on a drive.

use std::io;
use std::path::{Path, PathBuf};

/// What Windows puts in front of an extended-length path.
#[cfg(windows)]
const VERBATIM: &str = r"\\?\";

/// `path` resolved, spelled the way the rest of this run spells one.
///
/// # Errors
/// Whatever [`std::fs::canonicalize`] reports: the path does not exist, or it
/// could not be read.
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    path.canonicalize().map(|resolved| plainly(&resolved))
}

/// `path` without the prefix Windows adds to an extended-length path, when taking it off names the same place.
///
/// A UNC path (`\\?\UNC\server\share`) keeps its prefix: what it becomes
/// without one is a different path rather than a plainer spelling of the same.
#[must_use]
pub fn plainly(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(VERBATIM)
            && !rest.starts_with("UNC\\")
            && Path::new(rest).is_absolute()
        {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}
