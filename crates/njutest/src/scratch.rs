// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One directory per run, and everything the run writes below it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::id::RunId;
#[cfg(feature = "testkit")]
use rust_mutants::tempowner::SweepResult;
use rust_mutants::tempowner::{self, ClaimError, Owner};

use crate::error::{self, ErrorCode};

/// The prefix of every run scratch, and the one a sweep knows.
pub const DIR_PREFIX: &str = "njutest-run-";

/// The marker schema of a directory this program made.
pub const MARKER_SCHEMA: &str = "njutest-temp-owner-v1";

/// The per-run build directory: what a suite's own Cargo commands write into, isolated from the engine's persistent target directory.
pub const BUILD_DIR_NAME: &str = "build";

/// Where instrumented test processes write their coverage profiles.
pub const PROFILES_DIR_NAME: &str = "profiles";

/// Where preserved command output goes.
pub const OUTPUT_DIR_NAME: &str = "output";

/// Where the crate planted for the engine's routing layers is written.
pub const SENTINEL_DIR_NAME: &str = "sentinel";

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
    /// The directory could not be claimed or marked as deliberately kept.
    #[error("{}: owning {path}: {source}", error::SCRATCH_UNUSABLE.code)]
    Ownership {
        /// The scratch directory.
        path: PathBuf,
        /// The ownership protocol failure.
        #[source]
        source: ClaimError,
    },
}

impl ScratchError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unusable { .. } | Self::Ownership { .. } => error::SCRATCH_UNUSABLE,
        }
    }
}

/// The directory one run owns, and everything it writes below it.
#[derive(Debug)]
pub struct Scratch {
    dir: PathBuf,
    owner: Option<Owner>,
    #[cfg(feature = "testkit")]
    swept: SweepResult,
    disarmed: bool,
}

impl Scratch {
    /// Sweeps `parent` of what earlier runs left, makes this run's directory below it, and claims it.
    ///
    /// # Errors
    /// [`ScratchError::Unusable`] when a directory cannot be made.
    /// Failing to claim one is not an error: see [`Scratch::is_claimed`].
    pub fn create(parent: &Path, run_id: &RunId, now: Timestamp) -> Result<Self, ScratchError> {
        let mut swept =
            tempowner::sweep(parent, &[DIR_PREFIX]).map_err(|source| ScratchError::Unusable {
                path: parent.to_path_buf(),
                source,
            })?;
        if let Some(failure) = swept.failures.pop() {
            return Err(ScratchError::Unusable {
                path: failure.dir,
                source: failure.source,
            });
        }
        let dir = parent.join(format!("{DIR_PREFIX}{run_id}"));
        for path in [
            dir.join(BUILD_DIR_NAME),
            dir.join(PROFILES_DIR_NAME),
            dir.join(OUTPUT_DIR_NAME),
        ] {
            fs::create_dir_all(&path).map_err(|source| ScratchError::Unusable { path, source })?;
        }
        let build = dir.join(BUILD_DIR_NAME);
        tempowner::tag_cache(&build).map_err(|source| ScratchError::Unusable {
            path: build,
            source,
        })?;
        let owner = match tempowner::claim_as(&dir, now, MARKER_SCHEMA) {
            Ok(owner) => Some(owner),
            Err(ClaimError::Owned { .. }) => None,
            Err(source @ (ClaimError::Lock { .. } | ClaimError::Marker { .. })) => {
                return Err(ScratchError::Ownership {
                    path: dir.clone(),
                    source,
                });
            }
        };
        let scratch = Self {
            dir,
            owner,
            #[cfg(feature = "testkit")]
            swept,
            disarmed: false,
        };
        #[cfg(not(feature = "testkit"))]
        drop(swept);
        Ok(scratch)
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

    /// Where the crate planted for the engine's routing layers is written.
    #[must_use]
    pub fn sentinel_dir(&self) -> PathBuf {
        self.dir.join(SENTINEL_DIR_NAME)
    }

    /// Where coverage profiles are written.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn profiles_dir(&self) -> PathBuf {
        self.dir.join(PROFILES_DIR_NAME)
    }

    /// Where preserved command output goes.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn output_dir(&self) -> PathBuf {
        self.dir.join(OUTPUT_DIR_NAME)
    }

    /// A directory for one round of work, made if it is not there yet.
    /// Asking twice asks for the same place.
    ///
    /// # Errors
    /// [`ScratchError::Unusable`] when it cannot be made.
    #[cfg(feature = "testkit")]
    pub fn round_dir(&self, name: &str) -> Result<PathBuf, ScratchError> {
        let path = self.dir.join(name);
        fs::create_dir_all(&path).map_err(|source| ScratchError::Unusable {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Whether this run holds the directory's lock.
    /// A run that does not still works here, and says so with [`crate::limitation::TEMP_DIRECTORY_UNCLAIMED`].
    #[must_use]
    pub const fn is_claimed(&self) -> bool {
        self.owner.is_some()
    }

    /// What the sweep on the way in collected.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn swept(&self) -> &SweepResult {
        &self.swept
    }

    /// Releases the lock and removes everything the run wrote, answering with whatever would not go.
    ///
    /// Answering "nothing was preserved" without looking leaves a directory on the disk that nothing names: not the ledger, which was given an empty list, and not the person, who was told the run cleaned up after itself.
    /// A tree something else is holding — a mount, an open handle — refuses,
    /// and that is the one case worth reporting.
    /// # Errors
    /// Returns the exact unlock, removal, or inspection failure.
    pub fn close(mut self) -> Result<Vec<PathBuf>, ScratchError> {
        let removed = self.remove();
        self.disarmed = true;
        removed?;
        match fs::symlink_metadata(&self.dir) {
            Ok(_replacement) => Ok(vec![self.dir.clone()]),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(source) => Err(ScratchError::Unusable {
                path: self.dir.clone(),
                source,
            }),
        }
    }

    /// Records the keep in the marker, releases the lock, and leaves everything where it is, answering with what was preserved.
    /// # Errors
    /// Returns the exact marker or unlock failure.
    pub fn keep(mut self) -> Result<Vec<PathBuf>, ScratchError> {
        let kept = match self.owner.as_mut() {
            Some(owner) => owner.keep().map_err(|source| ScratchError::Ownership {
                path: self.dir.clone(),
                source,
            }),
            None => Ok(()),
        };
        self.disarmed = true;
        kept?;
        Ok(vec![self.dir.clone()])
    }

    /// Releases the lock, then removes the tree.
    /// The lock goes first everywhere: on Windows an open handle inside a directory is what makes the removal fail.
    fn remove(&mut self) -> Result<(), ScratchError> {
        if let Some(owner) = self.owner.as_mut() {
            owner.release().map_err(|source| ScratchError::Unusable {
                path: tempowner::lock_path(&self.dir),
                source,
            })?;
        }
        match fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(ScratchError::Unusable {
                path: self.dir.clone(),
                source,
            }),
        }
    }
}

impl Drop for Scratch {
    /// Best effort, so an early return leaves nothing behind; [`Scratch::close`] and [`Scratch::keep`] are the authority.
    fn drop(&mut self) {
        if !self.disarmed && self.remove().is_err() {
            std::process::abort();
        }
    }
}
