// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A count kept on disk, written two ways: in pieces, which a stop between them tears, and whole into place, which no stop can.

use std::io::Write;
use std::path::Path;

/// The count kept at `path`, or nothing yet where there is no file.
///
/// # Errors
/// Whatever reading it said, or that the file holds no count.
pub fn load(path: &Path) -> std::io::Result<u32> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let unreadable = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not a count");
    let Some(count) = text.strip_prefix("count=") else {
        return Err(unreadable());
    };
    count.parse::<u32>().map_err(|_not_a_number| unreadable())
}

/// Keeps `count` at `path` a piece at a time.
///
/// # Errors
/// Whatever writing it said.
pub fn save_in_pieces(path: &Path, count: u32) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"count=")?;
    file.write_all(count.to_string().as_bytes())?;
    Ok(())
}

/// Keeps `count` at `path` by writing it beside and moving it into place.
///
/// # Errors
/// Whatever writing or moving it said.
pub fn save_whole(path: &Path, count: u32) -> std::io::Result<()> {
    let staged = path.with_extension("staged");
    std::fs::write(&staged, format!("count={count}"))?;
    std::fs::rename(&staged, path)?;
    Ok(())
}
