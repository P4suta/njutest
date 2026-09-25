// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What earlier runs established about individual mutants, and the conditions under which this run may believe it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rust_mutants::id::HexDigest;
use serde::{Deserialize, Serialize};

/// The name of the shape.
pub const SCHEMA: &str = "njutest-mutation-evidence-v2";

/// The directory records live in, below the store of answers.
pub const LAYOUT: &str = "mutants-v2";

/// What one earlier run established about one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// [`SCHEMA`].
    pub schema: String,
    /// The mutant's content-addressed identity.
    pub mutant: HexDigest,
    /// The run that established it.
    pub run_id: String,
    /// What it established.
    pub outcome: Outcome,
}

/// What an earlier run established about one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    /// One named target noticed it, and that target had this behaviour key.
    Killed {
        /// The target that noticed.
        target: String,
        /// That target's behaviour key.
        key: String,
        /// Every target asked before it, in the order they were asked, with what each answered.
        before: Vec<Answer>,
    },
    /// Every target that could notice it passed with it active.
    /// The claim is about all of them, so all of them are named.
    Survived {
        /// Every target the run routed to it, with the behaviour key each had.
        targets: BTreeMap<String, String>,
    },
}

/// One target a run asked about a mutant before another noticed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    /// The target asked.
    pub target: String,
    /// That target's behaviour key.
    pub key: String,
    /// What it answered.
    pub outcome: crate::report::Outcome,
}

/// Why a record could not be believed.
/// A run says which so a reader can tell "nothing was recorded" from "what was recorded no longer describes this tree".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing was ever recorded about this mutant.
    Nothing,
    /// A record is there and could not be read as one.
    Unreadable {
        /// What stopped the read.
        message: String,
    },
    /// A target this run routes to the mutant is not one it measured a baseline for, so a record naming it could be neither believed nor written.
    TargetUnknown {
        /// The target this run has no identity for.
        target: String,
    },
    /// The recorded target is not one this run routes to the mutant.
    NotRouted {
        /// The target the record names.
        target: String,
    },
    /// The recorded target does not behave the way it did.
    KeyChanged {
        /// The target whose key changed.
        target: String,
    },
    /// This run's own baseline did not see the recorded target pass on the original tree.
    NotPassing {
        /// The target this run could not vouch for.
        target: String,
    },
    /// A target this run routes to the mutant is one the record says nothing about: a test nothing was ever run against.
    TargetEntered {
        /// The target that entered the reaching set.
        target: String,
    },
    /// This run routes no target to the mutant, so the recorded survival is a universal claim over nothing.
    NothingRouted,
}

impl Refusal {
    /// The word a recording carries, so a reader counts the reasons a run did the work again without reading prose.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Nothing => "nothing-recorded",
            Self::Unreadable { .. } => "unreadable",
            Self::TargetUnknown { .. } => "target-unknown",
            Self::NotRouted { .. } => "not-routed",
            Self::KeyChanged { .. } => "key-changed",
            Self::NotPassing { .. } => "not-passing",
            Self::TargetEntered { .. } => "target-entered",
            Self::NothingRouted => "nothing-routed",
        }
    }
}

/// What this run knows about the targets a record may name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Standing {
    /// The behaviour key of every target this run's baseline saw pass on the original tree.
    pub passing: BTreeMap<String, String>,
}

impl Standing {
    /// Whether this run saw `target` pass and it behaves the way `key` says.
    fn vouches(&self, target: &str, key: &str) -> Result<(), Refusal> {
        match self.passing.get(target) {
            None => Err(Refusal::NotPassing {
                target: target.to_owned(),
            }),
            Some(current) if current == key => Ok(()),
            Some(_changed) => Err(Refusal::KeyChanged {
                target: target.to_owned(),
            }),
        }
    }
}

impl Record {
    /// Whether this run may believe what the record says, given the targets it routes to the mutant in the order it asks them and what it saw of them.
    ///
    /// A kill is believed only where this run would ask exactly the targets the record says were asked before the one that noticed, in the order it asks them, so the answers it reads back are the ones asking again would give.
    ///
    /// # Errors
    /// Returns why not, which is never "no reason".
    pub fn believable(&self, asking: &[String], standing: &Standing) -> Result<(), Refusal> {
        let reaching: BTreeSet<&String> = asking.iter().collect();
        match &self.outcome {
            Outcome::Killed {
                target,
                key,
                before,
            } => {
                let Some(position) = asking.iter().position(|one| one == target) else {
                    return Err(Refusal::NotRouted {
                        target: target.clone(),
                    });
                };
                standing.vouches(target, key)?;
                if let Some(noticed) = before
                    .iter()
                    .find(|answer| answer.outcome == crate::report::Outcome::Killed)
                {
                    return Err(Refusal::Unreadable {
                        message: format!(
                            "says {} noticed it before {target} did, and a run stops at the first \
                             kill it confirms",
                            noticed.target
                        ),
                    });
                }
                for answer in before {
                    if !asking
                        .iter()
                        .take(position)
                        .any(|one| *one == answer.target)
                    {
                        return Err(Refusal::NotRouted {
                            target: answer.target.clone(),
                        });
                    }
                    standing.vouches(&answer.target, &answer.key)?;
                }
                if let Some(entered) = asking
                    .iter()
                    .take(position)
                    .find(|one| !before.iter().any(|answer| answer.target == **one))
                {
                    return Err(Refusal::TargetEntered {
                        target: entered.clone(),
                    });
                }
                if !before
                    .iter()
                    .map(|answer| &answer.target)
                    .eq(asking.iter().take(position))
                {
                    return Err(Refusal::Unreadable {
                        message: "names the targets asked before the kill in an order no run \
                                  asks them in"
                            .to_owned(),
                    });
                }
                Ok(())
            }
            Outcome::Survived { targets } => {
                if reaching.is_empty() {
                    return Err(Refusal::NothingRouted);
                }
                for target in reaching {
                    let Some(key) = targets.get(target.as_str()) else {
                        return Err(Refusal::TargetEntered {
                            target: (*target).clone(),
                        });
                    };
                    standing.vouches(target, key)?;
                }
                Ok(())
            }
        }
    }
}

/// Why the store could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// A record could not be read or written.
    #[error("{}: {}: {source}", crate::error::CACHE_UNUSABLE.code, path.display())]
    Unusable {
        /// The record.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A record is there and is not the record it claims to be.
    #[error("{}: {}: {message}", crate::error::CACHE_CORRUPT.code, path.display())]
    Corrupt {
        /// The record.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
}

impl StoreError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::Unusable { .. } => crate::error::CACHE_UNUSABLE,
            Self::Corrupt { .. } => crate::error::CACHE_CORRUPT,
        }
    }
}

/// Where one mutant's record lives under `root`.
#[must_use]
pub fn path_of(root: &Path, mutant: &HexDigest) -> PathBuf {
    root.join(LAYOUT).join(format!("{mutant}.json"))
}

/// What an earlier run established about `mutant`, if anything.
///
/// # Errors
/// [`StoreError::Corrupt`] when a record is there and is not about the mutant it is filed under.
/// A record that describes something else is not a record this run may quietly ignore: it is one somebody wrote wrong.
pub fn read(root: &Path, mutant: &HexDigest) -> Result<Option<Record>, StoreError> {
    let path = path_of(root, mutant);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StoreError::Unusable { path, source }),
    };
    let record: Record =
        crate::strictjson::decode_str(&text).map_err(|error| StoreError::Corrupt {
            path: path.clone(),
            message: error.to_string(),
        })?;
    if record.schema != SCHEMA {
        return Err(StoreError::Corrupt {
            path,
            message: format!("says it is {:?} and not {SCHEMA:?}", record.schema),
        });
    }
    if &record.mutant != mutant {
        return Err(StoreError::Corrupt {
            path,
            message: format!("is about {} and was read for {mutant}", record.mutant),
        });
    }
    Ok(Some(record))
}

/// Records what this run established about one mutant, replacing whatever was there.
/// A record is contradicted by being replaced; nothing else removes one.
///
/// # Errors
/// See [`StoreError::Unusable`].
pub fn write(root: &Path, record: &Record) -> Result<PathBuf, StoreError> {
    let path = path_of(root, &record.mutant);
    let text = serde_json::to_string(record).map_err(|error| StoreError::Unusable {
        path: path.clone(),
        source: std::io::Error::other(error),
    })?;
    rust_mutants::replace::file(&path, text.as_bytes()).map_err(|failure| {
        StoreError::Unusable {
            path: failure.path,
            source: failure.source,
        }
    })?;
    Ok(path)
}

/// A record of what `run_id` established about `mutant`.
#[must_use]
pub fn record(mutant: HexDigest, run_id: &str, outcome: Outcome) -> Record {
    Record {
        schema: SCHEMA.to_owned(),
        mutant,
        run_id: run_id.to_owned(),
        outcome,
    }
}
