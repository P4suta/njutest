// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target that was put to mutations and noticed none of them.

use std::collections::BTreeMap;

use super::{Decision, Finding, FindingKind, MutantRecord};

/// How many mutations one target was asked about, and whether it ever noticed one.
#[derive(Debug, Clone, Copy, Default)]
struct Answering {
    /// How many mutations it was asked about.
    asked: u32,
    /// Whether it ever noticed one.
    noticed: bool,
}

/// Every target the records say was asked about mutations and noticed none of them.
///
/// The evidence is `routing.answered` and nothing else. A target that is
/// absent from every `killed_by` is not thereby a target that notices
/// nothing — a run records the first detection and asks the cheapest targets
/// first, so one that is outranked every time never appears there. What
/// `answered` says is who was actually put to a mutation, which is the only
/// ground on which a run may say a target noticed nothing.
#[must_use]
pub fn found(records: &[MutantRecord]) -> Vec<Finding> {
    let mut answering: BTreeMap<&str, Answering> = BTreeMap::new();
    for record in records {
        let Some(routing) = record.routing.as_ref() else {
            continue;
        };
        for answered in &routing.answered {
            let held = answering.entry(answered.target.as_str()).or_default();
            held.asked = held.asked.saturating_add(1);
            if Decision::of_outcome(&answered.outcome).is_some_and(|one| one == Decision::Tests) {
                held.noticed = true;
            }
        }
    }
    answering
        .into_iter()
        .filter(|(_, held)| held.asked > 0 && !held.noticed)
        .map(|(target, held)| {
            Finding::new(
                FindingKind::HollowTarget,
                target,
                &format!(
                    "{target} was put to {} mutation{} and noticed none of them, so \
                     nothing it asserts is held up by any of the changes this run made",
                    held.asked,
                    if held.asked == 1 { "" } else { "s" }
                ),
            )
        })
        .collect()
}
