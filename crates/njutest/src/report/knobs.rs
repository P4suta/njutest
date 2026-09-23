// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each measured target's verdict and reach hold when one thing the contract lets differ between machines is set differently for a control of it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::drift::{Moved, Unmeasured};
use super::{Finding, FindingKind, Limitation, MutantRecord};

/// One thing the contract lets differ between machines, which a run can set differently for a control.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Knob {
    /// The time zone.
    Timezone,
    /// The locale.
    Locale,
    /// The temporary directory.
    TempDirectory,
    /// The home directory.
    Home,
    /// The mask new files are created under.
    Umask,
    /// How wide the terminal is.
    Columns,
    /// How many tests the harness runs at once.
    Threads,
}

impl Knob {
    /// The name the configuration and a report spell it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Timezone => "timezone",
            Self::Locale => "locale",
            Self::TempDirectory => "temp-directory",
            Self::Home => "home",
            Self::Umask => "umask",
            Self::Columns => "columns",
            Self::Threads => "threads",
        }
    }

    /// The knob named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|knob| knob.name() == name)
    }

    /// What a control is started with when the knob is put, as a reader types it to see the same.
    #[must_use]
    pub const fn put(self) -> &'static str {
        match self {
            Self::Timezone => "TZ=Australia/Lord_Howe",
            Self::Locale => "LC_ALL=tr_TR.UTF-8",
            Self::TempDirectory => "TMPDIR set to an empty directory whose path holds a space",
            Self::Home => "HOME set to an empty directory, with cargo's and rustup's kept",
            Self::Umask => "umask 077",
            Self::Columns => "COLUMNS=37 LINES=11",
            Self::Threads => "--test-threads=1",
        }
    }
}

/// Why a knob asked for was not put for a target.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum NotPut {
    /// This platform has no way to put it.
    Platform,
    /// The time zone database has no zone of that name, so the process would run in UTC and a pass would say nothing.
    ZoneMissing,
    /// The locale is not installed, so the process would run in the C locale and a pass would say nothing.
    LocaleMissing,
    /// There is no shell to set the mask through.
    ShellMissing,
    /// The target runs through cargo, which reads the variable itself, so a failure would be about the toolchain.
    ThroughCargo,
    /// The target does not run under libtest, which is the only harness the argument is one for.
    NotLibtest,
}

impl NotPut {
    /// Why, as a clause a reader is told.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Platform => "this platform has no way to put it",
            Self::ZoneMissing => {
                "the time zone database has no Australia/Lord_Howe, so the process would run in \
                 UTC and a pass would say nothing"
            }
            Self::LocaleMissing => {
                "tr_TR.UTF-8 is not installed, so the process would run in the C locale and a pass \
                 would say nothing"
            }
            Self::ShellMissing => "there is no sh to set the mask through",
            Self::ThroughCargo => {
                "it runs through cargo, which reads the variable itself, so a failure would be \
                 about the toolchain"
            }
            Self::NotLibtest => {
                "it does not run under libtest, the only harness the argument is one for"
            }
        }
    }
}

/// Why a control under a knob established nothing about the target.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Unsettled {
    /// The control could not be measured.
    Errored,
    /// The control ran past its bound.
    Waited,
}

impl Unsettled {
    /// Why, as a clause a reader is told.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Errored => "the control could not be measured",
            Self::Waited => "the control ran past its bound",
        }
    }
}

/// What a control under a knob established about one target, against its baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Standing {
    /// It passed the tests its baseline passed and reached what its baseline did.
    Stable,
    /// It passed where its baseline passed, and is a target that records no reach to compare, as a doctest run through cargo is.
    Passed,
    /// It failed where its baseline passed.
    Broke {
        /// The tests that failed, as the harness named them.
        failed: Vec<String>,
    },
    /// It passed the tests its baseline passed and reached something else.
    Moved {
        /// What it reached that its baseline did not, and the reverse.
        reach: Box<Reach>,
    },
    /// It passed, and what it reached was not compared.
    Uncompared {
        /// Why.
        why: Unmeasured,
    },
    /// The control established nothing.
    Unsettled {
        /// Why.
        why: Unsettled,
    },
    /// The knob was not put.
    NotPut {
        /// Why.
        why: NotPut,
    },
}

/// What a control reached that its baseline did not, and the reverse, over the three unions drift compares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reach {
    /// The mutant sites.
    pub reached: Moved,
    /// The branch bodies entered.
    pub bodies: Moved,
    /// The mutations a guard saw its two branches differ over.
    pub infected: Moved,
}

/// What one knob established about one measured target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnobRecord {
    /// The target.
    pub target: String,
    /// The knob.
    pub knob: Knob,
    /// What its control established.
    pub standing: Standing,
}

/// The findings the records earn: an `environment-dependent` defect for each target a knob broke, and an `environment-dependent-reach` gap for each whose reach it moved, counting the dispositions resting on that target's baseline over `records`, which must be the whole catalog.
#[must_use]
pub fn found(knobs: &[KnobRecord], records: &[MutantRecord]) -> Vec<Finding> {
    knobs
        .iter()
        .filter_map(|one| match &one.standing {
            Standing::Broke { failed } => Some(Finding::new(
                FindingKind::EnvironmentDependent,
                &one.target,
                &broke(one, failed),
            )),
            Standing::Moved { .. } => {
                let (discharged, unreached) = super::drift::resting(records, &one.target);
                Some(Finding::new(
                    FindingKind::EnvironmentDependentReach,
                    &one.target,
                    &format!(
                        "{} reached something else with {} than on its baseline, over the same \
                         passing tests, so what it reaches depends on something that differs \
                         between machines, and every proof read off its baseline is unfounded \
                         where that differs: {} a proof removed its run of, and {} no test \
                         reached, rest on it",
                        one.target,
                        one.knob.put(),
                        super::drift::mutations(discharged),
                        super::drift::mutations(unreached),
                    ),
                ))
            }
            Standing::Stable
            | Standing::Passed
            | Standing::Uncompared { .. }
            | Standing::Unsettled { .. }
            | Standing::NotPut { .. } => None,
        })
        .collect()
}

/// The sentence of an `environment-dependent` finding.
fn broke(one: &KnobRecord, failed: &[String]) -> String {
    let named = if failed.is_empty() {
        "its harness named no test".to_owned()
    } else {
        failed.join(", ")
    };
    format!(
        "{} passed on its baseline and failed with {}: {named}. What it answers depends on \
         something that differs between machines; set it in the test, or make the code not read \
         it, and run again",
        one.target,
        one.knob.put(),
    )
}

/// The limitations the records earn: each knob that was asked for and not put, and each control under a knob that compared nothing, with the targets and why.
#[must_use]
pub fn limited(knobs: &[KnobRecord]) -> Vec<Limitation> {
    let mut unput: BTreeMap<(Knob, NotPut), Vec<&str>> = BTreeMap::new();
    let mut uncompared: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for one in knobs {
        match &one.standing {
            Standing::NotPut { why } => unput
                .entry((one.knob, *why))
                .or_default()
                .push(one.target.as_str()),
            Standing::Uncompared { why } => uncompared
                .entry(one.target.as_str())
                .or_default()
                .push(format!("{} ({})", one.knob.name(), why.said())),
            Standing::Unsettled { why } => uncompared
                .entry(one.target.as_str())
                .or_default()
                .push(format!("{} ({})", one.knob.name(), why.said())),
            Standing::Stable
            | Standing::Passed
            | Standing::Broke { .. }
            | Standing::Moved { .. } => {}
        }
    }
    let mut limitations: Vec<Limitation> = unput
        .into_iter()
        .map(|((knob, why), targets)| {
            Limitation::new(
                crate::limitation::KNOB_NOT_PUT,
                &format!(
                    "{} was asked for and not put on {}, because {}, so nothing is claimed about \
                     whether {} on it",
                    knob.name(),
                    targets.join(", "),
                    why.said(),
                    if targets.len() == 1 {
                        "that target depends"
                    } else {
                        "those targets depend"
                    },
                ),
            )
        })
        .collect();
    limitations.extend(uncompared.into_iter().map(|(target, knobs)| {
        Limitation::new(
            crate::limitation::KNOB_NOT_COMPARED,
            &format!(
                "the controls of {target} under {} established nothing to compare, so whether its \
                 verdict and reach hold there is not known",
                knobs.join(" and "),
            ),
        )
    }));
    limitations
}
