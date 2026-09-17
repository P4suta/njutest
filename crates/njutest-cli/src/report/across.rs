// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one mutation stands on when more than one build of the project measured it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::{Decision, MutantRecord, Report};

/// What a run records about one mutation, taken across every build that measured it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What the run records, which is the weakest thing any build established.
    pub decision: Decision,
    /// The builds under which nothing noticed it, in the order a report lists them. A run of one build names none: which build is not a question it has.
    pub unnoticed_in: Vec<String>,
}

/// What `by_build` leaves one mutation standing on, which is the weakest of what its builds established.
#[must_use]
pub fn across(by_build: &BTreeMap<String, Decision>) -> Resolved {
    let decision = by_build
        .values()
        .copied()
        .min_by_key(|decision| decision.standing())
        .unwrap_or(Decision::Undecided);
    let unnoticed_in = by_build
        .iter()
        .filter(|(_, held)| **held == Decision::Unnoticed)
        .map(|(name, _)| name.clone())
        .collect();
    Resolved {
        decision,
        unnoticed_in,
    }
}

/// Why the builds a run measured are not builds of one catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfiguredError {
    /// No build was measured, so there is nothing to reconcile.
    Nothing,
    /// A mutation one build catalogued and another did not.
    CataloguesDiffer {
        /// The mutation.
        mutant: String,
        /// The build that did not catalogue it.
        build: String,
    },
}

impl fmt::Display for ConfiguredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nothing => write!(
                f,
                "no build was measured, and a report of none of them would say a run \
                 established something it never looked at"
            ),
            Self::CataloguesDiffer { mutant, build } => write!(
                f,
                "the build {build:?} did not catalogue the mutation {mutant}, so the \
                 run cannot answer for it across the builds; taking the answers it \
                 does have would report that build's silence as agreement"
            ),
        }
    }
}

impl std::error::Error for ConfiguredError {}

/// The report the run writes from the builds it measured, where a mutation stands on the weakest of them.
///
/// # Errors
/// [`ConfiguredError`]: nothing to reconcile, or builds that did not
/// catalogue the same mutations.
pub fn configured(measured: &[(String, Report)]) -> Result<Report, ConfiguredError> {
    let (_, first) = measured.first().ok_or(ConfiguredError::Nothing)?;
    same_catalogue(measured)?;
    let mut whole = first.clone();
    whole.mutants = first
        .mutants
        .iter()
        .map(|record| weakest(record, measured))
        .collect();
    whole.accounting.mutants.observers = counted(&whole.mutants);
    whole.verdict = whole.concluded();
    Ok(whole)
}

/// Whether every build catalogued the same mutations, which is what makes them builds of one catalog.
fn same_catalogue(measured: &[(String, Report)]) -> Result<(), ConfiguredError> {
    let Some((_, first)) = measured.first() else {
        return Ok(());
    };
    let expected: BTreeSet<&str> = first.mutants.iter().map(|one| one.id.as_str()).collect();
    for (build, part) in measured {
        let held: BTreeSet<&str> = part.mutants.iter().map(|one| one.id.as_str()).collect();
        if let Some(missing) = expected.difference(&held).next() {
            return Err(ConfiguredError::CataloguesDiffer {
                mutant: (*missing).to_owned(),
                build: build.clone(),
            });
        }
        if let Some(extra) = held.difference(&expected).next() {
            return Err(ConfiguredError::CataloguesDiffer {
                mutant: (*extra).to_owned(),
                build: measured
                    .first()
                    .map_or_else(String::new, |(name, _)| name.clone()),
            });
        }
    }
    Ok(())
}

/// The record of whichever build left this mutation standing on least, carrying the builds nothing noticed it in.
fn weakest(record: &MutantRecord, measured: &[(String, Report)]) -> MutantRecord {
    let by_build: BTreeMap<String, Decision> = measured
        .iter()
        .filter_map(|(build, part)| {
            let held = part.mutants.iter().find(|one| one.id == record.id)?;
            Some((build.clone(), decision_of(held)))
        })
        .collect();
    let resolved = if measured.len() > 1 {
        across(&by_build)
    } else {
        Resolved {
            decision: decision_of(record),
            unnoticed_in: Vec::new(),
        }
    };
    let weakest = measured
        .iter()
        .filter_map(|(_, part)| part.mutants.iter().find(|one| one.id == record.id))
        .min_by_key(|one| decision_of(one).standing())
        .unwrap_or(record);
    MutantRecord {
        unnoticed_in: resolved.unnoticed_in,
        ..weakest.clone()
    }
}

/// What decided one record, where an outcome no decision is spelled for decided nothing.
fn decision_of(record: &MutantRecord) -> Decision {
    Decision::of_outcome(&record.outcome).unwrap_or(Decision::Undecided)
}

/// The columns the reconciled records add up to.
fn counted(records: &[MutantRecord]) -> super::ObserverAccounting {
    let mut counts = super::ObserverAccounting::default();
    for record in records {
        counts.counted(decision_of(record));
    }
    counts
}
