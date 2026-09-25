// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Replacing a file so that whoever reads it next reads one whole version of it.

use std::path::{Path, PathBuf};

/// The path a replacement could not be made through, and what the filesystem said.
#[derive(Debug)]
#[non_exhaustive]
pub struct Failure {
    /// The path that refused: the destination, or the directory it goes in.
    pub path: PathBuf,
    /// What the filesystem said.
    pub source: std::io::Error,
}

/// Both failures from refusing a replacement and then refusing to remove its staged bytes.
#[derive(Debug, thiserror::Error)]
#[error("renaming the staged file failed ({rename}); removing it also failed ({cleanup})")]
struct RenameCleanupError {
    rename: std::io::Error,
    cleanup: std::io::Error,
}

/// Writes `bytes` where `path` is, so that a reader holds the whole of what was there or the whole of this.
///
/// # Errors
/// See [`Failure`].
pub fn file(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let directory = directory_of(path);
    std::fs::create_dir_all(directory).map_err(|source| Failure {
        path: directory.to_path_buf(),
        source,
    })?;
    let staged = directory.join(staging(path));
    std::fs::write(&staged, bytes).map_err(|source| Failure {
        path: staged.clone(),
        source,
    })?;
    if let Err(source) = std::fs::rename(&staged, path) {
        let source = match std::fs::remove_file(&staged) {
            Ok(()) => source,
            Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => source,
            Err(cleanup) => std::io::Error::other(RenameCleanupError {
                rename: source,
                cleanup,
            }),
        };
        return Err(Failure {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// The directory a path is in, which for a bare name is the one the process is standing in.
#[must_use]
pub fn directory_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// The name the bytes are staged under: beside the destination, so the rename stays on the filesystem the store is on, and named after the writer, so no two writers stage at one path.
#[must_use]
pub fn staging(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("entry");
    let writer: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    format!(".{name}.{}.{writer}.writing", std::process::id())
}
