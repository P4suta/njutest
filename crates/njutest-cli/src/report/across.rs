// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one mutation stands on when more than one build of the project measured it.

use std::collections::BTreeMap;

use super::Decision;

/// What a run records about one mutation, taken across every build that measured it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What the run records, which is the weakest thing any build established.
    pub decision: Decision,
    /// The builds under which nothing noticed it, in the order a report lists them.
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
