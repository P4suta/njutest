// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One directory per run, and everything the run writes below it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::tempowner::{self, Owner, SweepResult};

use crate::error::{self, ErrorCode};

/// The prefix of every run scratch, and the one a sweep knows.
pub const DIR_PREFIX: &str = "mjutest-run-";

/// The marker schema of a directory this program made.
pub const MARKER_SCHEMA: &str = "mjutest-temp-owner-v1";

/// The per-run build directory: what a suite's own Cargo commands write into, isolated from the engine's persistent target directory.
pub const BUILD_DIR_NAME: &str = "build";

/// Where instrumented test processes write their coverage profiles.
pub const PROFILES_DIR_NAME: &str = "profiles";

/// Where preserved command output goes.
pub const OUTPUT_DIR_NAME: &str = "output";

/// Why a run has nowhere to work.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ScratchError {
    /// The directory could not be made.
    #[error("{}: the run has nowhere to work: creating {path}: {source}", error::SCRATCH_UNUSABLE.code)]
    Unusable {
        /// The directory.
        path: PathBuf,
        /// The failure.
        #[source]
        source: io::Error,
    },
}

impl ScratchError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unusable { .. } => error::SCRATCH_UNUSABLE,
        }
    }
}

/// The directory one run owns, and everything it writes below it.
#[derive(Debug)]
pub struct Scratch {
    dir: PathBuf,
    owner: Option<Owner>,
    swept: SweepResult,
    disarmed: bool,
}

impl Scratch {
    /// Sweeps `parent` of what earlier runs left, makes this run's directory below it, and claims it.
    ///
    /// # Errors
    /// [`ScratchError::Unusable`] when a directory cannot be made. Failing
    /// to claim one is not an error: see [`Scratch::is_claimed`].
    pub fn create(parent: &Path, run_id: &str, now: Timestamp) -> Result<Self, ScratchError> {
        let swept = tempowner::sweep(parent, &[DIR_PREFIX], now).unwrap_or_default();
        let dir = parent.join(format!("{DIR_PREFIX}{run_id}"));
        for path in [
            dir.join(BUILD_DIR_NAME),
            dir.join(PROFILES_DIR_NAME),
            dir.join(OUTPUT_DIR_NAME),
        ] {
            fs::create_dir_all(&path).map_err(|source| ScratchError::Unusable { path, source })?;
        }
        let owner = tempowner::claim_as(&dir, now, MARKER_SCHEMA).ok();
        Ok(Self {
            dir,
            owner,
            swept,
            disarmed: false,
        })
    }

    /// The directory this run owns.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The per-run build directory used for Cargo isolation.
    #[must_use]
    pub fn build_dir(&self) -> PathBuf {
        self.dir.join(BUILD_DIR_NAME)
    }

    /// Where coverage profiles are written.
    #[must_use]
    pub fn profiles_dir(&self) -> PathBuf {
        self.dir.join(PROFILES_DIR_NAME)
    }

    /// Where preserved command output goes.
    #[must_use]
    pub fn output_dir(&self) -> PathBuf {
        self.dir.join(OUTPUT_DIR_NAME)
    }

    /// A directory for one round of work, made if it is not there yet. Asking twice asks for the same place.
    ///
    /// # Errors
    /// [`ScratchError::Unusable`] when it cannot be made.
    pub fn round_dir(&self, name: &str) -> Result<PathBuf, ScratchError> {
        let path = self.dir.join(name);
        fs::create_dir_all(&path).map_err(|source| ScratchError::Unusable {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Whether this run holds the directory's lock. A run that does not still works here, and says so with [`crate::limitation::TEMP_DIRECTORY_UNCLAIMED`].
    #[must_use]
    pub const fn is_claimed(&self) -> bool {
        self.owner.is_some()
    }

    /// What the sweep on the way in collected.
    #[must_use]
    pub const fn swept(&self) -> &SweepResult {
        &self.swept
    }

    /// Releases the lock and removes everything the run wrote, answering with what was preserved: nothing.
    #[must_use]
    pub fn close(mut self) -> Vec<PathBuf> {
        self.disarmed = true;
        self.remove();
        Vec::new()
    }

    /// Records the keep in the marker, releases the lock, and leaves everything where it is, answering with what was preserved.
    #[must_use]
    pub fn keep(mut self) -> Vec<PathBuf> {
        self.disarmed = true;
        if let Some(owner) = self.owner.as_mut() {
            drop(owner.keep());
        }
        vec![self.dir.clone()]
    }

    /// Releases the lock, then removes the tree. The lock goes first everywhere: on Windows an open handle inside a directory is what makes the removal fail.
    fn remove(&mut self) {
        if let Some(owner) = self.owner.as_mut() {
            drop(owner.release());
        }
        drop(fs::remove_dir_all(&self.dir));
    }
}

impl Drop for Scratch {
    /// Best effort, so an early return leaves nothing behind; [`Scratch::close`] and [`Scratch::keep`] are the authority.
    fn drop(&mut self) {
        if !self.disarmed {
            self.remove();
        }
    }
}
