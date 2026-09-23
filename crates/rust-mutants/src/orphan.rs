// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The processes of an instrumented tree that ran without the environment the run gave them, as they said so in the watched directory.

use std::path::Path;
use std::str::FromStr as _;

use crate::instrument::ORPHAN_PREFIX;

/// One process that lost the run's environment: its own id and its parent's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Orphan {
    /// Its process id.
    pub pid: u32,
    /// Its parent's, or zero where the platform does not say.
    pub parent: u32,
}

/// Forgets every orphan `watched` holds, so what it holds next was left by what runs next.
///
/// # Errors
/// What the filesystem said, less the one answer that means nothing was ever left.
pub fn clear(watched: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(watched) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Every orphan `watched` holds; a name that is not one an orphan leaves is counted as one, since something wrote it where only orphans do.
///
/// # Errors
/// What the filesystem said, less the one answer that means nothing was ever left.
pub fn left(watched: &Path) -> std::io::Result<Vec<Orphan>> {
    let listing = match std::fs::read_dir(watched) {
        Ok(listing) => listing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut orphans = Vec::new();
    for entry in listing {
        let name = entry?.file_name();
        let ids = name
            .to_str()
            .and_then(|name| name.strip_prefix(ORPHAN_PREFIX))
            .and_then(|ids| ids.split_once('-'));
        orphans.push(
            match ids.map(|(pid, parent)| (u32::from_str(pid), u32::from_str(parent))) {
                Some((Ok(pid), Ok(parent))) => Orphan { pid, parent },
                Some(_) | None => Orphan { pid: 0, parent: 0 },
            },
        );
    }
    orphans.sort_unstable();
    Ok(orphans)
}
