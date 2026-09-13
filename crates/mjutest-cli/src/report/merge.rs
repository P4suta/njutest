// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Combining the parts of one catalog into the report the whole would have written.
//!
//! A part judged the mutants its shard held and measured the whole baseline,
//! and it concluded [`crate::report::Verdict::Partial`] because the mutations it did not
//! judge are not mutations nothing noticed. This puts the parts back together
//! and derives the verdict the whole supports.
//!
//! What it refuses is the point. The engine refuses parts of two catalogs and
//! parts that overlap; this refuses two more, because a runner's report is not
//! the same kind of document. The engine's is a collection of "running it said
//! this", and two such collections add up whatever settings produced them. A
//! runner's report is one claim — that a contract was met — and a claim
//! assembled from a part that met it and a part that met something else is
//! true of neither.

use std::collections::{BTreeMap, BTreeSet};

use super::{MutantAccounting, Report};

/// Why some reports are not the parts of one catalog.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MergeError {
    /// No parts were offered, and nothing is not a catalog.
    #[error("{}: no reports were offered, and nothing is not a catalog", crate::error::MERGE_REFUSED.code)]
    Nothing,
    /// The parts do not agree about what they were run against.
    #[error(
        "{}: the parts disagree about {about}: {first} and {other}. Adding up answers about two different runs produces an answer about neither",
        crate::error::MERGE_REFUSED.code
    )]
    Disagree {
        /// What they disagree about, as a reader would name it.
        about: &'static str,
        /// What the first part said.
        first: String,
        /// What the part that differs said.
        other: String,
    },
    /// Two parts judged the same mutant, so they were not cut from one division.
    #[error(
        "{}: {mutant} was judged by two parts, so they were not cut from one division of the catalog; run every part with the same N",
        crate::error::MERGE_REFUSED.code
    )]
    Overlapping {
        /// The mutant both parts hold.
        mutant: String,
    },
    /// The reports do not name every distinct part of one division.
    #[error(
        "{}: the reports are not one complete shard set: {because}. Offer exactly one report for every shard from 1/N through N/N",
        crate::error::MERGE_REFUSED.code
    )]
    ShardSet {
        /// What made the set incomplete or incoherent.
        because: String,
    },
}

impl MergeError {
    /// The code a reader looks up.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::Nothing
            | Self::Disagree { .. }
            | Self::Overlapping { .. }
            | Self::ShardSet { .. } => crate::error::MERGE_REFUSED,
        }
    }
}

/// The report the whole catalog would have written, from the reports of its parts.
///
/// The accounting and the verdict are derived again from what the whole holds
/// rather than combined from what the parts said: a part's numbers are over a
/// different denominator, and averaging them is how a report comes to say
/// something no run observed.
///
/// # Errors
/// See [`MergeError`]. Every one of them is a refusal to add up things that
/// are not parts of one answer.
pub fn merge(parts: &[Report]) -> Result<Report, MergeError> {
    let first = parts.first().ok_or(MergeError::Nothing)?;
    agree(parts, first)?;
    complete_shard_set(parts)?;
    let mut whole = first.clone();
    whole.scope.shard = None;
    whole.mutants = judged(parts)?;
    whole.findings = gathered(parts, |part| part.findings.clone());
    whole.limitations = gathered(parts, |part| part.limitations.clone());
    whole.candidates = gathered(parts, |part| part.candidates.clone());
    whole.timing = spanning(parts);
    whole.accounting.mutants = counted(parts, &whole.mutants);
    whole.count_targets();
    whole.verdict = whole.concluded();
    whole.sort_targets();
    Ok(whole)
}

/// Requires exactly one report for every part of one `K/N` division.
///
/// Clearing `scope.shard` turns the union into a claim about the whole catalog.
/// That is sound only when the labels themselves prove that no part is absent;
/// disjoint mutant rows cannot prove that, because an absent part has no rows
/// with which to overlap.
fn complete_shard_set(parts: &[Report]) -> Result<(), MergeError> {
    if parts.len() == 1 && parts.first().is_some_and(|part| part.scope.shard.is_none()) {
        return Ok(());
    }
    let mut denominator = None;
    let mut indices = BTreeSet::new();
    for part in parts {
        let text = part
            .scope
            .shard
            .as_deref()
            .ok_or_else(|| MergeError::ShardSet {
                because: format!("report {:?} does not name a shard", part.run_id),
            })?;
        let shard =
            rust_mutants::run::Shard::parse(text).map_err(|error| MergeError::ShardSet {
                because: format!("report {:?} names {text:?}: {error}", part.run_id),
            })?;
        match denominator {
            Some(expected) if shard.of != expected => {
                return Err(MergeError::ShardSet {
                    because: format!(
                        "shard {text} is one of {actual}, while the first report is one of {expected}",
                        actual = shard.of
                    ),
                });
            }
            None => denominator = Some(shard.of),
            Some(_) => {}
        }
        if !indices.insert(shard.index) {
            return Err(MergeError::ShardSet {
                because: format!("shard {text} was offered more than once"),
            });
        }
    }

    let Some(of) = denominator else {
        return Err(MergeError::Nothing);
    };
    if indices.len() != usize::try_from(of).unwrap_or(usize::MAX) {
        let Some(missing) = (1..=of).find(|index| !indices.contains(index)) else {
            return Err(MergeError::ShardSet {
                because: format!(
                    "{} distinct shard labels cannot describe the declared {of} parts",
                    indices.len()
                ),
            });
        };
        return Err(MergeError::ShardSet {
            because: format!(
                "shard {missing}/{of} is missing; {} of {of} reports were offered",
                indices.len()
            ),
        });
    }
    Ok(())
}

/// Whether every part answered the same question about the same tree.
fn agree(parts: &[Report], first: &Report) -> Result<(), MergeError> {
    for part in parts {
        for (about, one, other) in [
            (
                "the tree",
                &first.repository.workspace_digest,
                &part.repository.workspace_digest,
            ),
            (
                "the configuration",
                &first.repository.configuration_digest,
                &part.repository.configuration_digest,
            ),
        ] {
            if one != other {
                return Err(MergeError::Disagree {
                    about,
                    first: one.clone(),
                    other: other.clone(),
                });
            }
        }
        if first.contract != part.contract {
            return Err(MergeError::Disagree {
                about: "the contract",
                first: format!("{:?}", first.contract).to_lowercase(),
                other: format!("{:?}", part.contract).to_lowercase(),
            });
        }
        if first.run_kind != part.run_kind {
            return Err(MergeError::Disagree {
                about: "the run scope",
                first: format!("{:?}", first.run_kind).to_lowercase(),
                other: format!("{:?}", part.run_kind).to_lowercase(),
            });
        }
        let first_scope = (
            &first.scope.requested_packages,
            &first.scope.resolved_packages,
            &first.scope.excluded,
        );
        let part_scope = (
            &part.scope.requested_packages,
            &part.scope.resolved_packages,
            &part.scope.excluded,
        );
        if first_scope != part_scope {
            return Err(MergeError::Disagree {
                about: "the selected packages and exclusions",
                first: format!("{first_scope:?}"),
                other: format!("{part_scope:?}"),
            });
        }
        if first.tool != part.tool {
            return Err(MergeError::Disagree {
                about: "the runner and engine versions",
                first: format!("{:?}", first.tool),
                other: format!("{:?}", part.tool),
            });
        }
    }
    Ok(())
}

/// Every mutant the parts judged, in identity order, refusing one that two of them hold.
fn judged(parts: &[Report]) -> Result<Vec<super::MutantRecord>, MergeError> {
    let mut held: BTreeMap<String, super::MutantRecord> = BTreeMap::new();
    for part in parts {
        for mutant in &part.mutants {
            if held.contains_key(&mutant.id) {
                return Err(MergeError::Overlapping {
                    mutant: mutant.id.clone(),
                });
            }
            let _new = held.insert(mutant.id.clone(), mutant.clone());
        }
    }
    Ok(held.into_values().collect())
}

/// One list from every part's, each entry once, in the order the parts were offered.
///
/// A part states what it found and what it is not claiming, and two parts of
/// one run state the same limitations about the same baseline: keeping one of
/// each is what stops a whole from saying a thing twice for every part it was
/// cut into.
fn gathered<T: PartialEq + Clone, F: Fn(&Report) -> Vec<T>>(parts: &[Report], of: F) -> Vec<T> {
    let mut seen: Vec<T> = Vec::new();
    for part in parts {
        for one in of(part) {
            if !seen.contains(&one) {
                seen.push(one);
            }
        }
    }
    seen
}

/// The whole's mutant counts, derived from what the whole holds.
///
/// Derived rather than added up, so the whole's columns say what the whole's
/// own records say however a part counted itself. A *score* would not survive
/// either way — two ratios over different denominators average into a number
/// no run observed — which is why the verdict is decided again from the whole
/// rather than carried over from a part.
///
/// The one column no record carries is `accepted`, which is a fact about a
/// reviewer rather than about a mutant, so it is summed. Every mutant belongs
/// to exactly one part, so the sum counts each acceptance once.
fn counted(parts: &[Report], mutants: &[super::MutantRecord]) -> MutantAccounting {
    let mut counts = MutantAccounting {
        cataloged: u32::try_from(mutants.len()).unwrap_or(u32::MAX),
        accepted: parts.iter().fold(0, |sum, part| {
            sum.saturating_add(part.accounting.mutants.accepted)
        }),
        ..MutantAccounting::default()
    };
    for mutant in mutants {
        let executed = &mut counts.executed;
        match mutant.outcome.as_str() {
            "compile-rejected" => counts.rejected = counts.rejected.saturating_add(1),
            "killed" => {
                *executed = executed.saturating_add(1);
                counts.killed = counts.killed.saturating_add(1);
                if mutant.reused {
                    counts.reused_killed = counts.reused_killed.saturating_add(1);
                }
            }
            "timed_out" => {
                *executed = executed.saturating_add(1);
                counts.timed_out = counts.timed_out.saturating_add(1);
            }
            "survived" => {
                *executed = executed.saturating_add(1);
                counts.survived = counts.survived.saturating_add(1);
                if mutant.reused {
                    counts.reused_survived = counts.reused_survived.saturating_add(1);
                }
            }
            "unreached" => counts.unreached = counts.unreached.saturating_add(1),
            "equivalent" => counts.equivalent = counts.equivalent.saturating_add(1),
            _ => *executed = executed.saturating_add(1),
        }
    }
    counts
}

/// When the parts ran, as one span: the first start, the last finish, and the time they cost between them.
///
/// The duration is the sum rather than the span, because parts run on
/// different machines at once and the wall clock of the whole is not what the
/// work cost.
fn spanning(parts: &[Report]) -> super::Timing {
    let said = |when: fn(&Report) -> &String| {
        parts
            .iter()
            .map(when)
            .filter(|one| !one.is_empty())
            .cloned()
    };
    super::Timing {
        started: said(|part| &part.timing.started).min().unwrap_or_default(),
        finished: said(|part| &part.timing.finished).max().unwrap_or_default(),
        duration_ms: parts
            .iter()
            .fold(0, |sum, part| sum.saturating_add(part.timing.duration_ms)),
    }
}
