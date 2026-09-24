// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run established about each call a `?` asks about, once a fault failed it (ADR 0032).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{CatalogIndex, CountError, Finding, FindingKind, Limitation, Position};

/// What the suite did with one call failing, and who established it.
///
/// A closed set of its own rather than a [`super::Decision`]: a fault changes what the program is given, never the program, so nothing here is a kill and nothing is proved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case", deny_unknown_fields)]
pub enum FaultDecision {
    /// A test failed with the call failing and passed on the unchanged program, first in target order.
    Noticed {
        /// The target that noticed.
        by: String,
    },
    /// Every test that reached the site passed with the call failing.
    Unnoticed,
    /// No test reached the site, so every test runs the same with the call failing.
    Unreached,
    /// A bound expired with the call failing before a test finished, which establishes nothing.
    Waited {
        /// The target that did not finish.
        on: String,
    },
    /// A test failed with the call failing and the run could not confirm it was the fault, or could not run the test at all.
    Undecided {
        /// The target.
        on: String,
        /// What stood in the way.
        why: String,
    },
    /// The compiler refused the fault, because the engine cannot make the error type the site propagates.
    NotPut {
        /// The first line the compiler said.
        diagnostic: String,
    },
}

impl FaultDecision {
    /// Names every decision and decides nothing, so a decision added later sends whoever adds it to [`Self::every`].
    #[cfg(feature = "testkit")]
    const fn witnessed(&self) {
        match self {
            Self::Noticed { .. }
            | Self::Unnoticed
            | Self::Unreached
            | Self::Waited { .. }
            | Self::Undecided { .. }
            | Self::NotPut { .. } => {}
        }
    }

    /// One of every decision, named in full, so a test can hold the set to the schema that publishes it.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn every() -> Vec<Self> {
        let every = vec![
            Self::Noticed { by: "t".to_owned() },
            Self::Unnoticed,
            Self::Unreached,
            Self::Waited { on: "t".to_owned() },
            Self::Undecided {
                on: "t".to_owned(),
                why: "w".to_owned(),
            },
            Self::NotPut {
                diagnostic: "d".to_owned(),
            },
        ];
        for decision in &every {
            decision.witnessed();
        }
        every
    }

    /// The wire name a report records.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Noticed { .. } => "noticed",
            Self::Unnoticed => "unnoticed",
            Self::Unreached => "unreached",
            Self::Waited { .. } => "waited",
            Self::Undecided { .. } => "undecided",
            Self::NotPut { .. } => "not-put",
        }
    }
}

/// One site a fault was asked at, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultRecord {
    /// The dense position in the catalog of faults, which is what a shard divides.
    pub catalog_index: CatalogIndex,
    /// The fault's full identity.
    pub id: String,
    /// The fault's short identity, which a person types.
    pub display_id: String,
    /// The file the `?` is in, relative to the workspace root.
    pub path: String,
    /// The item that holds it.
    pub item: String,
    /// Where the call it fails starts.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub position: Option<Position>,
    /// What became of it.
    pub decision: FaultDecision,
}

impl FaultRecord {
    /// Where a person reads it: the file, and the line where the run knows one.
    #[must_use]
    pub fn place(&self) -> String {
        self.position.map_or_else(
            || self.path.clone(),
            |at| format!("{}:{}", self.path, at.line),
        )
    }
}

/// How many sites a run asked a fault at, by what became of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultAccounting {
    /// Every site, which the six after it add up to.
    pub sites: u32,
    /// How many a test noticed.
    pub noticed: u32,
    /// How many every reaching test passed.
    pub unnoticed: u32,
    /// How many no test reached.
    pub unreached: u32,
    /// How many a bound expired on.
    pub waited: u32,
    /// How many the run put and could not decide.
    pub undecided: u32,
    /// How many the compiler refused.
    pub not_put: u32,
}

impl FaultAccounting {
    /// The counts of `records`.
    ///
    /// # Errors
    /// Returns the counter a v1 report cannot hold.
    pub fn of(records: &[FaultRecord]) -> Result<Self, CountError> {
        let mut counted = Self::default();
        for record in records {
            let (field, count) = match record.decision {
                FaultDecision::Noticed { .. } => ("fault noticed", &mut counted.noticed),
                FaultDecision::Unnoticed => ("fault unnoticed", &mut counted.unnoticed),
                FaultDecision::Unreached => ("fault unreached", &mut counted.unreached),
                FaultDecision::Waited { .. } => ("fault waited", &mut counted.waited),
                FaultDecision::Undecided { .. } => ("fault undecided", &mut counted.undecided),
                FaultDecision::NotPut { .. } => ("fault not put", &mut counted.not_put),
            };
            *count = count.checked_add(1).ok_or(CountError::Overflow { field })?;
        }
        counted.sites =
            u32::try_from(records.len()).map_err(|_outside_wire_range| CountError::Width {
                ledger: "fault sites",
                count: records.len(),
            })?;
        Ok(counted)
    }

    /// Whether the six decisions add up to the sites, which every report holds.
    #[must_use]
    pub fn adds_up(self) -> bool {
        [
            self.noticed,
            self.unnoticed,
            self.unreached,
            self.waited,
            self.undecided,
            self.not_put,
        ]
        .into_iter()
        .try_fold(0_u32, u32::checked_add)
            == Some(self.sites)
    }
}

/// One finding for every failure nothing noticed, and one for every fault the run put and could not decide, which leaves the run short of assured.
#[must_use]
pub fn found(records: &[FaultRecord]) -> Vec<Finding> {
    records
        .iter()
        .filter_map(|record| {
            let (kind, detail) = match &record.decision {
                FaultDecision::Unnoticed => (
                    FindingKind::UnnoticedFault,
                    format!(
                        "the call the `?` at {} asks about failed and every test that reached \
                         it passed: no test asserts what `{}` does when it fails",
                        record.place(),
                        record.item
                    ),
                ),
                FaultDecision::Waited { on } => (
                    FindingKind::NotMeasured,
                    format!(
                        "the call the `?` at {} asks about failed and {on} did not finish \
                         before its bound, so nothing is claimed about that failure",
                        record.place()
                    ),
                ),
                FaultDecision::Undecided { on, why } => (
                    FindingKind::NotMeasured,
                    format!(
                        "the call the `?` at {} asks about failed and what {on} did with it \
                         was not decided ({why}), so nothing is claimed about that failure",
                        record.place()
                    ),
                ),
                FaultDecision::Noticed { .. }
                | FaultDecision::Unreached
                | FaultDecision::NotPut { .. } => return None,
            };
            let mut finding = Finding::new(kind, &record.display_id, &detail);
            finding.path = Some(record.path.clone());
            finding.position = record.position;
            Some(finding)
        })
        .collect()
}

/// What a run does not claim about the faults the compiler refused, stated once with every class of refusal and its sites.
#[must_use]
pub fn limited(records: &[FaultRecord]) -> Vec<Limitation> {
    let mut refused: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for record in records {
        if let FaultDecision::NotPut { diagnostic } = &record.decision {
            refused
                .entry(class(diagnostic))
                .or_default()
                .push(record.place());
        }
    }
    if refused.is_empty() {
        return Vec::new();
    }
    let classes: Vec<String> = refused
        .iter()
        .map(|(class, places)| format!("{class} at {}", places.join(", ")))
        .collect();
    vec![Limitation::new(
        crate::limitation::FAULT_NOT_PUT,
        &format!(
            "the compiler refused {} fault(s), because the engine makes only the standard \
             error types it can build without guessing and these sites propagate another, so \
             nothing is claimed about their failures: {}",
            refused.values().map(Vec::len).sum::<usize>(),
            classes.join("; ")
        ),
    )]
}

/// The compiler's error code in a first line, or the whole line where it named none.
fn class(diagnostic: &str) -> &str {
    diagnostic
        .strip_prefix("error[")
        .and_then(|rest| rest.split_once(']'))
        .map_or(diagnostic, |(code, _message)| code)
}
