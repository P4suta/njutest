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
    /// Every test that reached the site passed with the call failing, and something formatted the failure it made, or the record cannot say.
    Unnoticed,
    /// Every test that reached the site passed with the call failing, and a run of each dropped every failure it made without anything reading it (ADR 0032 decision 5).
    Absorbed,
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
            | Self::Absorbed
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
            Self::Absorbed,
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
            Self::Absorbed => "absorbed",
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
    /// The dense position in the faulted session's catalog, which also holds the mutations a fault is put beside and is what a shard divides.
    pub catalog_index: CatalogIndex,
    /// The fault's full identity.
    pub id: String,
    /// The fault's short identity, which a person types.
    pub display_id: String,
    /// The file the `?` is in, relative to the workspace root.
    pub path: String,
    /// The rule that proposed it, whose name is part of its identity.
    pub rule: String,
    /// The version of that rule, which is part of its identity too.
    pub rule_version: u32,
    /// The bytes of the file the call it fails covers.
    pub span: rust_mutants::span::Span,
    /// The lowercase hex SHA-256 of the whole file as the run read it.
    pub source_digest: String,
    /// The call it fails, exactly as the file spells it over `span`.
    pub original: String,
    /// What the call becomes under the fault.
    pub replacement: String,
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
        match self.position {
            Some(at) => format!("{}:{}", self.path, at.line),
            None => self.path.clone(),
        }
    }
}

/// Which of the two runs of a target failed, where exactly one did: the call failing alone, or the call failing with the mutation beside it.
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
pub enum Failed {
    /// The target passed with the call failing, and failed with the mutation beside it.
    Beside,
    /// The target failed with the call failing, and passed with the mutation beside it.
    Alone,
}

impl Failed {
    /// The name a drawing spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Beside => "beside",
            Self::Alone => "alone",
        }
    }
}

/// What a survivor a target told from the original only with the call at its own site failing is called: evidence it is no equivalence, in no kill count and no score (ADR 0032 decision 6).
pub const OBSERVABLE_UNDER_FAULT: &str = "observable-under-fault";

/// A survivor put again with the fault at its own call beside it, where a target told it from the call failing alone (ADR 0032 decision 6).
///
/// It is evidence that the survivor is not an equivalence, never a kill: no test made the call fail, the run did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BesideRecord {
    /// The survivor's short identity.
    pub mutant: String,
    /// The fault's short identity.
    pub fault: String,
    /// The target that told them apart.
    pub target: String,
    /// Which of the two runs failed.
    pub failed: Failed,
}

/// One pair of runs of one target behind evidence beside a fault: the call failing alone, then with the survivor beside it.
///
/// Recorded for every pair a run makes, so the evidence can be re-derived from the runs rather than read back from itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BesideRun {
    /// The survivor's short identity.
    pub mutant: String,
    /// The fault's short identity.
    pub fault: String,
    /// The target both runs were put to.
    pub target: String,
    /// What the run with the call failing alone came to, as the engine names outcomes.
    pub alone: String,
    /// What the run with the survivor beside the failing call came to.
    pub with: String,
}

/// How many sites a run asked a fault at, by what became of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultAccounting {
    /// Every site, which the seven after it add up to.
    pub sites: u32,
    /// How many a test noticed.
    pub noticed: u32,
    /// How many every reaching test passed, with the failure read by something or the record unable to say.
    pub unnoticed: u32,
    /// How many every reaching test passed with the failure dropped unread.
    pub absorbed: u32,
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
                FaultDecision::Absorbed => ("fault absorbed", &mut counted.absorbed),
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

    /// Whether the seven decisions add up to the sites, which every report holds.
    #[must_use]
    pub fn adds_up(self) -> bool {
        [
            self.noticed,
            self.unnoticed,
            self.absorbed,
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
                FaultDecision::Absorbed => (
                    FindingKind::UnnoticedFault,
                    format!(
                        "the call the `?` at {} asks about failed, every test that reached it \
                         passed, and the failure it made was dropped without anything reading \
                         it: `{}` goes on as if the call had not failed, and no test asserts \
                         what it does instead",
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
        crate::limitation::Limitation::FaultNotPut,
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
    match diagnostic
        .strip_prefix("error[")
        .and_then(|rest| rest.split_once(']'))
    {
        Some((code, _message)) => code,
        None => diagnostic,
    }
}
