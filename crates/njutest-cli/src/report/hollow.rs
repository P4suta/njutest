// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target that was put to mutations and noticed none of them.

use std::collections::BTreeMap;

use super::{Blind, Decision, Finding, FindingKind, MutantRecord};

/// How many mutations one target answered, and whether it ever noticed one.
#[derive(Debug, Clone, Default)]
struct Answering {
    /// One witness for every mutation it gave an answer to.  The count is the
    /// allocation's exact length, not an independently maintained integer.
    answered: Vec<()>,
    /// Whether it ever noticed one.
    noticed: bool,
}

/// Every target the records say answered about mutations and noticed none of them.
///
/// The evidence is `routing.answered` and nothing else. A target that is
/// absent from every `killed_by` is not thereby a target that notices
/// nothing — a run records the first detection and asks the cheapest targets
/// first, so one that is outranked every time never appears there. What
/// `answered` says is who was actually put to a mutation, which is the only
/// ground on which a run may say a target noticed nothing.
///
/// Only an answer somebody decided counts. A target whose harness would not
/// start, or whose pair did not agree, did not notice nothing — the run
/// established nothing about it, and counting those as chances it failed to
/// take would accuse a broken harness of asserting nothing, with the count as
/// the weight of the accusation.
///
/// `records` must be the whole catalog. A part of one has seen a slice of the
/// mutations, and a target silent in this part may have noticed something in
/// another; the caller is what knows which it holds.
#[must_use]
pub fn found(records: &[MutantRecord]) -> Vec<Finding> {
    let mut answering: BTreeMap<&str, Answering> = BTreeMap::new();
    for record in records {
        let Some(routing) = record.routing.as_ref() else {
            continue;
        };
        for answered in &routing.answered {
            let decision = answered.outcome.decision();
            if decision.blind().is_some_and(Blind::is_unanswered) {
                continue;
            }
            let held = answering.entry(answered.target.as_str()).or_default();
            held.answered.push(());
            if decision == Decision::Tests {
                held.noticed = true;
            }
        }
    }
    answering
        .into_iter()
        .filter(|(_, held)| !held.answered.is_empty() && !held.noticed)
        .map(|(target, held)| {
            Finding::new(
                FindingKind::HollowTarget,
                target,
                &format!(
                    "{target} answered about {} mutation{} and noticed none of them",
                    held.answered.len(),
                    if held.answered.len() == 1 { "" } else { "s" }
                ),
            )
        })
        .collect()
}
