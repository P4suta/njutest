// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run established along each dimension it can measure, read off the records it already holds (ADR 0033).

use super::faults::{FaultDecision, FaultRecord};
use super::knobs::{KnobRecord, Standing};
use super::{Finding, FindingKind, Limitation, SeamDecision, SeamRecord};

/// One way a run perturbs what it measures, and so one column of the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Dimension {
    /// What a mutation of the source changes.
    Mutation,
    /// What a knob sets differently for a control.
    Repeatable,
    /// What a failed call does.
    Fault,
    /// What another schedule of the same threads does.
    Schedule,
    /// What a seam is asked.
    Wire,
    /// What a crash at a persistence point leaves.
    Durable,
}

impl Dimension {
    /// The name the record stream and a finding spell it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mutation => "mutation",
            Self::Repeatable => "repeatable",
            Self::Fault => "fault",
            Self::Schedule => "schedule",
            Self::Wire => "wire",
            Self::Durable => "durable",
        }
    }
}

/// What a run established along one dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Column {
    /// The run measured it.
    Measured {
        /// What the dimension could have asked.
        catalogued: usize,
        /// What it decided.
        answered: usize,
        /// What it put and could not decide.
        holes: usize,
        /// Each class the dimension cannot put at all, named; never a hole.
        speaks_not_about: Vec<String>,
    },
    /// The run was asked to measure it and could not measure it at all.
    Unmeasured {
        /// What stood in the way.
        why: String,
    },
    /// The run could have measured it and was not asked to.
    NotAsked,
    /// The run was asked and found nothing it could ask.
    NothingToAsk {
        /// What was found to be absent.
        why: String,
    },
    /// No measurement of the dimension exists in this release.
    NotInThisRelease,
}

impl Column {
    /// The name the record stream spells the column's state by.
    #[must_use]
    pub const fn state(&self) -> &'static str {
        match self {
            Self::Measured { .. } => "measured",
            Self::Unmeasured { .. } => "unmeasured",
            Self::NotAsked => "not-asked",
            Self::NothingToAsk { .. } => "nothing-to-ask",
            Self::NotInThisRelease => "not-in-this-release",
        }
    }

    /// Why a run that asks every dimension is not assured along this one, where it is not.
    #[must_use]
    pub fn hole(&self, dimension: Dimension) -> Option<String> {
        let name = dimension.name();
        match self {
            Self::Measured { holes: 0, .. } | Self::NothingToAsk { .. } => None,
            Self::Measured { holes, .. } => Some(format!(
                "{holes} thing(s) the {name} dimension put were not decided, so it is not \
                 established along it"
            )),
            Self::Unmeasured { why } => Some(format!(
                "the {name} dimension was asked and could not be measured: {why}"
            )),
            Self::NotAsked => Some(format!(
                "the {name} dimension was not asked, and a run that asks every dimension \
                 establishes nothing along one it did not measure"
            )),
            Self::NotInThisRelease => Some(format!(
                "{name} is not in this release; whole-v1 cannot be satisfied until it is"
            )),
        }
    }
}

/// One column, with the dimension it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The dimension.
    pub dimension: Dimension,
    /// What the run established along it.
    pub column: Column,
}

/// The records one run, or every part of one build, holds about what it measured.
#[derive(Debug, Clone, Copy)]
pub struct Evidence<'a> {
    /// How many mutations the run decided, and how many it put and could not decide.
    pub mutations: (usize, usize),
    /// What each knob established about each target.
    pub knobs: &'a [KnobRecord],
    /// What each fault site came to.
    pub faults: &'a [FaultRecord],
    /// What each seam question came to.
    pub seams: &'a [SeamRecord],
    /// What the run said it does not claim.
    pub limitations: &'a [Limitation],
    /// What the run found.
    pub findings: &'a [Finding],
}

impl<'a> Evidence<'a> {
    /// What one run's own report holds.
    #[must_use]
    pub fn of(report: &'a super::BuildReport) -> Self {
        let holes = report
            .mutants
            .iter()
            .filter(|mutant| unsettled(mutant.outcome.outcome()))
            .count();
        let answered = report
            .mutants
            .iter()
            .filter(|mutant| !unsettled(mutant.outcome.outcome()))
            .count();
        Self {
            mutations: (answered, holes),
            knobs: &report.knobs,
            faults: &report.faults,
            seams: &report.seams,
            limitations: &report.limitations,
            findings: &report.findings,
        }
    }
}

/// Whether a mutation's outcome is one the run put and could not decide.
#[must_use]
pub const fn unsettled(outcome: super::Outcome) -> bool {
    match outcome {
        super::Outcome::Waited
        | super::Outcome::StepLimitReached
        | super::Outcome::Unconfirmed
        | super::Outcome::Errored => true,
        super::Outcome::CompileRejected
        | super::Outcome::Killed
        | super::Outcome::ModelNoticed
        | super::Outcome::ModelProved
        | super::Outcome::Survived
        | super::Outcome::Unreached
        | super::Outcome::Equivalent => false,
    }
}

/// The matrix: one row for every dimension, in the order a reader meets them.
#[must_use]
pub fn rows(evidence: &Evidence<'_>) -> Vec<Row> {
    Dimension::ALL
        .into_iter()
        .map(|dimension| Row {
            dimension,
            column: column(dimension, evidence),
        })
        .collect()
}

/// One finding for every dimension a run that asks every dimension is not assured along.
#[must_use]
pub fn holes(rows: &[Row]) -> Vec<Finding> {
    rows.iter()
        .filter_map(|row| {
            row.column.hole(row.dimension).map(|why| {
                Finding::new(
                    FindingKind::DimensionNotMeasured,
                    row.dimension.name(),
                    &why,
                )
            })
        })
        .collect()
}

/// What the run established along `dimension`.
fn column(dimension: Dimension, evidence: &Evidence<'_>) -> Column {
    match dimension {
        Dimension::Mutation => {
            let (answered, holes) = evidence.mutations;
            measured(answered, holes, named(evidence.limitations, "skipped-"))
        }
        Dimension::Repeatable => repeatable(evidence.knobs),
        Dimension::Fault => fault(evidence),
        Dimension::Schedule | Dimension::Durable => Column::NotInThisRelease,
        Dimension::Wire => wire(evidence),
    }
}

/// A measured column, or an unmeasured one where its counts do not add up to a count.
fn measured(answered: usize, holes: usize, speaks_not_about: Vec<String>) -> Column {
    answered.checked_add(holes).map_or_else(
        || Column::Unmeasured {
            why: "more was put than a count can hold".to_owned(),
        },
        |catalogued| Column::Measured {
            catalogued,
            answered,
            holes,
            speaks_not_about,
        },
    )
}

/// The knobs' column.
fn repeatable(knobs: &[KnobRecord]) -> Column {
    if knobs.is_empty() {
        return Column::NotAsked;
    }
    let answered = knobs
        .iter()
        .filter(|record| {
            matches!(
                record.standing,
                Standing::Stable
                    | Standing::Passed
                    | Standing::Broke { .. }
                    | Standing::Moved { .. }
            )
        })
        .count();
    let holes = knobs
        .iter()
        .filter(|record| {
            matches!(
                record.standing,
                Standing::Uncompared { .. } | Standing::Unsettled { .. }
            )
        })
        .count();
    let speaks_not_about = knobs
        .iter()
        .filter_map(|record| match &record.standing {
            Standing::NotPut { why } => Some(format!(
                "{} on {} ({})",
                record.knob.name(),
                record.target,
                why.said()
            )),
            Standing::Stable
            | Standing::Passed
            | Standing::Broke { .. }
            | Standing::Moved { .. }
            | Standing::Uncompared { .. }
            | Standing::Unsettled { .. } => None,
        })
        .collect();
    measured(answered, holes, speaks_not_about)
}

/// The faults' column.
fn fault(evidence: &Evidence<'_>) -> Column {
    if evidence.faults.is_empty() {
        if evidence
            .findings
            .iter()
            .any(|finding| finding.subject == crate::assure::faults::NOT_MEASURED)
        {
            return Column::Unmeasured {
                why: "the tree with every fault site guarded gave no baseline".to_owned(),
            };
        }
        if evidence
            .limitations
            .iter()
            .any(|limitation| limitation.name == crate::limitation::FAULT_NO_SITE)
        {
            return Column::NothingToAsk {
                why: "no measured file has a `?`".to_owned(),
            };
        }
        return Column::NotAsked;
    }
    let answered = evidence
        .faults
        .iter()
        .filter(|record| {
            matches!(
                record.decision,
                FaultDecision::Noticed { .. } | FaultDecision::Unnoticed | FaultDecision::Unreached
            )
        })
        .count();
    let holes = evidence
        .faults
        .iter()
        .filter(|record| {
            matches!(
                record.decision,
                FaultDecision::Waited { .. } | FaultDecision::Undecided { .. }
            )
        })
        .count();
    let speaks_not_about = evidence
        .faults
        .iter()
        .filter(|record| matches!(record.decision, FaultDecision::NotPut { .. }))
        .map(|record| {
            format!(
                "an error type the engine does not make, at {}",
                record.place()
            )
        })
        .collect();
    measured(answered, holes, speaks_not_about)
}

/// The seams' column.
fn wire(evidence: &Evidence<'_>) -> Column {
    let unwatched = named(evidence.limitations, crate::limitation::SEAM_NOT_WATCHED).len();
    let answered = evidence
        .seams
        .iter()
        .filter(|seam| seam.decision != SeamDecision::Unreached)
        .count();
    let unreached = evidence
        .seams
        .iter()
        .filter(|seam| seam.decision == SeamDecision::Unreached)
        .count();
    unwatched.checked_add(unreached).map_or_else(
        || Column::Unmeasured {
            why: "more was put than a count can hold".to_owned(),
        },
        |holes| {
            measured(
                answered,
                holes,
                vec![
                    "a seam the configuration does not name, since a seam is only ever one it \
                     names"
                        .to_owned(),
                ],
            )
        },
    )
}

/// Every limitation whose name starts with `prefix`, by name.
fn named(limitations: &[Limitation], prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = limitations
        .iter()
        .filter(|limitation| limitation.name.starts_with(prefix))
        .map(|limitation| limitation.name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}
