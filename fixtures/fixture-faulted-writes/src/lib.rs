// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A call whose failure a test answers by writing into the tree it is measured in.

use std::path::Path;

/// The text of the file at `path`.
///
/// # Errors
/// Whatever reading it said.
pub fn load(path: &Path) -> std::io::Result<String> {
    let text = std::fs::read_to_string(path)?;
    Ok(text)
}
