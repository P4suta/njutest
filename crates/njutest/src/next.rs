// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What to do next: the gaps in the tests, one behaviour at a time, cheapest first, each with the checked test that closes it where the run was offered one.

use std::collections::BTreeMap;

use crate::presentation::{Place, Standing, Told};
use crate::report::CandidateRecord;

/// One behaviour the tests do not pin: the mutations of one item no test noticed, and what closes them.
#[derive(Debug, Clone)]
pub struct Gap<'a> {
    /// The place the mutations are in.
    pub place: &'a Place,
    /// The display identity of each mutation of it no test noticed, in the order the place shows them.
    pub mutants: Vec<&'a str>,
    /// The checked test that closes the most of them, where the run was offered one.
    pub offer: Option<Offer<'a>>,
}

/// One candidate the run checked, with the mutations of a gap it was checked against and held up for: at least one, so everything said of it is said of something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer<'a> {
    /// The candidate's record for the first mutation it closes.
    pub first: &'a CandidateRecord,
    /// Its records for every other, each naming the same content by the same digest.
    pub rest: Vec<&'a CandidateRecord>,
}

impl<'a> Offer<'a> {
    /// Every record of it, one per mutation it closes.
    pub fn closes(&self) -> impl Iterator<Item = &'a CandidateRecord> + '_ {
        std::iter::once(self.first).chain(self.rest.iter().copied())
    }

    /// How many mutations it closes.
    #[must_use]
    pub fn count(&self) -> usize {
        self.closes().count()
    }

    /// The content every record of it names.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.first.digest
    }

    /// Where it would be written.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.first.path
    }

    /// The fewest times any of its records passed on the clean tree and noticed its mutation, which is what holds for every one of them.
    #[must_use]
    pub fn held(&self) -> (u32, u32) {
        self.closes().fold(
            (self.first.stability_runs, self.first.kill_runs),
            |(stable, killing), one| (stable.min(one.stability_runs), killing.min(one.kill_runs)),
        )
    }
}

/// Every gap in the tests `told` shows, cheapest to close first: one a checked test closes before one nothing was offered for, more closed before fewer, then by path and item.
///
/// A mutation the run established nothing about is a gap in the run, not in the tests, so it is never offered here (ADR 0023): no test closes it.
#[must_use]
pub fn gaps<'a>(told: &'a Told, candidates: &'a [CandidateRecord]) -> Vec<Gap<'a>> {
    let mut found: Vec<Gap<'a>> = told
        .places
        .iter()
        .filter_map(|place| {
            let mutants: Vec<&str> = place
                .spots
                .iter()
                .filter(|spot| match spot.standing {
                    Standing::Blind(_) => true,
                    Standing::Unsettled(_) => false,
                })
                .map(|spot| spot.mutant.as_str())
                .collect();
            if mutants.is_empty() {
                return None;
            }
            let offer = offered(&mutants, candidates);
            Some(Gap {
                place,
                mutants,
                offer,
            })
        })
        .collect();
    found.sort_by(|one, other| {
        let closed = |gap: &Gap<'_>| gap.offer.as_ref().map_or(0, Offer::count);
        other
            .offer
            .is_some()
            .cmp(&one.offer.is_some())
            .then_with(|| closed(other).cmp(&closed(one)))
            .then_with(|| other.mutants.len().cmp(&one.mutants.len()))
            .then_with(|| one.place.path.cmp(&other.place.path))
            .then_with(|| one.place.item.cmp(&other.place.item))
    });
    found
}

/// The checked content that closes the most of `mutants`: of the records that held up for one of them, those naming one digest, the digest with the most, and of two with as many the smaller.
fn offered<'a>(mutants: &[&str], candidates: &'a [CandidateRecord]) -> Option<Offer<'a>> {
    let mut by_digest: BTreeMap<&str, Vec<&'a CandidateRecord>> = BTreeMap::new();
    for record in candidates
        .iter()
        .filter(|record| record.accepted && mutants.contains(&record.mutant.as_str()))
    {
        let closes = by_digest.entry(record.digest.as_str()).or_default();
        if !closes.iter().any(|one| one.mutant == record.mutant) {
            closes.push(record);
        }
    }
    by_digest
        .into_values()
        .rev()
        .max_by_key(Vec::len)
        .and_then(|closes| {
            let (first, rest) = closes.split_first()?;
            Some(Offer {
                first,
                rest: rest.to_vec(),
            })
        })
}

/// What somebody says to a checked test they are offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnOffer {
    /// Write it, once it holds up again against the tree as it is now.
    Take,
    /// Leave this gap open and show the next.
    Next,
    /// Show nothing more.
    Stop,
}

/// What somebody says to a gap nothing closes yet, where there is nothing to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnGap {
    /// Show the next.
    Next,
    /// Show nothing more.
    Stop,
}

/// Whoever answers, one question per gap: an offer can be taken, and a gap nothing closes cannot, so the two are asked apart.
pub trait Deciding {
    /// A checked test, described by `said`.
    fn about_an_offer(&mut self, said: &str) -> OnOffer;
    /// A gap nothing was offered for, described by `said`.
    fn about_a_gap(&mut self, said: &str) -> OnGap;
}

/// What going through the gaps came to.
#[derive(Debug, Clone)]
pub struct Walked<'a> {
    /// The offers somebody took, in the order they were taken.
    pub taken: Vec<Offer<'a>>,
    /// Whether somebody stopped before the last.
    pub stopped: bool,
}

/// Goes through `gaps` in order, one question each, until somebody stops.
pub fn walk<'a>(gaps: &[Gap<'a>], deciding: &mut dyn Deciding) -> Walked<'a> {
    let mut taken = Vec::new();
    let mut stopped = false;
    for gap in gaps {
        let said = said(gap);
        let going_on = match &gap.offer {
            Some(offer) => match deciding.about_an_offer(&said) {
                OnOffer::Take => {
                    taken.push(offer.clone());
                    true
                }
                OnOffer::Next => true,
                OnOffer::Stop => false,
            },
            None => match deciding.about_a_gap(&said) {
                OnGap::Next => true,
                OnGap::Stop => false,
            },
        };
        if !going_on {
            stopped = true;
            break;
        }
    }
    Walked { taken, stopped }
}

/// What became of one taken offer when it was checked again against the tree as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Became {
    /// It held up and was written.
    Written,
    /// It held up, and the file already is what it would write.
    Already,
    /// It did not hold up, so nothing was written.
    Refused,
}

/// How many mutations of `gaps` are still open once each of `taken` became what `became` says, in the same order: a take closes its mutations only where it was written or already there.
#[must_use]
pub fn open(gaps: &[Gap<'_>], taken: &[Offer<'_>], became: &[Became]) -> usize {
    let closed: std::collections::BTreeSet<&str> = taken
        .iter()
        .zip(became)
        .filter(|(_, became)| match became {
            Became::Written | Became::Already => true,
            Became::Refused => false,
        })
        .flat_map(|(offer, _)| offer.closes().map(|record| record.mutant.as_str()))
        .collect();
    gaps.iter()
        .flat_map(|gap| gap.mutants.iter())
        .filter(|mutant| !closed.contains(**mutant))
        .count()
}

/// What a person is told about one gap: what closes it and how that was checked, or that nothing was offered and how to be.
#[must_use]
pub fn said(gap: &Gap<'_>) -> String {
    let item = &gap.place.item;
    let path = &gap.place.path;
    let all = gap.mutants.len();
    match &gap.offer {
        Some(offer) => {
            let closed = offer.count();
            let (stable, killing) = offer.held();
            let mut told = format!(
                "the cheapest thing you can do closes {closed} of the {all} gaps in `{item}` ({path}).\n  \
                 a checked test that does it: {} ({})\n  \
                 it passed on this tree {stable} times, and failed under each of those {closed} {} {killing} times.",
                offer.path(),
                offer.digest().get(..12).unwrap_or_else(|| offer.digest()),
                if closed == 1 { "mutation" } else { "mutations" },
            );
            let left = gap
                .mutants
                .iter()
                .filter(|mutant| !offer.closes().any(|record| record.mutant == **mutant))
                .count();
            if left > 0 {
                told = format!(
                    "{told}\n  {left} other {} of `{item}` {} open after it.",
                    if left == 1 { "mutation" } else { "mutations" },
                    if left == 1 { "stays" } else { "stay" },
                );
            }
            told
        }
        None => {
            let lines: Vec<String> = gap
                .place
                .spots
                .iter()
                .filter(|spot| gap.mutants.contains(&spot.mutant.as_str()))
                .map(|spot| spot.line.to_string())
                .collect();
            format!(
                "{all} {} of `{item}` ({path}) no test noticed, at {} {}.\n  \
                 no checked test was offered for {}; with `[generation]` configured, `njutest verify` asks for one.",
                if all == 1 { "mutation" } else { "mutations" },
                if lines.len() == 1 { "line" } else { "lines" },
                lines.join(", "),
                if all == 1 { "it" } else { "them" },
            )
        }
    }
}
