// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Waiting for the run that is already doing this work, rather than doing it twice.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rust_mutants::runner::Cancel;
use rust_mutants::tempowner::{self, Lock};

/// How long between attempts while waiting for the owner to finish.
pub const POLL: Duration = Duration::from_millis(50);

/// An exclusive claim on one cache entry, held while a run establishes it.
#[derive(Debug)]
pub struct Lease {
    path: PathBuf,
    lock: Option<Lock>,
}

impl Lease {
    /// Where the claim is recorded.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Releases the claim. Idempotent, and never removes anything.
    ///
    /// # Errors
    /// The lock could not be released.
    pub fn release(&mut self) -> std::io::Result<()> {
        self.lock.take().map_or(Ok(()), |mut lock| lock.release())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        drop(self.release());
    }
}

/// Why a claim could not be made.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LeaseError {
    /// The lock file could not be opened or locked.
    #[error("claiming {}: {source}", path.display())]
    Unusable {
        /// The lock file.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// The wait was cancelled.
    #[error("waiting for {} was interrupted", path.display())]
    Interrupted {
        /// The lock file.
        path: PathBuf,
    },
    /// The owner did not finish in the time allowed.
    #[error("{} is still owned after {} seconds", path.display(), waited.as_secs())]
    TimedOut {
        /// The lock file.
        path: PathBuf,
        /// How long was spent waiting.
        waited: Duration,
    },
}

/// Takes the claim on `path` without waiting. `Ok(None)` means somebody else has it.
///
/// # Errors
/// See [`LeaseError::Unusable`].
pub fn try_claim(path: &Path) -> Result<Option<Lease>, LeaseError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| LeaseError::Unusable {
            path: path.to_path_buf(),
            source,
        })?;
    }
    let lock = tempowner::acquire(path).map_err(|source| LeaseError::Unusable {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(lock.map(|lock| Lease {
        path: path.to_path_buf(),
        lock: Some(lock),
    }))
}

/// Takes the claim on `path`, waiting for whoever has it. `waiting` is called once, the first time the claim is contended, so a command line can say what it is waiting for.
///
/// # Errors
/// See [`LeaseError`].
pub fn claim(
    path: &Path,
    within: Duration,
    cancel: &Cancel,
    waiting: &mut dyn FnMut(),
) -> Result<Lease, LeaseError> {
    let started = Instant::now();
    let mut said = false;
    loop {
        if let Some(lease) = try_claim(path)? {
            return Ok(lease);
        }
        if cancel.is_cancelled() {
            return Err(LeaseError::Interrupted {
                path: path.to_path_buf(),
            });
        }
        if started.elapsed() >= within {
            return Err(LeaseError::TimedOut {
                path: path.to_path_buf(),
                waited: started.elapsed(),
            });
        }
        if !said {
            said = true;
            waiting();
        }
        std::thread::sleep(POLL);
    }
}
