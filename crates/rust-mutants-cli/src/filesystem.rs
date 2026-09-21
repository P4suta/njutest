// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exact filesystem entry classification without collapsing an I/O failure into absence.

use std::path::Path;

/// The kind of directory entry present at one path, without following a symbolic link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "sibling modules share this private filesystem-policy vocabulary"
)]
pub(crate) enum EntryKind {
    /// No entry is present.
    Missing,
    /// A regular file is present.
    File,
    /// A directory is present.
    Directory,
    /// An entry of another kind, including a symbolic link, is present.
    Other,
}

/// Classifies `path` while preserving every metadata error other than absence.
///
/// # Errors
/// Returns the operating system failure when the entry's metadata cannot be read.
#[expect(
    clippy::redundant_pub_crate,
    reason = "sibling modules share this private filesystem-policy boundary"
)]
pub(crate) fn entry_kind(path: &Path) -> std::io::Result<EntryKind> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(EntryKind::File),
        Ok(metadata) if metadata.file_type().is_dir() => Ok(EntryKind::Directory),
        Ok(_other) => Ok(EntryKind::Other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(EntryKind::Missing),
        Err(error) => Err(error),
    }
}
