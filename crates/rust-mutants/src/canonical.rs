// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One spelling for a directory, whatever the platform calls it.

use std::io;
use std::path::{Path, PathBuf};

/// `path` resolved, spelled the way the rest of this run spells one.
///
/// # Errors
/// Whatever [`std::fs::canonicalize`] reports: the path does not exist, or it could not be read.
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    path.canonicalize().map(|resolved| plainly(&resolved))
}

/// Normalizes equivalent Windows disk and UNC prefixes while preserving distinct filesystem roots.
#[must_use]
pub fn plainly(path: &Path) -> PathBuf {
    njutest_fixture_tree::filesystem_spelling(path)
}
