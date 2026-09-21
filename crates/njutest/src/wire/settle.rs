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

/// What putting one fault to the suite came to.
///
/// An empty list of answers meant both `the question was never put` and `the question was put and the suite could not be measured`, and the one sentence both reached told a reader the exchange never came past.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// The suite ran with the fault in place, and this is what each target said.
    Answered(Vec<Answered>),
    /// The suite ran and nothing about it could be measured, so the question stands unanswered.
    NotMeasured(rust_mutants::outcome::Outcome),
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
/// A fault nothing ran is a hole rather than a survivor: counting a question nobody asked as one nothing could answer would let a run that never reached a seam report it as assured.
#[must_use]
pub fn settle(fault: &Fault, asked: &Asked) -> Settled {
    let answers = match asked {
        Asked::Answered(answers) => answers.as_slice(),
        Asked::NotMeasured(_nothing_to_read) => &[],
    };
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
