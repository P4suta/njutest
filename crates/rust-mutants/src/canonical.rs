// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One spelling for a directory, whatever the platform calls it.

use std::io;
use std::path::{Path, PathBuf};

/// What Windows puts in front of an extended-length path.
#[cfg(windows)]
const VERBATIM: &str = r"\\?\";

/// `path` resolved, spelled the way the rest of this run spells one.
///
/// # Errors
/// Whatever [`std::fs::canonicalize`] reports: the path does not exist, or it could not be read.
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    path.canonicalize().map(|resolved| plainly(&resolved))
}

/// `path` without the prefix Windows adds to an extended-length path, when taking it off names the same place.
#[must_use]
pub fn plainly(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        match path.strip_prefix(VERBATIM) {
            Ok(rest)
                if !rest.as_os_str().as_encoded_bytes().starts_with(b"UNC\\")
                    && rest.is_absolute() =>
            {
                return rest.to_path_buf();
            }
            Ok(_) | Err(_) => {}
        }
    }
    path.to_path_buf()
}
