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

/// What each target did before any fault went in.
///
/// A fault is noticed when a target that passed without it fails with it, so deciding that needs both runs.
/// Given only the faulted one, a target that was already red reads as a detection, and a suite that is simply broken reports wire coverage it has none of -- the one direction this phase must never fail in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Before {
    passed: std::collections::BTreeSet<String>,
    failed: std::collections::BTreeSet<String>,
}

impl Before {
    /// What passed when the suite ran with no fault in place.
    #[must_use]
    pub fn of(answers: &[Answered]) -> Self {
        Self {
            passed: answers
                .iter()
                .filter(|one| one.passed)
                .map(|one| one.target.clone())
                .collect(),
            failed: answers
                .iter()
                .filter(|one| !one.passed)
                .map(|one| one.target.clone())
                .collect(),
        }
    }

    /// Which targets failed with no fault in place, which is why a question about one establishes nothing.
    ///
    /// A run that could not attribute a failure knows which target was already red, and saying nothing about it leaves a reader to guess at the machine.
    #[must_use]
    pub fn already_failing(&self) -> Vec<&str> {
        self.failed.iter().map(String::as_str).collect()
    }

    /// Whether a failure of `target` is one a fault could have caused, which is to say it passed without one.
    #[must_use]
    pub fn could_notice(&self, target: &str) -> bool {
        self.passed.contains(target)
    }

    /// Whether no target passed with no fault in place, so nothing in the suite can answer a question about one.
    #[must_use]
    pub fn nothing_passed(&self) -> bool {
        self.passed.is_empty()
    }
}

/// What a run establishes about one fault, and who established it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    /// The fault this is about.
    pub fault: Fault,
    /// What the run establishes, and — where somebody decided — who that was.
    pub decision: SeamDecision,
}

/// What `answers` establish about `fault`, given what the same targets did in `before`.
///
/// A fault nothing ran is a hole rather than a survivor: counting a question nobody asked as one nothing could answer would let a run that never reached a seam report it as assured.
/// A target that was already failing is a hole wearing a detection: its failure is not attributable to the fault, so crediting it would let a broken suite report wire coverage it has none of.
/// The search is therefore for a target that *could* have noticed rather than the first that failed, so one already-red target standing ahead of a genuine detection does not hide it.
///
/// One target that could have noticed and did not is enough to say nothing noticed, even beside others that could not answer.
/// Letting an already-red target pull that back to `Unreached` would mean running one more test could leave a run establishing less than before, and measuring more would be a way of finding less.
#[must_use]
pub fn settle(fault: &Fault, asked: &Asked, before: &Before) -> Settled {
    let answers = match asked {
        Asked::Answered(answers) => answers.as_slice(),
        Asked::NotMeasured(_nothing_to_read) => &[],
    };
    let noticed_by = answers
        .iter()
        .find(|answered| !answered.passed && before.could_notice(&answered.target))
        .map(|answered| answered.target.clone());
    let decision = if let Some(noticed_by) = noticed_by {
        SeamDecision::Tests { noticed_by }
    } else if answers
        .iter()
        .any(|answered| before.could_notice(&answered.target))
    {
        SeamDecision::Unnoticed
    } else {
        SeamDecision::Unreached
    };
    Settled {
        fault: fault.clone(),
        decision,
    }
}

/// Whether the suite answered and not one target it answered with could have noticed a fault.
///
/// `SeamDecision::Unreached` reaches a reader through a finding saying which of its causes this was, and this is the one a decision cannot tell from the others by itself.
#[must_use]
pub fn nothing_could_answer(asked: &Asked, before: &Before) -> bool {
    let Asked::Answered(answers) = asked else {
        return false;
    };
    !answers.is_empty()
        && !answers
            .iter()
            .any(|answered| before.could_notice(&answered.target))
}
