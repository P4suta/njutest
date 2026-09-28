// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a file a piece of evidence names, confined to the directory the evidence belongs to and bounded.

use std::io::Read as _;
use std::path::{Component, Path};

/// Reads the regular file at `relative` under `root` whole, refusing a path that is not plain names, crosses a link or reparse point, or names more than `limit` bytes.
///
/// # Errors
/// The file could not be opened inside `root`, is not a regular file, is larger than `limit`, or changed size while it was read.
pub fn read(root: &Path, relative: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut file = open(root, relative)?;
    let before = file.metadata()?;
    if !before.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "it is not a regular file",
        ));
    }
    if before.len() > limit {
        return Err(larger(limit));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit.checked_add(1).ok_or_else(|| larger(limit))?)
        .read_to_end(&mut bytes)?;
    let read = u64::try_from(bytes.len()).map_err(|_beyond_any_file| larger(limit))?;
    if read > limit {
        return Err(larger(limit));
    }
    if read != before.len() || file.metadata()?.len() != before.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "it changed size while it was read",
        ));
    }
    Ok(bytes)
}

/// The refusal of a file larger than `limit`.
fn larger(limit: u64) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("it is larger than {limit} bytes, more than whoever wrote it keeps whole"),
    )
}

/// Opens the entry at `relative` under `root` without following a link at any step, refusing a path that is not plain names.
///
/// # Errors
/// A component that is not a plain name, a link or reparse point on the way, or an entry that cannot be opened.
#[cfg(unix)]
pub fn open(root: &Path, relative: &Path) -> std::io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags};

    let mut directory = rustix::fs::open(
        root,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the path is not canonical",
            ));
        };
        if components.peek().is_some() {
            directory = rustix::fs::openat(
                &directory,
                name,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                Mode::empty(),
            )?;
            continue;
        }
        let descriptor = rustix::fs::openat(
            &directory,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        return Ok(std::fs::File::from(descriptor));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "the path is empty",
    ))
}

/// Opens the entry at `relative` under `root` without following a reparse point at any step, refusing a path that is not plain names.
///
/// # Errors
/// A component that is not a plain name, a reparse point or a non-file entry on the way, or an entry that cannot be opened.
#[cfg(windows)]
pub fn open(root: &Path, relative: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    let mut path = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the path is not canonical",
            ));
        };
        path.push(name);
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (components.peek().is_some() && !metadata.file_type().is_dir())
            || (components.peek().is_none() && !metadata.file_type().is_file())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the path crosses a reparse point or non-file entry",
            ));
        }
    }
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Opens the entry at `relative` under `root`, refusing a path that is not plain names or crosses a link.
///
/// # Errors
/// A component that is not a plain name, a link on the way, or an entry that cannot be opened.
#[cfg(not(any(unix, windows)))]
pub fn open(root: &Path, relative: &Path) -> std::io::Result<std::fs::File> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the path is not canonical",
            ));
        };
        path.push(name);
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the path crosses a symlink",
            ));
        }
    }
    std::fs::File::open(path)
}
