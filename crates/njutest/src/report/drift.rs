// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each measured target reached, on an original-code control, what it reached on its baseline (ADR 0025).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{Finding, FindingKind, Limitation, MutantRecord, Outcome};

/// What one kind of report a control made that the baseline did not, and the reverse, by catalog index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moved {
    /// What only the control reported.
    pub gained: BTreeSet<u32>,
    /// What only the baseline reported.
    pub lost: BTreeSet<u32>,
}

impl Moved {
    fn of(moved: &rust_mutants::touch::Moved) -> Self {
        Self {
            gained: moved.gained.clone(),
            lost: moved.lost.clone(),
        }
    }
}

/// Why a target's baseline reach was not compared with a control's.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Unmeasured {
    /// No control of the whole target ran, which is what a target nothing was confirmed on has.
    NoControl,
    /// The control's process could not record what its guards reached.
    Unrecorded,
    /// The control's record did not read back.
    Unreadable,
    /// The control did not pass, so its reach is the reach of a failing run.
    ControlFailed,
    /// The control passed other tests than the baseline did, so its reach is the reach of other tests.
    OtherTests,
    /// The baseline recorded nothing for the target to be compared against.
    NoBaseline,
    /// The baseline passed only when run again in the directory its first attempt left, so it did not run under the conditions a control does.
    BaselineRetried,
    /// The tests one of the two runs was read as passing do not come to its own summary's count, so which tests passed is the parser's answer and not the harness's.
    Unparsed,
}

impl Unmeasured {
    /// Why nothing was compared, as a clause a reader is told.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::NoControl => "no control of the whole target ran",
            Self::Unrecorded => "the control could not record what its guards reached",
            Self::Unreadable => "the control's record did not read back",
            Self::ControlFailed => "the control did not pass",
            Self::OtherTests => "the control passed other tests than its baseline did",
            Self::NoBaseline => "the baseline recorded nothing to compare against",
            Self::BaselineRetried => "the baseline passed only when run again",
            Self::Unparsed => "the tests a run was read as passing do not come to its own count",
        }
    }

    const fn of(why: rust_mutants::touch::Unmeasured) -> Self {
        match why {
            rust_mutants::touch::Unmeasured::Unrecorded => Self::Unrecorded,
            rust_mutants::touch::Unmeasured::Unreadable => Self::Unreadable,
            rust_mutants::touch::Unmeasured::ControlFailed => Self::ControlFailed,
            rust_mutants::touch::Unmeasured::OtherTests => Self::OtherTests,
            rust_mutants::touch::Unmeasured::NoBaseline => Self::NoBaseline,
            rust_mutants::touch::Unmeasured::BaselineRetried => Self::BaselineRetried,
            rust_mutants::touch::Unmeasured::Unparsed => Self::Unparsed,
        }
    }
}

/// What a run established about whether one measured target's baseline reach holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Drift {
    /// A control that passed the tests the baseline passed reached exactly what the baseline did.
    Held {
        /// The target.
        target: String,
    },
    /// A control that passed the tests the baseline passed reached something else.
    Moved {
        /// The target.
        target: String,
        /// The mutant sites.
        reached: Moved,
        /// The branch bodies entered, by the marker at each body's first statement.
        bodies: Moved,
        /// The mutations a guard saw its two branches differ over.
        infected: Moved,
    },
    /// Nothing about it was compared.
    NotMeasured {
        /// The target.
        target: String,
        /// Why.
        why: Unmeasured,
    },
}

impl Drift {
    /// The target this is about.
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Self::Held { target }
            | Self::Moved { target, .. }
            | Self::NotMeasured { target, .. } => target,
        }
    }

    /// What the engine's control established about `target`.
    #[must_use]
    pub fn of(target: &str, steadiness: &rust_mutants::touch::Steadiness) -> Self {
        let target = target.to_owned();
        match steadiness {
            rust_mutants::touch::Steadiness::Held => Self::Held { target },
            rust_mutants::touch::Steadiness::Moved(moved) => Self::Moved {
                target,
                reached: Moved::of(&moved.reached),
                bodies: Moved::of(&moved.bodies),
                infected: Moved::of(&moved.infected),
            },
            rust_mutants::touch::Steadiness::NotMeasured(why) => Self::NotMeasured {
                target,
                why: Unmeasured::of(*why),
            },
        }
    }

    /// How much this establishes, so that of two observations of one target the one that says more stands: a move over a hold, a hold over a control that compared nothing, and that over no control at all.
    const fn weight(&self) -> u8 {
        match self {
            Self::NotMeasured {
                why: Unmeasured::NoControl,
                ..
            } => 0,
            Self::NotMeasured {
                why:
                    Unmeasured::Unrecorded
                    | Unmeasured::Unreadable
                    | Unmeasured::ControlFailed
                    | Unmeasured::OtherTests
                    | Unmeasured::NoBaseline
                    | Unmeasured::BaselineRetried
                    | Unmeasured::Unparsed,
                ..
            } => 1,
            Self::Held { .. } => 2,
            Self::Moved { .. } => 3,
        }
    }
}

/// One record per target the baseline measured, folding every observation of each: a move one control saw is not undone by another that did not see it.
#[must_use]
pub fn folded<'a>(
    measured: impl IntoIterator<Item = &'a str>,
    observed: impl IntoIterator<Item = Drift>,
) -> Vec<Drift> {
    let mut held: BTreeMap<String, Drift> = measured
        .into_iter()
        .map(|target| {
            (
                target.to_owned(),
                Drift::NotMeasured {
                    target: target.to_owned(),
                    why: Unmeasured::NoControl,
                },
            )
        })
        .collect();
    for one in observed {
        let Some(standing) = held.get_mut(one.target()) else {
            continue;
        };
        if one.weight() > standing.weight() {
            *standing = one;
        }
    }
    held.into_values().collect()
}

/// The records of every part of one build as one record per target, which is what a merge raises its finding and limitation from.
#[must_use]
pub fn combined<'a>(records: impl IntoIterator<Item = &'a Drift>) -> Vec<Drift> {
    let records: Vec<&Drift> = records.into_iter().collect();
    let targets: BTreeSet<&str> = records.iter().map(|one| one.target()).collect();
    folded(targets, records.into_iter().cloned())
}

/// The `unstable-baseline` finding each moved target earns, counting the dispositions a proof decided on its baseline over `records`, which must be the whole catalog.
#[must_use]
pub fn found(drift: &[Drift], records: &[MutantRecord]) -> Vec<Finding> {
    drift
        .iter()
        .filter_map(|one| match one {
            Drift::Moved { target, .. } => Some(target.as_str()),
            Drift::Held { .. } | Drift::NotMeasured { .. } => None,
        })
        .map(|target| {
            let (discharged, unreached) = resting(records, target);
            Finding::new(
                FindingKind::UnstableBaseline,
                target,
                &unstable(target, discharged, unreached),
            )
        })
        .collect()
}

/// How many of `records` a proof decided on `target`'s baseline: survivors whose route did not put them to it, and mutations no test reached, each of which says it reached nothing.
#[must_use]
pub fn resting(records: &[MutantRecord], target: &str) -> (usize, usize) {
    let discharged = records
        .iter()
        .filter(|record| record.outcome.outcome() == Outcome::Survived)
        .filter(|record| rests_on(record.routing.as_ref(), target))
        .count();
    let unreached = records
        .iter()
        .filter(|record| record.outcome.outcome() == Outcome::Unreached)
        .count();
    (discharged, unreached)
}

/// How many mutations, in the words a finding counts them in.
#[must_use]
pub fn mutations(count: usize) -> String {
    if count == 1 {
        "1 mutation".to_owned()
    } else {
        format!("{count} mutations")
    }
}

/// Whether a mutation's standing rests on `target`'s baseline reach: its route did not put it to `target`, so a discharge or the reach itself removed `target`, or it carries no route this run decided and so nothing this run can vouch for.
#[must_use]
pub fn rests_on(routing: Option<&super::Routing>, target: &str) -> bool {
    routing.is_none_or(|routing| !routing.reaching.iter().any(|one| one == target))
}

/// The sentence of an `unstable-baseline` finding.
fn unstable(target: &str, discharged: usize, unreached: usize) -> String {
    format!(
        "{target} reached something on an original-code control that it did not reach on its \
         baseline, over the same passing tests, so what it reaches is not a function of the \
         target and every proof read off its baseline is unfounded: {} a proof removed its run \
         of, and {} no test reached, rest on it. Make what the suite reaches independent of \
         order, time and earlier processes, and run again",
        mutations(discharged),
        mutations(unreached),
    )
}

/// The `drift-not-measured` limitation, naming every target no comparable control recorded, or nothing where every one was compared.
#[must_use]
pub fn unmeasured(drift: &[Drift]) -> Option<Limitation> {
    let named: Vec<&str> = drift
        .iter()
        .filter_map(|one| match one {
            Drift::NotMeasured { target, .. } => Some(target.as_str()),
            Drift::Held { .. } | Drift::Moved { .. } => None,
        })
        .collect();
    if named.is_empty() {
        return None;
    }
    Some(Limitation::new(
        crate::limitation::DRIFT_NOT_MEASURED,
        &format!(
            "no original-code control over the tests its baseline passed recorded what {} \
             reached, so whether {} reach is a function of the target is not known and every \
             proof read off {} baseline rests on one run ({})",
            if named.len() == 1 {
                "1 target".to_owned()
            } else {
                format!("{} targets", named.len())
            },
            if named.len() == 1 { "its" } else { "their" },
            if named.len() == 1 { "its" } else { "their" },
            named.join(", ")
        ),
    ))
}
