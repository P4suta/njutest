// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Scheduling state for continuing an interrupted verification.
//!
//! A checkpoint is never assurance evidence, never a partial report, and never
//! updates an index. One exact input identity owns one checkpoint, and there is
//! no resume flag: only a checkpoint under the newly computed, identical
//! identity is considered.
//!
//! A saved baseline target carries the files it reached and not the coverage
//! regions inside them, so it is routed at file granularity for the rest of the
//! run. A resumed run therefore executes at least the work a cold run would,
//! never less.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::coverage::{Block, Point};
use crate::report::TargetStatus;

/// The name of the shape.
pub const SCHEMA: &str = "mjutest-assurance-checkpoint-v1";

/// The file one identity's checkpoint is written to.
pub const FILE_NAME: &str = "checkpoint-v1.json";

/// Scheduling state for one interrupted run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// [`SCHEMA`].
    pub schema: String,
    /// The identity of the inputs this state is about.
    pub identity: String,
    /// How many runs have written to it, this one included.
    pub attempts: u32,
    /// The baseline targets already measured.
    pub targets: Vec<SavedTarget>,
    /// The mutants already judged.
    pub mutants: Vec<SavedMutant>,
}

impl State {
    /// Fresh state for `identity`.
    #[must_use]
    pub fn new(identity: &str) -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            identity: identity.to_owned(),
            attempts: 0,
            targets: Vec::new(),
            mutants: Vec::new(),
        }
    }

    /// Whether there is anything to continue from.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.targets.is_empty() && self.mutants.is_empty()
    }

    /// The saved target with this identity, if any.
    #[must_use]
    pub fn target(&self, id: &str) -> Option<&SavedTarget> {
        self.targets.iter().find(|target| target.id == id)
    }

    /// The saved mutant with this identity, if any.
    #[must_use]
    pub fn mutant(&self, id: &str) -> Option<&SavedMutant> {
        self.mutants.iter().find(|mutant| mutant.id == id)
    }

    /// Records one measured target, replacing what was there.
    pub fn record_target(&mut self, target: SavedTarget) {
        self.targets.retain(|saved| saved.id != target.id);
        self.targets.push(target);
        self.targets.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Records one judged mutant, replacing what was there. A disposition outside [`SAVEABLE`] is not recorded: it is not a claim the next run can inherit.
    pub fn record_mutant(&mut self, mutant: SavedMutant) {
        if !SAVEABLE.contains(&mutant.disposition.as_str()) {
            return;
        }
        self.mutants.retain(|saved| saved.id != mutant.id);
        self.mutants.push(mutant);
        self.mutants.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

/// One baseline target an interrupted run had already measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedTarget {
    /// The target's stable identity.
    pub id: String,
    /// Its terminal state.
    pub status: TargetStatus,
    /// How long it took.
    pub duration_ms: u64,
    /// What it said, when that matters.
    pub message: Option<String>,
    /// The files it reached, and not the regions inside them. A restored target keeps reaching its whole file.
    pub files: Vec<String>,
}

impl SavedTarget {
    /// The coverage a restored target contributes: one block spanning each file it reached, so it is a candidate for every position in that file and narrows none of them.
    #[must_use]
    pub fn coverage(&self) -> BTreeSet<Block> {
        self.files
            .iter()
            .map(|file| Block {
                file: PathBuf::from(file),
                start: Point { line: 0, column: 0 },
                end: Point {
                    line: u32::MAX,
                    column: u32::MAX,
                },
            })
            .collect()
    }
}

/// The dispositions a checkpoint may carry.
///
/// A kill and a confirmed timeout are existential claims: one named test
/// noticed the mutant on this exact tree, and that stays true however the next
/// run routes. Every other disposition depends on which tests the run decided
/// could notice, and a resumed run routes at file granularity, so it re-derives
/// them rather than inheriting a claim it did not make.
pub const SAVEABLE: [&str; 2] = ["killed", "timed_out"];

/// One mutant an interrupted run had already judged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedMutant {
    /// The mutant's full identity.
    pub id: String,
    /// What became of it, as the report spells it. One of [`SAVEABLE`].
    pub disposition: String,
    /// The target that noticed it.
    pub killed_by: Option<String>,
    /// How long it took.
    pub duration_ms: u64,
}

/// Why a checkpoint could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CheckpointError {
    /// The file could not be read or written.
    #[error("{}: {}: {source}", crate::error::CACHE_UNUSABLE.code, path.display())]
    Unusable {
        /// The file.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// The file is there and is not the state it claims to be.
    #[error("{}: {}: {message}", crate::error::CACHE_CORRUPT.code, path.display())]
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
}

impl CheckpointError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::Unusable { .. } => crate::error::CACHE_UNUSABLE,
            Self::Corrupt { .. } => crate::error::CACHE_CORRUPT,
        }
    }
}

/// Where one identity's checkpoint lives under `root`.
#[must_use]
pub fn path_of(root: &Path, identity: &str) -> PathBuf {
    root.join(identity).join(FILE_NAME)
}

/// The state an interrupted run left for `identity`, if any.
///
/// # Errors
/// [`CheckpointError::Corrupt`] when a file is there and is not the state it
/// claims to be: one that does not parse, or one filed under a different
/// identity. Continuing from a checkpoint that is about different inputs would
/// make a run claim what it never established.
pub fn read(root: &Path, identity: &str) -> Result<Option<State>, CheckpointError> {
    let path = path_of(root, identity);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(CheckpointError::Unusable { path, source }),
    };
    let state: State = serde_json::from_str(&text).map_err(|error| CheckpointError::Corrupt {
        path: path.clone(),
        message: error.to_string(),
    })?;
    if state.schema != SCHEMA {
        return Err(CheckpointError::Corrupt {
            path,
            message: format!("says it is {:?} and not {SCHEMA:?}", state.schema),
        });
    }
    if state.identity != identity {
        return Err(CheckpointError::Corrupt {
            path,
            message: format!("is about {} and was read for {identity}", state.identity),
        });
    }
    if let Some(other) = state
        .mutants
        .iter()
        .find(|mutant| !SAVEABLE.contains(&mutant.disposition.as_str()))
    {
        return Err(CheckpointError::Corrupt {
            path,
            message: format!(
                "carries {} for {}, which is a disposition a resumed run re-derives rather \
                 than inherits",
                other.disposition, other.id
            ),
        });
    }
    Ok(Some(state))
}

/// Writes `state` where [`read`] will find it, replacing what was there.
///
/// # Errors
/// See [`CheckpointError::Unusable`].
pub fn write(root: &Path, state: &State) -> Result<PathBuf, CheckpointError> {
    let path = path_of(root, &state.identity);
    let text = serde_json::to_string(state).map_err(|error| CheckpointError::Unusable {
        path: path.clone(),
        source: std::io::Error::other(error),
    })?;
    rust_mutants::replace::file(&path, text.as_bytes()).map_err(|failure| {
        CheckpointError::Unusable {
            path: failure.path,
            source: failure.source,
        }
    })?;
    Ok(path)
}

/// Removes the checkpoint for `identity`. A run that finished has nothing to continue from.
pub fn clear(root: &Path, identity: &str) {
    let path = path_of(root, identity);
    drop(std::fs::remove_file(&path));
    if let Some(directory) = path.parent() {
        drop(std::fs::remove_dir(directory));
    }
}
