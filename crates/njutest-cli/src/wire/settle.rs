// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run establishes about one wire fault, from what the tests did with it in place.

use super::derive::Fault;
use crate::report::SeamDecision;

/// What one target did with a fault in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The target, by the identity a report names it with.
    pub target: String,
    /// Whether it passed.
    pub passed: bool,
}

/// What a run establishes about one fault, and who established it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    /// The fault this is about.
    pub fault: Fault,
    /// What the run establishes, and — where somebody decided — who that was.
    pub decision: SeamDecision,
}

/// What `answers` establish about `fault`.
///
/// A fault nothing ran is a hole rather than a survivor: counting a question
/// nobody asked as one nothing could answer would let a run that never
/// reached a seam report it as assured.
#[must_use]
pub fn settle(fault: &Fault, answers: &[Answered]) -> Settled {
    let noticed_by = answers
        .iter()
        .find(|answered| !answered.passed)
        .map(|answered| answered.target.clone());
    let decision = match noticed_by {
        _ if answers.is_empty() => SeamDecision::Unreached,
        Some(noticed_by) => SeamDecision::Tests { noticed_by },
        None => SeamDecision::Unnoticed,
    };
    Settled {
        fault: fault.clone(),
        decision,
    }
}
