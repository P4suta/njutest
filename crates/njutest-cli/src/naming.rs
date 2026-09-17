// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a reader names one mutation, and the one place that answers to it.

use crate::report::MutantRecord;

/// How a reader names one mutation again, which has to hold after they have edited the file.
///
/// An identity is a function of the whole file, so the edit somebody makes
/// next — the test that closes this very survivor — re-mints it. A locator is
/// where the mutation is and what was done to it, which survives that edit.
#[must_use]
pub fn locator(mutant: &MutantRecord) -> String {
    if mutant.item.is_empty() || mutant.path.is_empty() {
        return mutant.display_id.clone();
    }
    format!(
        "{}:{}:{}@{}",
        mutant.path, mutant.item, mutant.rule, mutant.position.line
    )
}

/// Every mutation of `mutants` that `named` names.
///
/// One function rather than one per command, because a name a run printed has
/// to work everywhere a name is taken: `explain`, `accept` and `replay` each
/// resolving it their own way is how a tool comes to print a command it then
/// refuses.
#[must_use]
pub fn matching<'a>(mutants: &[&'a MutantRecord], named: &str) -> Vec<&'a MutantRecord> {
    if let Some(wanted) = rust_mutants::session::Locator::parse(named) {
        let found: Vec<&MutantRecord> = mutants
            .iter()
            .copied()
            .filter(|mutant| located(mutant, &wanted))
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    mutants
        .iter()
        .copied()
        .filter(|mutant| mutant.id.starts_with(named) || mutant.display_id.starts_with(named))
        .collect()
}

/// Whether one mutation is the one a locator names.
fn located(mutant: &MutantRecord, wanted: &rust_mutants::session::Locator) -> bool {
    mutant.path == wanted.path
        && mutant.item == wanted.item
        && mutant.rule == wanted.rule
        && wanted.line.is_none_or(|line| mutant.position.line == line)
}
