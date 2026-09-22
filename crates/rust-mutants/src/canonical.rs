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
///
/// The prefix comes off the spelling rather than the components: `Path::strip_prefix` matches whole components, and `\\?\C:\…` parses as one verbatim-disk prefix that no pattern spells, so it never matched and every canonicalized path kept the form cargo does not print.
#[must_use]
pub fn plainly(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(rest) = path.to_str().and_then(|text| text.strip_prefix(VERBATIM))
            && !rest.starts_with("UNC\\")
        {
            let plain = Path::new(rest);
            if plain.is_absolute() {
                return plain.to_path_buf();
            }
        }
    }
    path.to_path_buf()
}
