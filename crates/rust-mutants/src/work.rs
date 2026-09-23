// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run did, counted rather than timed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::report::run::{RunDocument, RunMutantDocument};

/// A count whose exact representation is part of the work ledger contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkQuantity {
    /// Built test targets.
    Targets,
    /// Compiler-accepted mutants.
    CatalogedMutants,
    /// Mutant-target pairs in the whole run.
    WholePairs,
    /// Processes actually started.
    StartedProcesses,
    /// Tests in the whole run.
    WholeTests,
    /// Tests actually started.
    StartedTests,
    /// Pairs attributed to one removal reason.
    RemovedPairs,
    /// Mutants attributed to one removal reason.
    RemovedMutants,
    /// Executed targets in one route.
    ExecutedTargets,
    /// Reaching targets in one route.
    ReachingTargets,
    /// Named tests in one route.
    NamedTests,
    /// Pairs discharged by proofs.
    DischargedPairs,
    /// All skipped pairs.
    SkippedPairs,
    /// Started and skipped pairs used to balance the ledger.
    BalancedPairs,
}

impl std::fmt::Display for WorkQuantity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Targets => "target count",
            Self::CatalogedMutants => "cataloged-mutant count",
            Self::WholePairs => "whole pair count",
            Self::StartedProcesses => "started-process count",
            Self::WholeTests => "whole test count",
            Self::StartedTests => "started-test count",
            Self::RemovedPairs => "removed-pair count",
            Self::RemovedMutants => "removed-mutant count",
            Self::ExecutedTargets => "executed-target count",
            Self::ReachingTargets => "reaching-target count",
            Self::NamedTests => "named-test count",
            Self::DischargedPairs => "discharged-pair count",
            Self::SkippedPairs => "skipped-pair count",
            Self::BalancedPairs => "balanced-pair count",
        })
    }
}

/// Why an exact work ledger cannot be constructed from a run document.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WorkError {
    /// A host collection length does not fit the durable counter.
    #[error("{quantity} {count} does not fit its durable counter")]
    CountOutsideRange {
        /// The count whose representation was refused.
        quantity: WorkQuantity,
        /// The exact host count.
        count: usize,
    },
    /// Exact arithmetic for one quantity overflowed.
    #[error("{quantity} overflowed its durable counter")]
    CountOverflow {
        /// The count whose arithmetic was refused.
        quantity: WorkQuantity,
    },
    /// One route claims mutually inconsistent target partitions.
    #[error(
        "a route over {targets} targets records {reaching} reaching, {discharged} discharged, and {executed} executed targets"
    )]
    RoutingContradiction {
        /// All targets in the run.
        targets: u64,
        /// Targets claimed to reach the mutant.
        reaching: u64,
        /// Target pairs claimed to be discharged.
        discharged: u64,
        /// Targets claimed to have executed.
        executed: u64,
    },
    /// One textual reason was assigned incompatible semantic kinds.
    #[error("removal reason {reason:?} has two incompatible kinds")]
    RemovalKindConflict {
        /// The ambiguous reason.
        reason: String,
    },
}

/// A mutation no measured target reaches, which coverage routing removed.
pub const UNREACHED: &str = "unreached";

/// A target the run never asked because an earlier one had already answered.
pub const ANSWERED: &str = "answered";

/// An outcome an earlier run of the same tree established.
pub const REUSED: &str = "reused";

/// What kind of thing removed a pair, which is what says whether the answer is still the whole answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Removal {
    /// Something proved the pair could establish nothing, so the answer is unchanged.
    Proof,
    /// A target had answered already, so asking another establishes nothing new.
    Sufficiency,
    /// An earlier run of the same tree established it, so the answer is unchanged.
    Memory,
    /// The run was asked for less than the whole, so the answer is about less.
    Selection,
}

impl Removal {
    /// Whether a run that removed pairs this way still answers for the whole catalog.
    #[must_use]
    pub const fn whole(self) -> bool {
        !matches!(self, Self::Selection)
    }
}

/// How many pairs one thing removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removed {
    /// What removed them: a proof's own name, or one of [`UNREACHED`], [`ANSWERED`], [`REUSED`], or a not-run reason.
    pub reason: String,
    /// What kind of removal it is.
    pub removal: Removal,
    /// How many (mutant, target) pairs it removed.
    pub pairs: u64,
    /// How many mutants it removed at least one pair of.
    pub mutants: u32,
}

/// What a whole run would have cost, what this one cost, and what removed the difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    /// The test targets the run built.
    pub targets: u32,
    /// The mutants the compiler accepted.
    pub cataloged: u32,
    /// Every pair a run that asked every target about every mutant would have started.
    pub whole: u64,
    /// The pairs this run started a process for, a confirming retry counted again.
    pub started: u64,
    /// Every test a run that asked every test of every target about every mutant would have started.
    pub tests_whole: u64,
    /// The tests this run started, the ones it started to establish a filter included.
    pub tests_started: u64,
    /// How many of those were started to establish that a filtered set answers on its own.
    pub established: u64,
    /// What removed the rest, the largest first.
    pub removed: Vec<Removed>,
}

impl Work {
    /// The ledger of one stored run.
    ///
    /// # Errors
    /// Refuses any count, arithmetic result, or route partition that cannot be represented exactly by the durable ledger.
    pub fn of(document: &RunDocument) -> Result<Self, WorkError> {
        let targets = count_u32(WorkQuantity::Targets, document.targets.len())?;
        let cataloged = count_u32(WorkQuantity::CatalogedMutants, document.mutants.len())?;
        let held: BTreeMap<&str, u64> = document
            .targets
            .iter()
            .map(|target| (target.id.as_str(), u64::from(target.tests.max(1))))
            .collect();
        let every = held.values().try_fold(0_u64, |total, tests| {
            checked_add(total, *tests, WorkQuantity::WholeTests)
        })?;
        let mut started: u64 = 0;
        let mut tests_started: u64 = document.established_tests;
        let mut removed: BTreeMap<String, (Removal, u64, u32)> = BTreeMap::new();
        for mutant in &document.mutants {
            started = checked_add(started, processes(mutant)?, WorkQuantity::StartedProcesses)?;
            tests_started = checked_add(
                tests_started,
                tests(mutant, &held)?,
                WorkQuantity::StartedTests,
            )?;
            for (reason, removal, pairs) in per_mutant(mutant, u64::from(targets))? {
                let reason_for_error = reason.clone();
                let entry = removed.entry(reason).or_insert((removal, 0, 0));
                if entry.0 != removal {
                    return Err(WorkError::RemovalKindConflict {
                        reason: reason_for_error,
                    });
                }
                entry.1 = checked_add(entry.1, pairs, WorkQuantity::RemovedPairs)?;
                entry.2 = entry.2.checked_add(1).ok_or(WorkError::CountOverflow {
                    quantity: WorkQuantity::RemovedMutants,
                })?;
            }
        }
        let mut removed: Vec<Removed> = removed
            .into_iter()
            .map(|(reason, (removal, pairs, mutants))| Removed {
                reason,
                removal,
                pairs,
                mutants,
            })
            .collect();
        removed.sort_by(|left, right| {
            right
                .pairs
                .cmp(&left.pairs)
                .then_with(|| left.reason.cmp(&right.reason))
        });
        Ok(Self {
            targets,
            cataloged,
            whole: checked_mul(
                u64::from(cataloged),
                u64::from(targets),
                WorkQuantity::WholePairs,
            )?,
            started,
            tests_whole: checked_mul(u64::from(cataloged), every, WorkQuantity::WholeTests)?,
            tests_started,
            established: document.established_tests,
            removed,
        })
    }

    /// Whether every pair a whole run would have started is one this run started or one something named removed.
    ///
    /// # Errors
    /// Refuses when summing the removed pairs or adding them to the started pairs exceeds the durable counter.
    pub fn balances(&self) -> Result<bool, WorkError> {
        Ok(checked_add(self.pairs(), self.skipped()?, WorkQuantity::BalancedPairs)? == self.whole)
    }

    /// The pairs a process was started for, counting a pair asked twice once.
    #[must_use]
    pub fn pairs(&self) -> u64 {
        self.started.min(self.whole)
    }

    /// Every pair something removed.
    ///
    /// # Errors
    /// Refuses when the exact sum of removed pairs exceeds the durable counter.
    pub fn skipped(&self) -> Result<u64, WorkError> {
        self.removed.iter().try_fold(0_u64, |total, one| {
            checked_add(total, one.pairs, WorkQuantity::SkippedPairs)
        })
    }

    /// How many tests this run started to establish that a filtered set answers on its own.
    #[must_use]
    pub const fn established_tests(&self) -> u64 {
        self.established
    }

    /// The share of the tests a whole run would have started that this one did not, between 0 and 1.
    #[must_use]
    pub fn tests_saved(&self) -> f64 {
        if self.tests_whole == 0 || self.tests_started >= self.tests_whole {
            return 0.0;
        }
        let Some(saved) = self.tests_whole.checked_sub(self.tests_started) else {
            return 0.0;
        };
        match crate::count::ratio(saved, self.tests_whole) {
            Some(share) => share,
            None => 0.0,
        }
    }

    /// The share of a whole run this one did not do, between 0 and 1.
    ///
    /// # Errors
    /// Refuses when the exact sum of removed pairs exceeds the durable counter.
    pub fn saved(&self) -> Result<f64, WorkError> {
        Ok(match crate::count::ratio(self.skipped()?, self.whole) {
            Some(share) => share,
            None => 0.0,
        })
    }

    /// Whether every removal leaves the verdict where a whole run would have left it.
    #[must_use]
    pub fn answers_for_the_whole(&self) -> bool {
        self.removed.iter().all(|one| one.removal.whole())
    }
}

/// How many processes one mutant cost.
fn processes(mutant: &RunMutantDocument) -> Result<u64, WorkError> {
    if mutant.source_run_id.is_some() {
        return Ok(0);
    }
    let executed = mutant
        .route
        .as_ref()
        .map_or(0, |route| route.executed.len());
    let executed = count_u64(WorkQuantity::ExecutedTargets, executed)?;
    checked_add(
        executed,
        u64::from(mutant.retried),
        WorkQuantity::StartedProcesses,
    )
}

/// How many tests one mutant cost, which is what each target it ran was asked for.
fn tests(mutant: &RunMutantDocument, held: &BTreeMap<&str, u64>) -> Result<u64, WorkError> {
    if mutant.source_run_id.is_some() {
        return Ok(0);
    }
    let Some(route) = mutant.route.as_ref() else {
        return Ok(0);
    };
    let asked = |target: &str| -> Result<u64, WorkError> {
        match route.tests.get(target) {
            Some(named) => count_u64(WorkQuantity::NamedTests, named.len()),
            None => Ok(held.get(target).copied().unwrap_or(1)),
        }
    };
    let walked = route.executed.iter().try_fold(0_u64, |total, target| {
        checked_add(total, asked(target)?, WorkQuantity::StartedTests)
    })?;
    let again = if mutant.retried {
        asked(&mutant.target)?
    } else {
        0
    };
    checked_add(walked, again, WorkQuantity::StartedTests)
}

/// What removed each of one mutant's pairs, and what kind of removal it was.
fn per_mutant(
    mutant: &RunMutantDocument,
    targets: u64,
) -> Result<Vec<(String, Removal, u64)>, WorkError> {
    if mutant.source_run_id.is_some() {
        return Ok(vec![(REUSED.to_owned(), Removal::Memory, targets)]);
    }
    let Some(route) = mutant.route.as_ref() else {
        let reason = mutant
            .not_run_reason
            .map_or(UNREACHED, crate::run::NotRunReason::name)
            .to_owned();
        return Ok(vec![(reason.clone(), kind_of(&reason), targets)]);
    };
    let reaching = count_u64(WorkQuantity::ReachingTargets, route.reaching.len())?;
    let executed = count_u64(WorkQuantity::ExecutedTargets, route.executed.len())?;
    let mut removed: Vec<(String, Removal, u64)> = Vec::new();
    let mut by_proof: BTreeMap<&str, u64> = BTreeMap::new();
    for discharge in &route.discharged {
        let counted = by_proof.entry(discharge.proof.as_str()).or_insert(0);
        *counted = checked_add(*counted, 1, WorkQuantity::DischargedPairs)?;
    }
    let discharged = by_proof.values().try_fold(0_u64, |total, pairs| {
        checked_add(total, *pairs, WorkQuantity::DischargedPairs)
    })?;
    for (proof, pairs) in by_proof {
        removed.push((proof.to_owned(), Removal::Proof, pairs));
    }
    let partitioned = reaching.checked_add(discharged);
    let unreached = partitioned.and_then(|partitioned| targets.checked_sub(partitioned));
    let Some(unreached) = unreached else {
        return Err(WorkError::RoutingContradiction {
            targets,
            reaching,
            discharged,
            executed,
        });
    };
    if unreached > 0 {
        removed.push((UNREACHED.to_owned(), Removal::Proof, unreached));
    }
    let Some(unasked) = reaching.checked_sub(executed) else {
        return Err(WorkError::RoutingContradiction {
            targets,
            reaching,
            discharged,
            executed,
        });
    };
    if unasked > 0 {
        let reason = mutant
            .not_run_reason
            .map_or(ANSWERED, crate::run::NotRunReason::name)
            .to_owned();
        removed.push((reason.clone(), kind_of(&reason), unasked));
    }
    Ok(removed)
}

fn count_u32(quantity: WorkQuantity, count: usize) -> Result<u32, WorkError> {
    u32::try_from(count).map_err(|_outside_range| WorkError::CountOutsideRange { quantity, count })
}

fn count_u64(quantity: WorkQuantity, count: usize) -> Result<u64, WorkError> {
    u64::try_from(count).map_err(|_outside_range| WorkError::CountOutsideRange { quantity, count })
}

fn checked_add(left: u64, right: u64, quantity: WorkQuantity) -> Result<u64, WorkError> {
    left.checked_add(right)
        .ok_or(WorkError::CountOverflow { quantity })
}

fn checked_mul(left: u64, right: u64, quantity: WorkQuantity) -> Result<u64, WorkError> {
    left.checked_mul(right)
        .ok_or(WorkError::CountOverflow { quantity })
}

/// What kind of removal a not-run reason is.
fn kind_of(reason: &str) -> Removal {
    match reason {
        UNREACHED | "discharged" => Removal::Proof,
        ANSWERED => Removal::Sufficiency,
        REUSED => Removal::Memory,
        _ => Removal::Selection,
    }
}
