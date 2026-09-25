// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Taking directories back, within a budget, saying what is still there.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long taking things back may take before the rest is left for next time.
///
/// Removing a directory is usually instant and occasionally is not: one a wedged device still holds takes minutes to refuse, and a loop over a few hundred of those runs for a day.
/// Every reclamation here is housekeeping done on the way to the work or on the way out of it, so it is the one thing that has to be faster than what it is cleaning up after.
pub const BUDGET: Duration = Duration::from_secs(10);

/// What a reclamation did, and what it did not.
///
/// The second half is the part that was being dropped everywhere: a directory that refused and a directory that was never reached both stay on the disk,
/// and a caller that is handed only a count of successes has nothing to put in a ledger and nothing to tell a person.
#[derive(Debug, Default)]
pub struct Reclaimed {
    /// Every directory that went.
    pub removed: Vec<PathBuf>,
    /// Every one that refused, with what it said.
    pub refused: Vec<(PathBuf, String)>,
    /// Every one the budget ran out before reaching.
    pub unreached: Vec<PathBuf>,
}

impl Reclaimed {
    /// Whether anything is still there, which is what a caller has to report or record.
    #[must_use]
    pub fn left(&self) -> Vec<&PathBuf> {
        self.refused
            .iter()
            .map(|(path, _why)| path)
            .chain(self.unreached.iter())
            .collect()
    }
}

/// Removes each of `directories`, stopping at [`BUDGET`] and naming what is left.
#[must_use]
pub fn all<'a>(directories: impl IntoIterator<Item = &'a Path>) -> Reclaimed {
    with(directories, &|path: &Path| {
        crate::tempowner::remove_tree(path)
    })
}

/// [`all`] with its removal as an argument, so a directory that refuses can be tested without a filesystem persuaded into refusing.
#[must_use]
pub fn with<'a>(
    directories: impl IntoIterator<Item = &'a Path>,
    remove: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Reclaimed {
    let started = Instant::now();
    let mut done = Reclaimed::default();
    for path in directories {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_irregular) => {
                done.refused.push((
                    path.to_path_buf(),
                    String::from("refusing to reclaim a non-directory or symbolic link"),
                ));
                continue;
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                done.refused.push((path.to_path_buf(), source.to_string()));
                continue;
            }
        }
        if started.elapsed() >= BUDGET {
            done.unreached.push(path.to_path_buf());
            continue;
        }
        match remove(path) {
            Ok(()) => done.removed.push(path.to_path_buf()),
            Err(why) => done.refused.push((path.to_path_buf(), why.to_string())),
        }
    }
    done
}
