// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Replacing a file so that whoever reads it next reads one whole version of it.
//!
//! Three stores under `.mjutest` answer a later run: the checkpoint an
//! interrupted run left, the reports a cache keeps, and what earlier runs
//! established about individual mutants. Every one of them is written by one
//! run and believed by another, and the two are not ordered — a run is
//! interrupted mid-write, a shard writes while a sibling shard reads, an editor
//! loop verifies while a pipeline does.
//!
//! Bytes that land in the destination itself arrive over the old ones, and a
//! reader that arrives between the truncation and the last byte holds a file
//! that is neither answer. That reader either refuses it, which throws away an
//! answer nobody contradicted, or reads a prefix that happens to parse, which
//! is the same store saying two things. So the bytes are staged beside the
//! destination and arrive by a rename, which is one step: a reader holds what
//! was there or what replaced it, and never the seam between them.
//!
//! Saying it here rather than in each of the three is the point. Two of them
//! said it already and the third did not, which is what a rule written more
//! than once looks like from the outside.

use std::path::{Path, PathBuf};

/// The path a replacement could not be made through, and what the filesystem said.
///
/// This is not an error a person meets: each store names the path in an error
/// of its own, because what a reader can do about it depends on which store it
/// was.
#[derive(Debug)]
#[non_exhaustive]
pub struct Failure {
    /// The path that refused: the destination, or the directory it goes in.
    pub path: PathBuf,
    /// What the filesystem said.
    pub source: std::io::Error,
}

/// Writes `bytes` where `path` is, so that a reader holds the whole of what was there or the whole of this.
///
/// The directory is made if it is not there: a store takes its shape as it
/// fills, and the run that fills it is the one that knows the shape.
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
        drop(std::fs::remove_file(&staged));
        return Err(Failure {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// The directory a path is in, which for a bare name is the one the process is standing in.
fn directory_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// The name the bytes are staged under: beside the destination, so the rename stays on the filesystem the store is on, and named after the writer, so no two writers stage at one path.
///
/// A writer is a thread of a process, and the staged file exists only between
/// the write and the rename, so two replacements that could overlap are two
/// that carry different names here and two that carry the same name cannot
/// overlap.
fn staging(path: &Path) -> String {
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
