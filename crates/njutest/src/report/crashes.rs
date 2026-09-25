// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run established about each call that writes, once a crash stopped the process just after it (ADR 0035).

use serde::{Deserialize, Serialize};

use super::{CatalogIndex, CountError, Finding, FindingKind, Limitation, Position};

/// What the next run did over what a crash left.
///
/// A closed set of its own: a crash is not a change to the program, and a next run passing over what it left says nothing about whether it read any of it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CrashDecision {
    /// The next run passed over the files the crash left.
    Restarted {
        /// The target and test the crash stopped and the next run ran.
        on: String,
        /// The files the crash left in the scratch, relative to it.
        left: Vec<String>,
    },
    /// The next run failed over what the crash left, a run in a fresh scratch passed, and a second crash failed the next run again.
    Corrupt {
        /// The target and test.
        on: String,
        /// The tests the next run failed.
        failed: Vec<String>,
    },
    /// The crashed run left no file in its scratch, so the next run could not have read anything of it.
    Unshared {
        /// The target and test.
        on: String,
    },
    /// No test that reached the call stopped at it.
    Unreached,
    /// A run could not be decided: a bound expired, which test reaches the call is not known, or the failure did not reproduce.
    Undecided {
        /// The target and test, or the target.
        on: String,
        /// What stood in the way.
        why: String,
    },
    /// The compiler refused the crash.
    NotPut {
        /// The first line the compiler said.
        diagnostic: String,
    },
}

impl CrashDecision {
    /// Names every decision and decides nothing, so a decision added later sends whoever adds it to [`Self::every`].
    #[cfg(feature = "testkit")]
    const fn witnessed(&self) {
        match self {
            Self::Restarted { .. }
            | Self::Corrupt { .. }
            | Self::Unshared { .. }
            | Self::Unreached
            | Self::Undecided { .. }
            | Self::NotPut { .. } => {}
        }
    }

    /// One of every decision, named in full, so a test can hold the set to the schema that publishes it.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn every() -> Vec<Self> {
        let every = vec![
            Self::Restarted {
                on: "t".to_owned(),
                left: vec!["count".to_owned()],
            },
            Self::Corrupt {
                on: "t".to_owned(),
                failed: vec!["t".to_owned()],
            },
            Self::Unshared { on: "t".to_owned() },
            Self::Unreached,
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
            Self::Restarted { .. } => "restarted",
            Self::Corrupt { .. } => "corrupt",
            Self::Unshared { .. } => "unshared",
            Self::Unreached => "unreached",
            Self::Undecided { .. } => "undecided",
            Self::NotPut { .. } => "not-put",
        }
    }
}

/// One call that writes a crash was asked at, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashRecord {
    /// The dense position in the crashed session's catalog, which is what a shard divides.
    pub catalog_index: CatalogIndex,
    /// The crash's full identity.
    pub id: String,
    /// The crash's short identity, which a person types.
    pub display_id: String,
    /// The file the call is in, relative to the workspace root.
    pub path: String,
    /// The item that holds it.
    pub item: String,
    /// Where the call starts, or nothing where the run could not place it.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub position: Option<Position>,
    /// What became of it.
    pub decision: CrashDecision,
}

impl CrashRecord {
    /// Where a person reads it: the file, and the line where the run knows one.
    #[must_use]
    pub fn place(&self) -> String {
        self.position.map_or_else(
            || self.path.clone(),
            |at| format!("{}:{}", self.path, at.line),
        )
    }
}

/// How many calls that write a run asked a crash at, by what became of each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashAccounting {
    /// Every site, which the six after it add up to.
    pub sites: u32,
    /// How many the next run passed over.
    pub restarted: u32,
    /// How many the next run could not start over.
    pub corrupt: u32,
    /// How many left nothing in the scratch.
    pub unshared: u32,
    /// How many no test stopped at.
    pub unreached: u32,
    /// How many the run put and could not decide.
    pub undecided: u32,
    /// How many the compiler refused.
    pub not_put: u32,
}

impl CrashAccounting {
    /// The counts of `records`.
    ///
    /// # Errors
    /// Returns the counter a v1 report cannot hold.
    pub fn of(records: &[CrashRecord]) -> Result<Self, CountError> {
        let count = |field: &'static str, decided: fn(&CrashDecision) -> bool| {
            let found = records
                .iter()
                .filter(|record| decided(&record.decision))
                .count();
            u32::try_from(found).map_err(|_outside_wire_range| CountError::Width {
                ledger: field,
                count: found,
            })
        };
        Ok(Self {
            sites: count("crash sites", |_every| true)?,
            restarted: count("crashes restarted", |one| {
                matches!(one, CrashDecision::Restarted { .. })
            })?,
            corrupt: count("crashes corrupt", |one| {
                matches!(one, CrashDecision::Corrupt { .. })
            })?,
            unshared: count("crashes unshared", |one| {
                matches!(one, CrashDecision::Unshared { .. })
            })?,
            unreached: count("crashes unreached", |one| {
                matches!(one, CrashDecision::Unreached)
            })?,
            undecided: count("crashes undecided", |one| {
                matches!(one, CrashDecision::Undecided { .. })
            })?,
            not_put: count("crashes not put", |one| {
                matches!(one, CrashDecision::NotPut { .. })
            })?,
        })
    }

    /// Whether the six decisions add up to the sites, which every report holds.
    #[must_use]
    pub fn adds_up(self) -> bool {
        [
            self.restarted,
            self.corrupt,
            self.unshared,
            self.unreached,
            self.undecided,
            self.not_put,
        ]
        .into_iter()
        .try_fold(0_u32, u32::checked_add)
            == Some(self.sites)
    }
}

/// One finding for every call the next run could not start over, and one for every crash the run could not decide or whose run left nothing to start over.
#[must_use]
pub fn found(records: &[CrashRecord]) -> Vec<Finding> {
    records
        .iter()
        .filter_map(|record| {
            let (kind, detail) = match &record.decision {
                CrashDecision::Corrupt { on, failed } => (
                    FindingKind::CorruptAfterCrash,
                    format!(
                        "the process stopped just after the call at {} writes, and the next run \
                         of {on} failed over what it left ({}): what `{}` writes cannot be \
                         started over after a stop there",
                        record.place(),
                        failed.join(", "),
                        record.item
                    ),
                ),
                CrashDecision::Unshared { on } => (
                    FindingKind::NotMeasured,
                    format!(
                        "the process stopped just after the call at {} writes and {on} left \
                         nothing in its scratch, so no next run could have read anything of it",
                        record.place()
                    ),
                ),
                CrashDecision::Undecided { on, why } => (
                    FindingKind::NotMeasured,
                    format!(
                        "a stop just after the call at {} writes was not decided on {on}: {why}",
                        record.place()
                    ),
                ),
                CrashDecision::Restarted { .. }
                | CrashDecision::Unreached
                | CrashDecision::NotPut { .. } => return None,
            };
            let mut finding = Finding::new(kind, &record.display_id, &detail);
            finding.path = Some(record.path.clone());
            finding.position = record.position;
            Some(finding)
        })
        .collect()
}

/// What a run does not claim about the crashes the compiler refused.
#[must_use]
pub fn limited(records: &[CrashRecord]) -> Vec<Limitation> {
    let refused: Vec<String> = records
        .iter()
        .filter(|record| matches!(record.decision, CrashDecision::NotPut { .. }))
        .map(CrashRecord::place)
        .collect();
    if refused.is_empty() {
        return Vec::new();
    }
    vec![Limitation::new(
        crate::limitation::CRASH_NOT_PUT,
        &format!(
            "the compiler refused {} crash(es), so nothing is claimed about a stop after those \
             calls: {}",
            refused.len(),
            refused.join(", ")
        ),
    )]
}
