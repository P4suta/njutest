// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A `?` in statement position whose deletion no test notices, so the survivor gains its evidence only from the call failing beside it.

use std::path::Path;

/// Leaves a note at `path` for whoever reads it next, and nothing checks that it landed.
///
/// # Errors
/// Whatever writing it said.
pub fn leave(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, "left")?;
    Ok(())
}
