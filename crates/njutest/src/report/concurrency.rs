// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run establishes about whether each test binary runs one thread, and what exploring its schedules found, as a report keeps it.

use serde::{Deserialize, Serialize};

use crate::concurrency::proof::Standing;

use super::{Finding, FindingKind, Limitation};

/// What a run established about one test binary's threads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcurrencyRecord {
    /// The binary, as `package/kind/name`.
    pub target: String,
    /// What was established.
    pub standing: Standing,
    /// What delaying its guards found.
    pub explored: Exploration,
}

/// What delaying the guards of one binary found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Exploration {
    /// No schedule of it was explored, and why.
    Unexplored {
        /// Why.
        why: Unexplored,
    },
    /// Every delayed schedule passed or settled nothing: a sample, never a proof.
    Sampled {
        /// The catalog index of every guard delayed, in the order they were.
        delayed: Vec<u32>,
        /// Those whose delayed controls settled nothing.
        undecided: Vec<u32>,
    },
    /// Delaying one site made its tests fail, twice more, and they passed without the delay.
    Broke {
        /// The catalog index of the guard.
        site: u32,
        /// Where it is.
        path: String,
        /// Its 1-based line.
        line: u32,
        /// The tests that failed.
        failed: Vec<String>,
    },
}

/// Why no schedule of a binary was explored.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Unexplored {
    /// It is proven to run one thread, so there is no schedule to explore.
    NotNeeded,
    /// No schedule was asked for.
    NotAsked,
    /// Its baseline did not pass, so a schedule has nothing to hold.
    NotPassing,
    /// Its baseline reached no guard, so there is nowhere to delay a thread.
    NoSite,
}

/// The findings the records earn: one `schedule-dependent` for each binary a delayed site broke.
#[must_use]
pub fn found(records: &[ConcurrencyRecord]) -> Vec<Finding> {
    records
        .iter()
        .filter_map(|record| match &record.explored {
            Exploration::Broke {
                path, line, failed, ..
            } => Some(Finding::new(
                FindingKind::ScheduleDependent,
                &record.target,
                &format!(
                    "{} passed on its baseline and failed when a thread was paused the first time \
                     it reached {path}:{line}: {}. It passed again without the pause, so what it \
                     answers depends on the schedule its threads get; make the test wait for what \
                     it asserts on rather than for time to pass, and run again",
                    record.target,
                    if failed.is_empty() {
                        "its harness named no test".to_owned()
                    } else {
                        failed.join(", ")
                    }
                ),
            )),
            Exploration::Unexplored { .. } | Exploration::Sampled { .. } => None,
        })
        .collect()
}

/// The limitations the records earn: the binaries not proven to run one thread and not explored, and those only sampled, each named in the closing list.
#[must_use]
pub fn limited(records: &[ConcurrencyRecord]) -> Vec<Limitation> {
    let mut unexplored = Vec::new();
    let mut sampled = Vec::new();
    for record in records {
        match &record.explored {
            Exploration::Broke { .. }
            | Exploration::Unexplored {
                why: Unexplored::NotNeeded,
            } => {}
            Exploration::Unexplored {
                why: Unexplored::NotAsked | Unexplored::NotPassing | Unexplored::NoSite,
            } => unexplored.push(record.target.as_str()),
            Exploration::Sampled { .. } => sampled.push(record.target.as_str()),
        }
    }
    let mut limitations = Vec::new();
    if !unexplored.is_empty() {
        limitations.push(Limitation::new(
            crate::limitation::SCHEDULE_NOT_EXPLORED,
            &format!(
                "{} not proven to run one thread and no schedule of {} was explored, so what {} \
                 when {} threads interleave otherwise is not known ({})",
                binaries(unexplored.len()),
                if unexplored.len() == 1 { "it" } else { "them" },
                if unexplored.len() == 1 {
                    "it does"
                } else {
                    "they do"
                },
                if unexplored.len() == 1 {
                    "its"
                } else {
                    "their"
                },
                unexplored.join(", ")
            ),
        ));
    }
    if !sampled.is_empty() {
        limitations.push(Limitation::new(
            crate::limitation::SCHEDULE_SAMPLED,
            &format!(
                "{} passed every schedule a delayed guard made, which is a sample of the schedules \
                 and not all of them, so a race none of those delays exposed is not ruled out ({})",
                binaries(sampled.len()),
                sampled.join(", ")
            ),
        ));
    }
    limitations
}

/// `1 test binary` or `N test binaries`, with the verb that agrees.
fn binaries(count: usize) -> String {
    if count == 1 {
        "1 test binary is".to_owned()
    } else {
        format!("{count} test binaries are")
    }
}

/// Whether the records name each binary once, in order: what a part is refused for where they do not.
#[must_use]
pub fn ordered(records: &[ConcurrencyRecord]) -> bool {
    records
        .windows(2)
        .all(|pair| matches!(pair, [one, next] if one.target < next.target))
}
