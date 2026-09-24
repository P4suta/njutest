// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The processes of an instrumented tree that ran without the environment the run gave them, as they said so in the watched directory.

use std::path::Path;
use std::str::FromStr as _;

use crate::instrument::ORPHAN_PREFIX;

/// One process that lost the run's environment: its own id, its parent's, and when it said so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Orphan {
    /// Its process id.
    pub pid: u32,
    /// Its parent's, or zero where the platform does not say.
    pub parent: u32,
    /// When it said so, or nothing where the filesystem could not say, which could be any time at all.
    pub at: Option<std::time::SystemTime>,
}

/// How far either side of an execution an orphan's time still counts as during it: a filesystem's clock is coarser than a process's.
pub const SLACK: std::time::Duration = std::time::Duration::from_secs(2);

impl Orphan {
    /// Whether this orphan may have been left while something ran from `started` to `ended`.
    #[must_use]
    pub fn during(&self, started: std::time::SystemTime, ended: std::time::SystemTime) -> bool {
        let Some(at) = self.at else {
            return true;
        };
        let from = started.checked_sub(SLACK).unwrap_or(std::time::UNIX_EPOCH);
        let to = ended.checked_add(SLACK).unwrap_or(ended);
        from <= at && at <= to
    }
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
        let entry = entry?;
        let at = match entry.metadata().and_then(|meta| meta.modified()) {
            Ok(at) => Some(at),
            Err(_unknown) => None,
        };
        let name = entry.file_name();
        let ids = name
            .to_str()
            .and_then(|name| name.strip_prefix(ORPHAN_PREFIX))
            .and_then(|ids| ids.split_once('-'));
        orphans.push(
            match ids.map(|(pid, parent)| (u32::from_str(pid), u32::from_str(parent))) {
                Some((Ok(pid), Ok(parent))) => Orphan { pid, parent, at },
                Some(_) | None => Orphan {
                    pid: 0,
                    parent: 0,
                    at,
                },
            },
        );
    }
    orphans.sort_unstable();
    Ok(orphans)
}

/// Whether `orphan` was left by the execution `leader` led, rather than by another running beside it.
///
/// A child names its parent, and a child the execution's own process started names that process; a parent that another execution led, or that is still running, belongs to someone else.
/// Anything else — a parent the platform does not name, or one that has already gone — cannot be told apart and counts as this execution's, so a survival is never read past a child that may have been its own.
#[must_use]
pub fn ours(
    orphan: &Orphan,
    leader: Option<u32>,
    others: &std::collections::BTreeSet<u32>,
) -> bool {
    if orphan.parent == 0 || leader == Some(orphan.parent) {
        return true;
    }
    !(others.contains(&orphan.parent) || running(orphan.parent))
}

/// Whether the process `pid` is still running.
#[cfg(unix)]
fn running(pid: u32) -> bool {
    match i32::try_from(pid).map(rustix::process::Pid::from_raw) {
        Ok(Some(pid)) => rustix::process::test_kill_process(pid).is_ok(),
        Ok(None) | Err(_) => false,
    }
}

/// Whether the process `pid` is still running, which this platform is not asked.
#[cfg(not(unix))]
const fn running(_pid: u32) -> bool {
    false
}
