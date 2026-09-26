// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! When an answer recorded on one tree answers for another that differs only inside sealed bodies its executions never entered (ADR 0041).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use std::path::{Path, PathBuf};

use crate::id::HexDigest;
use crate::outcomes::{CacheOutcome, Keyed, StoreError};
use crate::touch::{Completeness, ItemRef};

/// The schema every carried record names.
pub const SCHEMA: &str = "rust-mutants-carried-v1";

/// Where carried records live under the cache directory.
pub const LAYOUT: &str = "rust-mutants/carried-v1";

/// The file a run keeps beside its report naming every carried record it believed.
pub const FILE: &str = "carried-v1.json";

/// The document type of [`FILE`].
pub const DOCUMENT: &str = "rust-mutants/carried";

/// The version of [`DOCUMENT`] this release writes.
pub const DOCUMENT_VERSION: u32 = 1;

/// Every carried record one run believed, which is what an audit re-derives each carried answer from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Believed {
    /// [`DOCUMENT`].
    pub document_type: String,
    /// [`DOCUMENT_VERSION`].
    pub schema_version: u32,
    /// Each record, by the mutant it answered for, in the order of their full identities.
    pub records: Vec<BelievedRecord>,
}

/// One carried record a run believed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BelievedRecord {
    /// The mutant's full identity in this run's catalog.
    pub mutant: String,
    /// The record.
    pub record: Carried,
    /// The executions this run's route would have made, which the record was held to.
    pub plan: Vec<Planned>,
}

/// Where a mutation sits and what it writes, named by nothing outside the item it edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locus {
    /// The item whose body the edit is inside.
    pub item: ItemRef,
    /// The digest of that body in the pristine tree.
    pub body_digest: String,
    /// Where the edit starts, in bytes from the start of the body.
    pub start: u32,
    /// Where the edit ends, in bytes from the start of the body.
    pub end: u32,
    /// The text the edit writes.
    pub replacement: String,
    /// The rule that made the mutation, with its version.
    pub rule: String,
}

/// One execution an answer rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    /// The target that ran.
    pub target: String,
    /// The tests it was told to run, or nothing where it ran the whole target.
    pub filter: Option<Vec<String>>,
    /// The skeleton of the target when it ran.
    pub skeleton: String,
    /// Every item the process entered, with the digest its body had.
    pub entered: BTreeSet<Entered>,
    /// How much of the process that list accounts for.
    pub completeness: Completeness,
    /// Whether a test of it failed.
    pub detected: bool,
}

/// One item an execution entered, with the digest its body had then.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entered {
    /// The item.
    pub item: ItemRef,
    /// The digest of its body.
    pub body_digest: String,
}

/// An answer about one mutation, with every execution it rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Carried {
    /// [`SCHEMA`].
    pub schema: String,
    /// Where the mutation sits.
    pub locus: Locus,
    /// Everything else the record is filed under; its closure is kept as a note and is not in the key.
    pub keyed: Keyed,
    /// What the executions established.
    pub outcome: CacheOutcome,
    /// The target that answered.
    pub target: String,
    /// How many tests the answering execution ran, when the harness said.
    pub tests_run: Option<u32>,
    /// Every test that failed with the mutant active.
    pub failed_tests: Vec<String>,
    /// The run that established it.
    pub run_id: String,
    /// Every execution the answer rests on, in the order they ran.
    pub executions: Vec<Execution>,
}

impl Carried {
    /// The key the record is filed under, recomputed from what it holds.
    #[must_use]
    pub fn key(&self) -> HexDigest {
        key(&self.keyed, &self.locus)
    }
}

/// The key a record of the mutation at `locus` is filed under: everything the exact key holds except the closure, and the locus in place of the mutant's identity.
#[must_use]
pub fn key(keyed: &Keyed, locus: &Locus) -> HexDigest {
    let mut hasher = Sha256::new();
    let abi = crate::outcomes::Abi::CURRENT;
    for field in [
        SCHEMA.to_owned(),
        abi.rule.to_string(),
        abi.instrumentation.to_string(),
        abi.step_policy.to_string(),
        abi.cache.to_string(),
        keyed.manifests.clone(),
        keyed.toolchain.clone(),
        keyed.engine.clone(),
        keyed.runner_tag(),
        keyed.timeout.clone(),
        keyed.steps.to_string(),
        keyed.declared.digest.clone(),
        locus.item.package.clone(),
        locus.item.path.clone(),
        locus.item.ordinal.to_string(),
        locus.body_digest.clone(),
        locus.rule.clone(),
        locus.start.to_string(),
        locus.end.to_string(),
        locus.replacement.clone(),
    ] {
        framed(&mut hasher, &field);
    }
    for list in [&keyed.args, &keyed.build] {
        framed(&mut hasher, &list.len().to_string());
        for argument in list {
            framed(&mut hasher, argument);
        }
    }
    HexDigest::finish(hasher)
}

fn framed(hasher: &mut Sha256, field: &str) {
    hasher.update(field.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(field.as_bytes());
}

/// What this run knows of its own tree, which a record is held to.
#[derive(Debug, Clone)]
pub struct Now<'a> {
    /// Each target's skeleton now.
    pub skeletons: BTreeMap<String, String>,
    /// Each item's body digest now, and whether that body is sealed.
    pub items: &'a BTreeMap<ItemRef, Body>,
    /// The targets whose reach held under a control of this tree.
    pub held: BTreeSet<String>,
}

/// One item's body now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// Its digest.
    pub digest: String,
    /// Whether it is sealed.
    pub sealing: Sealing,
}

/// Whether a body contributes anything to the program but its own execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Sealing {
    /// It contributes nothing else.
    Sealed,
    /// It may.
    Unsealed,
}

/// One execution this run's route would make: a target and the tests it would name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Planned {
    /// The target.
    pub target: String,
    /// The tests it would be told to run, or nothing for the whole target.
    pub filter: Option<Vec<String>>,
}

/// Why a record does not answer for this run, in the words the trace uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Refusal {
    /// A target's skeleton is not what it was.
    SkeletonChanged,
    /// An item an execution entered has another body now.
    ItemChanged,
    /// An item an execution entered is not sealed now.
    Unsealed,
    /// An execution's record of what it entered does not reach far enough for this answer.
    EntryIncomplete,
    /// The route executes a target no recorded execution ran.
    RouteGrew,
    /// The route names other tests of a target than the recorded execution ran.
    FilterDiffers,
    /// A target the answer rests on did not hold its reach under a control of this tree, so the answer may have missed a changed body by chance.
    ReachMoved,
    /// A survival rests on a target whose process this run cannot see into, so no run of it could have claimed a survival.
    Uncontrolled,
}

impl Refusal {
    /// The word the trace records.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SkeletonChanged => "skeleton-changed",
            Self::ItemChanged => "item-changed",
            Self::Unsealed => "unsealed",
            Self::EntryIncomplete => "entry-incomplete",
            Self::RouteGrew => "route-grew",
            Self::FilterDiffers => "filter-differs",
            Self::ReachMoved => "reach-moved",
            Self::Uncontrolled => "uncontrolled",
        }
    }
}

/// Why a record is not the record it claims to be.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MalformedCarriedError {
    /// A kill whose last execution detected nothing, or a survival an execution detected.
    #[error("the record says {outcome:?} and its executions do not end the way that outcome ends")]
    Outcome {
        /// What the record says.
        outcome: CacheOutcome,
    },
    /// The answering target is not the target of the execution that answered.
    #[error("the record says {target} answered, and its last execution ran {ran}")]
    Target {
        /// What the record says.
        target: String,
        /// What the last execution ran.
        ran: String,
    },
}

impl Carried {
    /// Whether the record's executions end the way its outcome says.
    ///
    /// # Errors
    /// [`MalformedCarriedError`] naming the first way it does not.
    pub fn validate(&self) -> Result<(), MalformedCarriedError> {
        let detected: Vec<&Execution> = self.executions.iter().filter(|one| one.detected).collect();
        let last = self.executions.last();
        let ends = match self.outcome {
            CacheOutcome::Killed => detected.len() == 1 && last.is_some_and(|one| one.detected),
            CacheOutcome::Survived => detected.is_empty() && last.is_some(),
        };
        if !ends {
            return Err(MalformedCarriedError::Outcome {
                outcome: self.outcome,
            });
        }
        if let Some(last) = last.filter(|last| last.target != self.target) {
            return Err(MalformedCarriedError::Target {
                target: self.target.clone(),
                ran: last.target.clone(),
            });
        }
        Ok(())
    }
}

/// Whether `record` answers for this run, whose tree is `now` and whose route would make `plan`: every premise of ADR 0041 holds, or the first that does not is named.
///
/// A kill rests on its killing execution alone; a survival rests on an execution of every target the route makes, each with the tests it would name.
///
/// # Errors
/// The [`Refusal`] of the first premise that does not hold.
pub fn believe(record: &Carried, now: &Now<'_>, plan: &[Planned]) -> Result<(), Refusal> {
    match record.outcome {
        CacheOutcome::Killed => {
            let Some(killer) = record.executions.iter().rev().find(|one| one.detected) else {
                return Err(Refusal::EntryIncomplete);
            };
            let Some(planned) = plan.iter().find(|one| one.target == killer.target) else {
                return Err(Refusal::FilterDiffers);
            };
            if planned.filter != killer.filter {
                return Err(Refusal::FilterDiffers);
            }
            held(
                killer,
                now,
                &[Completeness::Whole, Completeness::UpToFirstFailure],
            )
        }
        CacheOutcome::Survived => {
            for planned in plan {
                let mut ran = record
                    .executions
                    .iter()
                    .filter(|one| one.target == planned.target)
                    .peekable();
                if ran.peek().is_none() {
                    return Err(Refusal::RouteGrew);
                }
                let Some(execution) = ran.find(|one| one.filter == planned.filter) else {
                    return Err(Refusal::FilterDiffers);
                };
                held(execution, now, &[Completeness::Whole])?;
            }
            Ok(())
        }
    }
}

/// Whether one execution would do on this tree what it did on its own: its record reaches far enough, its target holds its reach, its skeleton is unchanged, and every item it entered has the same sealed body.
fn held(execution: &Execution, now: &Now<'_>, enough: &[Completeness]) -> Result<(), Refusal> {
    if !enough.contains(&execution.completeness) {
        return Err(Refusal::EntryIncomplete);
    }
    if !now.held.contains(&execution.target) {
        return Err(Refusal::ReachMoved);
    }
    if now.skeletons.get(&execution.target) != Some(&execution.skeleton) {
        return Err(Refusal::SkeletonChanged);
    }
    for entered in &execution.entered {
        let Some(body) = now.items.get(&entered.item) else {
            return Err(Refusal::ItemChanged);
        };
        if body.digest != entered.body_digest {
            return Err(Refusal::ItemChanged);
        }
        if body.sealing == Sealing::Unsealed {
            return Err(Refusal::Unsealed);
        }
    }
    Ok(())
}

/// The skeleton every target of a tree is held to: the fold of every unit's skeleton, named without a package id, in byte order of the names.
///
/// It is coarser than the units one target links, and sound for being so: an edit outside a sealed body anywhere moves it.
#[must_use]
pub fn tree_skeleton(units: &[crate::skeleton::UnitSkeleton]) -> String {
    let mut lines: Vec<String> = units
        .iter()
        .map(|unit| {
            format!(
                "{}\0{}\0{}\0{}\0{}\n",
                unit.package, unit.target, unit.kind, unit.test, unit.skeleton
            )
        })
        .collect();
    lines.sort();
    crate::id::digest(lines.concat().as_bytes())
}

/// The carried records earlier runs left, each under its locus key.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The store under `cache_directory`.
    #[must_use]
    pub fn new(cache_directory: &Path) -> Self {
        Self {
            root: cache_directory.join(LAYOUT),
        }
    }

    /// The record filed under `key`, when one is there and is the record its name promises.
    ///
    /// # Errors
    /// Every I/O or decoding failure, and a record whose key, schema or executions contradict it; a corrupt entry is not an absent one.
    pub fn get(&self, key: &HexDigest) -> Result<Option<Carried>, StoreError> {
        let path = self.entry(key);
        let text = match crate::outcomes::read_through_a_replacement(&path) {
            Ok(Some(text)) => text,
            Ok(None) => return Ok(None),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let corrupt = |message: String| StoreError::Corrupt {
            path: path.clone(),
            message,
        };
        let record: Carried =
            crate::strictjson::decode_str(&text).map_err(|error| corrupt(error.to_string()))?;
        if record.schema != SCHEMA {
            return Err(corrupt(format!(
                "record schema {:?} is not {SCHEMA:?}",
                record.schema
            )));
        }
        if record.key() != *key {
            return Err(corrupt(format!(
                "the record's own inputs key it as {}, not the {key} it is filed under",
                record.key()
            )));
        }
        record
            .validate()
            .map_err(|malformed| corrupt(malformed.to_string()))?;
        Ok(Some(record))
    }

    /// Files `record` under the key its own inputs name.
    ///
    /// # Errors
    /// Serialization and filesystem failures.
    pub fn put(&self, record: &Carried) -> Result<PathBuf, StoreError> {
        let path = self.entry(&record.key());
        let text = serde_json::to_string(record).map_err(|error| StoreError::Corrupt {
            path: path.clone(),
            message: error.to_string(),
        })?;
        crate::replace::file(&path, text.as_bytes()).map_err(|failure| StoreError::Io {
            path: failure.path,
            source: failure.source,
        })?;
        Ok(path)
    }

    fn entry(&self, key: &HexDigest) -> PathBuf {
        self.root.join(format!("{key}.json"))
    }
}
