// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run did, counted rather than timed.
//!
//! The unit is one **pair**: one mutant and one test target, which a run either
//! starts a process for or does not. A run that asked every target about every
//! mutant would start `cataloged × targets` of them; every pair short of that
//! is one something removed, and this ledger names what.
//!
//! A pair is a process, and a process is not the whole of what a run does: one
//! that runs the two tests that reached a mutation costs less than one that
//! runs the target's two hundred. So the ledger counts **tests** as well, and
//! that is the number the guards move. Every test a run started is in it, the
//! ones it started to establish that a filtered set answers on its own
//! included.
//!
//! Nothing here is a duration. A count is the same on a loaded machine and an
//! idle one, on four jobs and on one, so a change that makes the engine do less
//! work is a change a test can see and a ratchet can hold. Time is what the
//! work costs, not what it is.
//!
//! Every number is derived from the stored run report and nothing else, so an
//! audit re-derives the same ledger without the engine that produced it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::report::run::{RunDocument, RunMutantDocument};

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
    ///
    /// A proof, a sufficient answer and a remembered one all leave the verdict
    /// exactly where a whole run would have left it. A selection does not: it
    /// is a smaller question, honestly asked.
    #[must_use]
    pub const fn whole(self) -> bool {
        !matches!(self, Self::Selection)
    }
}

/// How many pairs one thing removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[must_use]
    pub fn of(document: &RunDocument) -> Self {
        let targets = u32::try_from(document.targets.len()).unwrap_or(u32::MAX);
        let cataloged = u32::try_from(document.mutants.len()).unwrap_or(u32::MAX);
        let held: BTreeMap<&str, u64> = document
            .targets
            .iter()
            .map(|target| (target.id.as_str(), u64::from(target.tests.max(1))))
            .collect();
        let every: u64 = held.values().sum();
        let mut started: u64 = 0;
        let mut tests_started: u64 = document.established_tests;
        let mut removed: BTreeMap<String, (Removal, u64, u32)> = BTreeMap::new();
        for mutant in &document.mutants {
            started = started.saturating_add(processes(mutant));
            tests_started = tests_started.saturating_add(tests(mutant, &held));
            for (reason, removal, pairs) in per_mutant(mutant, u64::from(targets)) {
                let entry = removed.entry(reason).or_insert((removal, 0, 0));
                entry.1 = entry.1.saturating_add(pairs);
                entry.2 = entry.2.saturating_add(1);
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
        Self {
            targets,
            cataloged,
            whole: u64::from(cataloged).saturating_mul(u64::from(targets)),
            started,
            tests_whole: u64::from(cataloged).saturating_mul(every),
            tests_started,
            established: document.established_tests,
            removed,
        }
    }

    /// Whether every pair a whole run would have started is one this run started or one something named removed.
    ///
    /// A retry is work beyond the whole, so it is added back before the two are
    /// compared: the identity is about pairs, and a pair asked twice is still
    /// one pair.
    #[must_use]
    pub fn balances(&self) -> bool {
        self.pairs().saturating_add(self.skipped()) == self.whole
    }

    /// The pairs a process was started for, counting a pair asked twice once.
    #[must_use]
    pub fn pairs(&self) -> u64 {
        self.started.min(self.whole)
    }

    /// Every pair something removed.
    #[must_use]
    pub fn skipped(&self) -> u64 {
        self.removed.iter().map(|one| one.pairs).sum()
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
        let widened =
            |count: u64| u32::try_from(count).map_or_else(|_| f64::from(u32::MAX), f64::from);
        widened(self.tests_whole.saturating_sub(self.tests_started)) / widened(self.tests_whole)
    }

    /// The share of a whole run this one did not do, between 0 and 1.
    #[must_use]
    pub fn saved(&self) -> f64 {
        if self.whole == 0 {
            return 0.0;
        }
        let widened =
            |count: u64| u32::try_from(count).map_or_else(|_| f64::from(u32::MAX), f64::from);
        widened(self.skipped()) / widened(self.whole)
    }

    /// Whether every removal leaves the verdict where a whole run would have left it.
    #[must_use]
    pub fn answers_for_the_whole(&self) -> bool {
        self.removed.iter().all(|one| one.removal.whole())
    }
}

/// How many processes one mutant cost.
fn processes(mutant: &RunMutantDocument) -> u64 {
    if mutant.source_run_id.is_some() {
        return 0;
    }
    let executed = mutant
        .route
        .as_ref()
        .map_or(0, |route| route.executed.len());
    let executed = u64::try_from(executed).unwrap_or(u64::MAX);
    executed.saturating_add(u64::from(mutant.retried))
}

/// How many tests one mutant cost, which is what each target it ran was asked for.
///
/// A target a route narrowed to some of its tests was asked for exactly those;
/// one it did not narrow was asked for every test that target has. A retry
/// puts the same question to the target that answered, so it costs what that
/// target was asked for again.
fn tests(mutant: &RunMutantDocument, held: &BTreeMap<&str, u64>) -> u64 {
    if mutant.source_run_id.is_some() {
        return 0;
    }
    let Some(route) = mutant.route.as_ref() else {
        return 0;
    };
    let asked = |target: &str| -> u64 {
        route.tests.get(target).map_or_else(
            || held.get(target).copied().unwrap_or(1),
            |named| u64::try_from(named.len()).unwrap_or(u64::MAX),
        )
    };
    let walked: u64 = route
        .executed
        .iter()
        .map(|target| asked(target))
        .fold(0, u64::saturating_add);
    let again = if mutant.retried {
        asked(&mutant.target)
    } else {
        0
    };
    walked.saturating_add(again)
}

/// What removed each of one mutant's pairs, and what kind of removal it was.
fn per_mutant(mutant: &RunMutantDocument, targets: u64) -> Vec<(String, Removal, u64)> {
    if mutant.source_run_id.is_some() {
        return vec![(REUSED.to_owned(), Removal::Memory, targets)];
    }
    let Some(route) = mutant.route.as_ref() else {
        let reason = mutant
            .not_run_reason
            .clone()
            .unwrap_or_else(|| UNREACHED.to_owned());
        return vec![(reason.clone(), kind_of(&reason), targets)];
    };
    let reaching = u64::try_from(route.reaching.len()).unwrap_or(u64::MAX);
    let executed = u64::try_from(route.executed.len()).unwrap_or(u64::MAX);
    let mut removed: Vec<(String, Removal, u64)> = Vec::new();
    let mut by_proof: BTreeMap<&str, u64> = BTreeMap::new();
    for discharge in &route.discharged {
        let counted = by_proof.entry(discharge.proof.as_str()).or_insert(0);
        *counted = counted.saturating_add(1);
    }
    let discharged: u64 = by_proof.values().sum();
    for (proof, pairs) in by_proof {
        removed.push((proof.to_owned(), Removal::Proof, pairs));
    }
    let unreached = targets.saturating_sub(reaching).saturating_sub(discharged);
    if unreached > 0 {
        removed.push((UNREACHED.to_owned(), Removal::Proof, unreached));
    }
    let unasked = reaching.saturating_sub(executed);
    if unasked > 0 {
        let reason = mutant
            .not_run_reason
            .clone()
            .unwrap_or_else(|| ANSWERED.to_owned());
        removed.push((reason.clone(), kind_of(&reason), unasked));
    }
    removed
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
