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
                "the {name} dimension was not measured, and a run that asks every dimension \
                 establishes nothing along one it did not measure"
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
    /// What each call that writes came to under a crash.
    pub crashes: &'a [super::crashes::CrashRecord],
    /// What each test binary established about whether it runs one thread, and what exploring its schedules found.
    pub concurrency: &'a [super::concurrency::ConcurrencyRecord],
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
            crashes: &report.crashes,
            concurrency: &report.concurrency,
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
        Dimension::Mutation => match evidence.mutations {
            (0, 0) => Column::NothingToAsk {
                why: "no measured file has anything to mutate".to_owned(),
            },
            (answered, holes) => measured(answered, holes, named(evidence.limitations, "skipped-")),
        },
        Dimension::Repeatable => repeatable(evidence.knobs),
        Dimension::Fault => fault(evidence),
        Dimension::Schedule => schedule(evidence.concurrency),
        Dimension::Durable => durable(evidence),
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

/// Where one record of a dimension falls: decided, put and not decided, or of a class the dimension cannot put at all.
enum Counted {
    /// Decided.
    Answered,
    /// Put and not decided.
    Hole,
    /// Of a class the dimension does not speak about, named.
    SpeaksNotAbout(String),
}

/// A measured column from every record's place, each classified by one exhaustive match so that a decision added later is one somebody has to place.
fn tallied(counted: impl Iterator<Item = Counted>, mut speaks_not_about: Vec<String>) -> Column {
    let counted: Vec<Counted> = counted.collect();
    let answered = counted
        .iter()
        .filter(|one| matches!(one, Counted::Answered))
        .count();
    let holes = counted
        .iter()
        .filter(|one| matches!(one, Counted::Hole))
        .count();
    let put = !counted.is_empty();
    speaks_not_about.extend(counted.into_iter().filter_map(|one| match one {
        Counted::SpeaksNotAbout(class) => Some(class),
        Counted::Answered | Counted::Hole => None,
    }));
    if put && answered == 0 && holes == 0 {
        return Column::Unmeasured {
            why: format!(
                "every record is of a class the column does not speak about: {}",
                speaks_not_about.join("; ")
            ),
        };
    }
    measured(answered, holes, speaks_not_about)
}

/// The knobs' column.
fn repeatable(knobs: &[KnobRecord]) -> Column {
    if knobs.is_empty() {
        return Column::NotAsked;
    }
    tallied(
        knobs.iter().map(|record| match &record.standing {
            Standing::Stable
            | Standing::Passed
            | Standing::Broke { .. }
            | Standing::Moved { .. } => Counted::Answered,
            Standing::Uncompared { .. } | Standing::Unsettled { .. } => Counted::Hole,
            Standing::NotPut { why } if why.another_machine_could() => Counted::Hole,
            Standing::NotPut { why } => Counted::SpeaksNotAbout(format!(
                "{} on {} ({})",
                record.knob.name(),
                record.target,
                why.said()
            )),
        }),
        Vec::new(),
    )
}

/// The faults' column.
fn fault(evidence: &Evidence<'_>) -> Column {
    if evidence
        .findings
        .iter()
        .any(|finding| finding.subject == crate::assure::faults::NOT_MEASURED)
    {
        return Column::Unmeasured {
            why: "the tree with every fault site guarded gave no baseline".to_owned(),
        };
    }
    if evidence.faults.is_empty() {
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
    tallied(
        evidence.faults.iter().map(|record| match &record.decision {
            FaultDecision::Noticed { .. } | FaultDecision::Unnoticed | FaultDecision::Unreached => {
                Counted::Answered
            }
            FaultDecision::Waited { .. } | FaultDecision::Undecided { .. } => Counted::Hole,
            FaultDecision::NotPut { .. } => Counted::SpeaksNotAbout(format!(
                "an error type the engine does not make, at {}",
                record.place()
            )),
        }),
        Vec::new(),
    )
}

/// The schedules' column: a binary proven to run one thread, or one a delayed guard broke, is answered; a sample of schedules that all passed, one whose delays settled nothing, and a binary no schedule of which was explored, is a hole (ADR 0034).
fn schedule(records: &[super::concurrency::ConcurrencyRecord]) -> Column {
    use super::concurrency::{Exploration, Unexplored};
    if records.is_empty() {
        return Column::NothingToAsk {
            why: "no test binary ran".to_owned(),
        };
    }
    tallied(
        records.iter().map(|record| match &record.explored {
            Exploration::Unexplored {
                why: Unexplored::NotNeeded,
            }
            | Exploration::Broke { .. } => Counted::Answered,
            Exploration::Sampled { .. }
            | Exploration::Undecided { .. }
            | Exploration::Unexplored {
                why: Unexplored::NotAsked | Unexplored::NotPassing | Unexplored::NoSite,
            } => Counted::Hole,
        }),
        Vec::new(),
    )
}

/// The crashes' column.
fn durable(evidence: &Evidence<'_>) -> Column {
    use super::crashes::CrashDecision;
    if evidence
        .findings
        .iter()
        .any(|finding| finding.subject == crate::assure::crashes::NOT_MEASURED)
    {
        return Column::Unmeasured {
            why: "the tree with every call that writes guarded gave no baseline".to_owned(),
        };
    }
    if evidence.crashes.is_empty() {
        if evidence
            .limitations
            .iter()
            .any(|limitation| limitation.name == crate::limitation::CRASH_NO_SITE)
        {
            return Column::NothingToAsk {
                why: "no measured file calls anything that writes".to_owned(),
            };
        }
        return Column::NotAsked;
    }
    tallied(
        evidence
            .crashes
            .iter()
            .map(|record| match &record.decision {
                CrashDecision::Restarted { .. }
                | CrashDecision::Corrupt { .. }
                | CrashDecision::Unreached => Counted::Answered,
                CrashDecision::Unshared { .. } | CrashDecision::Undecided { .. } => Counted::Hole,
                CrashDecision::NotPut { .. } => Counted::SpeaksNotAbout(format!(
                    "a call the compiler would not stop after, at {}",
                    record.place()
                )),
            }),
        vec![
            "whether the next run read what the stop left".to_owned(),
            "writes the system had not yet flushed to disk".to_owned(),
        ],
    )
}

/// The seams' column: every question decided or not, and every configured seam that could not be watched as a hole of its own.
fn wire(evidence: &Evidence<'_>) -> Column {
    let unwatched = evidence
        .limitations
        .iter()
        .filter(|limitation| limitation.name == crate::limitation::SEAM_NOT_WATCHED)
        .map(|_unwatched| Counted::Hole);
    if evidence.seams.is_empty() && unwatched.clone().next().is_none() {
        return Column::NothingToAsk {
            why: "no seam was configured or asked a question".to_owned(),
        };
    }
    tallied(
        evidence
            .seams
            .iter()
            .map(|seam| match seam.decision {
                SeamDecision::Tests { .. }
                | SeamDecision::Proved { .. }
                | SeamDecision::Unnoticed => Counted::Answered,
                SeamDecision::Unreached => Counted::Hole,
            })
            .chain(unwatched),
        vec![
            "a seam the configuration does not name, since a seam is only ever one it names"
                .to_owned(),
        ],
    )
}

/// One column for several builds: every build's counts where each measured the dimension, and otherwise the column of the build that established least, so a hole in any build is a hole of all of them.
#[must_use]
pub fn pooled(columns: Vec<Column>) -> Column {
    let mut pooled: Option<Column> = None;
    for column in columns {
        pooled = Some(match (pooled, column) {
            (None, column) => column,
            (
                Some(Column::Measured {
                    catalogued,
                    answered,
                    holes,
                    mut speaks_not_about,
                }),
                Column::Measured {
                    catalogued: more,
                    answered: more_answered,
                    holes: more_holes,
                    speaks_not_about: more_classes,
                },
            ) => {
                speaks_not_about.extend(more_classes);
                speaks_not_about.dedup();
                match (
                    catalogued.checked_add(more),
                    answered.checked_add(more_answered),
                    holes.checked_add(more_holes),
                ) {
                    (Some(catalogued), Some(answered), Some(holes)) => Column::Measured {
                        catalogued,
                        answered,
                        holes,
                        speaks_not_about,
                    },
                    _ => Column::Unmeasured {
                        why: "more was put than a count can hold".to_owned(),
                    },
                }
            }
            (Some(one), other) => {
                if weight(&other) > weight(&one) {
                    other
                } else {
                    one
                }
            }
        });
    }
    pooled.unwrap_or(Column::NotAsked)
}

/// How little a column establishes, so the one that establishes least stands for several.
const fn weight(column: &Column) -> u8 {
    match column {
        Column::NothingToAsk { .. } => 0,
        Column::Measured { .. } => 1,
        Column::NotAsked => 2,
        Column::Unmeasured { .. } => 3,
    }
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
