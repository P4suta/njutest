// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The accounting of a run, laid out once for every projection of it.

use rust_mutants::report::run::{Accounting, RunDocument};

/// What a run counted, arranged so that a reader can add the right things together.
///
/// Every projection used to lay this out for itself, and all four laid it out
/// wrong in the same way: the counts that partition the catalog and the counts
/// that are subsets of one of those went into one list, so a reader adding a
/// column got more than there are. Two of them also disagreed about which
/// columns exist. The arrangement is made here, once, and each projection
/// renders what it is given.
#[derive(Debug)]
pub struct Tally {
    /// What the parts add up to, and the word for it.
    pub whole: (&'static str, u32),
    /// The counts that partition the whole. These add to it and to nothing else.
    pub parts: Vec<(&'static str, u32)>,
    /// A count of some of one of the parts: the part's name, this one's name, and how many.
    pub within: Vec<(&'static str, &'static str, u32)>,
    /// A count of something outside the whole: neither a part of it nor a part of a part.
    pub beside: Vec<(&'static str, u32)>,
}

impl Tally {
    /// The accounting of `document`, arranged.
    #[must_use]
    pub fn of(document: &RunDocument) -> Self {
        let counted: &Accounting = &document.accounting;
        Self {
            whole: ("cataloged", counted.cataloged),
            parts: vec![
                ("killed", counted.killed.count()),
                ("survived", counted.survived.count()),
                ("step_limit_reached", counted.step_limit_reached.count()),
                ("waited", counted.waited.count()),
                ("inconclusive", counted.inconclusive.count()),
                ("errored", counted.errored.count()),
                ("not run", counted.not_run.count()),
            ],
            within: vec![
                ("not run", "unreached", counted.unreached.count()),
                ("not run", "discharged", counted.discharged.count()),
                ("survived", "expected", counted.expected.count()),
            ],
            beside: vec![
                ("executed", counted.executed.count()),
                ("refused by the compiler", counted.refused.count()),
                ("places that produced no candidate", counted.skipped.count()),
            ],
        }
    }

    /// The sentence that says what the whole is and what sits outside it.
    #[must_use]
    pub fn said(&self) -> String {
        let (name, whole) = self.whole;
        let beside = self
            .beside
            .iter()
            .map(|(what, count)| format!("{count} {what}"))
            .collect::<Vec<String>>()
            .join(", ");
        format!("{whole} mutants were {name}: {beside}.")
    }

    /// The sentence that says the parts add up, and what is counted inside them.
    #[must_use]
    pub fn within_said(&self) -> String {
        let (name, whole) = self.whole;
        let within = self
            .within
            .iter()
            .map(|(part, what, count)| format!("{part} is {count} {what}"))
            .collect::<Vec<String>>()
            .join(", ");
        format!(
            "Those {} add to the {whole} {name}. Within them, {within}.",
            self.parts.len()
        )
    }
}
