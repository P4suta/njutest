// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Scheduling state for continuing an interrupted verification.

#[cfg(feature = "testkit")]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "testkit")]
use crate::coverage::{Block, Point};
use crate::report::TargetStatus;

/// The name of the shape.
pub const SCHEMA: &str = "njutest-assurance-checkpoint-v2";

/// The file one identity's checkpoint is written to.
pub const FILE_NAME: &str = "checkpoint-v2.json";

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
    #[cfg(feature = "testkit")]
    pub fn target(&self, id: &str) -> Option<&SavedTarget> {
        self.targets.iter().find(|target| target.id == id)
    }

    /// The saved mutant with this identity, if any.
    #[must_use]
    pub fn mutant(&self, id: &str) -> Option<&SavedMutant> {
        self.mutants.iter().find(|mutant| mutant.id == id)
    }

    /// Records one measured target, replacing what was there.
    #[cfg(feature = "testkit")]
    pub fn record_target(&mut self, target: SavedTarget) {
        self.targets.retain(|saved| saved.id != target.id);
        self.targets.push(target);
        self.targets.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Records one judged mutant, replacing what was there.
    ///
    /// [`SavedDisposition`] has no inconclusive arm, so a caller cannot put a
    /// timeout or finite step boundary into continuation evidence.
    pub fn record_mutant(&mut self, mutant: SavedMutant) {
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
    #[cfg(feature = "testkit")]
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

/// The only mutation fact a current checkpoint may carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SavedDisposition {
    /// A named test noticed this exact mutation.
    Killed {
        /// The target that noticed it.
        by: String,
    },
}

/// One mutant an interrupted run had already judged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedMutant {
    /// The mutant's full identity.
    pub id: String,
    /// The closed fact a successor may inherit.
    pub disposition: SavedDisposition,
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
    #[error("{}: {}: {violation}", crate::error::CACHE_CORRUPT.code, path.display())]
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        violation: CheckpointViolation,
    },
}

/// A contradiction inside an untrusted checkpoint document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckpointViolation {
    /// The bytes are not the closed JSON shape for [`State`].
    #[error("the document is not a checkpoint: {detail}")]
    Malformed {
        /// The parser's diagnostic.
        detail: String,
    },
    /// A filesystem key is not a canonical full digest.
    #[error("checkpoint identity {value:?} is not 64 lowercase hexadecimal characters")]
    InvalidIdentity {
        /// The untrusted value, before it reaches a path join.
        value: String,
    },
    /// The document claims another schema identity.
    #[error("says it is {found:?} and not {SCHEMA:?}")]
    WrongSchema {
        /// The claimed schema.
        found: String,
    },
    /// The document is filed under a different input identity.
    #[error("is about {found} and was read for {expected}")]
    WrongIdentity {
        /// The identity inside the document.
        found: String,
        /// The identity selected by the caller.
        expected: String,
    },
    /// A stored file cannot describe zero attempts: no attempt would have
    /// written it.
    #[error("records zero attempts")]
    NoAttempts,
    /// A target cannot be referred to by an empty identity.
    #[error("a saved target has an empty identity")]
    EmptyTarget,
    /// Target identities are not in the one canonical, duplicate-free order.
    #[error("target ids are not strictly increasing: {previous:?}, then {current:?}")]
    TargetOrder {
        /// The prior id.
        previous: String,
        /// The following id.
        current: String,
    },
    /// Mutant identities are not in the one canonical, duplicate-free order.
    #[error("mutant ids are not strictly increasing: {previous:?}, then {current:?}")]
    MutantOrder {
        /// The prior id.
        previous: String,
        /// The following id.
        current: String,
    },
    /// A saved mutant is not named by its canonical full identity.
    #[error("saved mutant identity {value:?} is not 64 lowercase hexadecimal characters")]
    InvalidMutant {
        /// The untrusted value.
        value: String,
    },
    /// A reached source path is not canonical and workspace-relative.
    #[error("target {target:?} carries noncanonical source path {path:?}: {detail}")]
    InvalidFile {
        /// The target carrying the path.
        target: String,
        /// The untrusted path.
        path: String,
        /// Why it is not canonical.
        detail: String,
    },
    /// Reached source paths are not in one duplicate-free order.
    #[error(
        "target {target:?} file paths are not strictly increasing: {previous:?}, then {current:?}"
    )]
    FileOrder {
        /// The target carrying the paths.
        target: String,
        /// The prior path.
        previous: String,
        /// The following path.
        current: String,
    },
    /// A kill without the target that observed it proves nothing.
    #[error("mutant {mutant:?} has an empty observing target")]
    EmptyObserver {
        /// The affected mutant.
        mutant: String,
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
///
/// # Errors
/// Returns [`CheckpointViolation::InvalidIdentity`] before joining a value
/// that is not a canonical full digest into the filesystem path.
pub fn path_of(root: &Path, identity: &str) -> Result<PathBuf, CheckpointViolation> {
    if !rust_mutants::id::is_digest(identity) {
        return Err(CheckpointViolation::InvalidIdentity {
            value: identity.to_owned(),
        });
    }
    Ok(root.join(identity).join(FILE_NAME))
}

/// The state an interrupted run left for `identity`, if any.
///
/// # Errors
/// [`CheckpointError::Corrupt`] when a file is there and is not the state it
/// claims to be: one that does not parse, or one filed under a different
/// identity. Continuing from a checkpoint that is about different inputs would
/// make a run claim what it never established.
pub fn read(root: &Path, identity: &str) -> Result<Option<State>, CheckpointError> {
    let path = checked_path(root, identity)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(CheckpointError::Unusable { path, source }),
    };
    let state: State =
        crate::strictjson::decode_str(&text).map_err(|error| CheckpointError::Corrupt {
            path: path.clone(),
            violation: CheckpointViolation::Malformed {
                detail: error.to_string(),
            },
        })?;
    validate(&state, identity).map_err(|violation| CheckpointError::Corrupt { path, violation })?;
    Ok(Some(state))
}

/// Writes `state` where [`read`] will find it, replacing what was there.
///
/// # Errors
/// See [`CheckpointError::Unusable`].
pub fn write(root: &Path, state: &State) -> Result<PathBuf, CheckpointError> {
    validate(state, &state.identity).map_err(|violation| CheckpointError::Corrupt {
        path: root.to_path_buf(),
        violation,
    })?;
    let path = checked_path(root, &state.identity)?;
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
///
/// # Errors
/// [`CheckpointError::Unusable`] when an existing checkpoint or its now-empty
/// identity directory cannot be removed. Absence is success: interrupted
/// runs need not have established a reusable fact.
pub fn clear(root: &Path, identity: &str) -> Result<(), CheckpointError> {
    let path = checked_path(root, identity)?;
    remove_or_absent(&path, |candidate| std::fs::remove_file(candidate))?;
    if let Some(directory) = path.parent() {
        remove_or_absent(directory, |candidate| std::fs::remove_dir(candidate))?;
    }
    Ok(())
}

fn checked_path(root: &Path, identity: &str) -> Result<PathBuf, CheckpointError> {
    path_of(root, identity).map_err(|violation| CheckpointError::Corrupt {
        path: root.to_path_buf(),
        violation,
    })
}

fn remove_or_absent(
    path: &Path,
    remove: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), CheckpointError> {
    match remove(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CheckpointError::Unusable {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate(state: &State, identity: &str) -> Result<(), CheckpointViolation> {
    if !rust_mutants::id::is_digest(identity) {
        return Err(CheckpointViolation::InvalidIdentity {
            value: identity.to_owned(),
        });
    }
    if state.schema != SCHEMA {
        return Err(CheckpointViolation::WrongSchema {
            found: state.schema.clone(),
        });
    }
    if state.identity != identity {
        return Err(CheckpointViolation::WrongIdentity {
            found: state.identity.clone(),
            expected: identity.to_owned(),
        });
    }
    if state.attempts == 0 {
        return Err(CheckpointViolation::NoAttempts);
    }
    validate_targets(&state.targets)?;
    validate_mutants(&state.mutants)
}

fn validate_targets(targets: &[SavedTarget]) -> Result<(), CheckpointViolation> {
    for target in targets {
        if target.id.is_empty() {
            return Err(CheckpointViolation::EmptyTarget);
        }
        for file in &target.files {
            let normalized = rust_mutants::id::normalize_path(file).map_err(|error| {
                CheckpointViolation::InvalidFile {
                    target: target.id.clone(),
                    path: file.clone(),
                    detail: error.to_string(),
                }
            })?;
            if normalized != *file {
                return Err(CheckpointViolation::InvalidFile {
                    target: target.id.clone(),
                    path: file.clone(),
                    detail: format!("canonical spelling is {normalized:?}"),
                });
            }
        }
        for pair in target.files.windows(2) {
            let [previous, current] = pair else {
                continue;
            };
            if previous >= current {
                return Err(CheckpointViolation::FileOrder {
                    target: target.id.clone(),
                    previous: previous.clone(),
                    current: current.clone(),
                });
            }
        }
    }
    for pair in targets.windows(2) {
        let [previous, current] = pair else {
            continue;
        };
        if previous.id >= current.id {
            return Err(CheckpointViolation::TargetOrder {
                previous: previous.id.clone(),
                current: current.id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_mutants(mutants: &[SavedMutant]) -> Result<(), CheckpointViolation> {
    for pair in mutants.windows(2) {
        let [previous, current] = pair else {
            continue;
        };
        if previous.id >= current.id {
            return Err(CheckpointViolation::MutantOrder {
                previous: previous.id.clone(),
                current: current.id.clone(),
            });
        }
    }
    for mutant in mutants {
        if !rust_mutants::id::is_id(&mutant.id) {
            return Err(CheckpointViolation::InvalidMutant {
                value: mutant.id.clone(),
            });
        }
        match &mutant.disposition {
            SavedDisposition::Killed { by } if by.is_empty() => {
                return Err(CheckpointViolation::EmptyObserver {
                    mutant: mutant.id.clone(),
                });
            }
            SavedDisposition::Killed { .. } => {}
        }
    }
    Ok(())
}
