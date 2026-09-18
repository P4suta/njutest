// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One place at a time, with what a reviewer said supplied rather than typed.

use super::{Blindness, Place, Spot, Standing, Telling, Terminal, Told, Unsettled};

/// Why a mutation may stand, which is never nothing.
///
/// An acceptance with no reason is a mutation nobody looked at, recorded as
/// one somebody did. Held as a type rather than checked at the point of use so
/// that every path to an acceptance goes through the same refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason(String);

impl Reason {
    /// The reason somebody wrote, or nothing where they wrote nothing.
    #[must_use]
    pub fn of(said: &str) -> Option<Self> {
        let said = said.trim();
        (!said.is_empty()).then(|| Self(said.to_owned()))
    }

    /// What they wrote.
    #[must_use]
    pub fn said(&self) -> &str {
        &self.0
    }
}

/// What a reviewer says about a gap the run established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutAGap {
    /// It may stand, and this is why.
    Accept(Reason),
    /// Not now.
    Leave,
    /// Stop the review here.
    Stop,
}

/// What a reviewer says about a place the run established nothing about.
///
/// No `Accept`, and that is the whole point of the second type. An acceptance
/// says somebody looked at what a run found and decided it may stand; where
/// nothing was found there is nothing to have looked at, and accepting one
/// would record a decision about a measurement that never happened
/// (ADR 0023). The arm does not exist, so the loop cannot offer it and a
/// caller cannot answer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AboutTheUnsettled {
    /// Move on.
    Leave,
    /// Stop the review here.
    Stop,
}

/// What a review came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reviewed {
    /// What may stand, by the name a reader types, with why.
    pub accepted: Vec<(String, Reason)>,
    /// How many were shown and left.
    pub left: usize,
    /// Where it stopped, when somebody stopped it.
    pub stopped_at: Option<String>,
}

/// Somebody deciding, one place at a time.
///
/// Two methods rather than one, because there are two questions and their
/// answers are different types. A reviewer cannot accept what the run
/// established nothing about, because the method that would carry the answer
/// returns a type with no such arm (ADR 0023).
///
/// A trait rather than two closures so that one reviewer holds one terminal
/// and one keyboard: two closures each wanting the same stream is a borrow
/// the compiler refuses, and it is right to.
pub trait Answers {
    /// What to do about a gap the run established.
    fn about_a_gap(&mut self, spot: &Spot, blindness: Blindness, drawn: &str) -> AboutAGap;

    /// What to do about a place the run established nothing about.
    fn about_the_unsettled(
        &mut self,
        spot: &Spot,
        unsettled: Unsettled,
        drawn: &str,
    ) -> AboutTheUnsettled;
}

/// Shows each place the tests do not see, one at a time, and keeps what a reviewer decided.
///
/// The answers are an argument, so every rule of the loop is asserted in an
/// ordinary test with no terminal and no keyboard — the shape `njutest watch`
/// already uses for `look` and `round` (ADR 0020).
pub fn review<A: Answers>(told: &Told, terminal: Terminal, answers: &mut A) -> Reviewed {
    let telling = Telling::of(terminal);
    let mut reviewed = Reviewed::default();
    for place in &told.places {
        for spot in &place.spots {
            let drawn = one(place, spot, telling);
            let stop = match spot.standing {
                Standing::Blind(blindness) => match answers.about_a_gap(spot, blindness, &drawn) {
                    AboutAGap::Accept(reason) => {
                        reviewed.accepted.push((spot.locator.clone(), reason));
                        false
                    }
                    AboutAGap::Leave => {
                        reviewed.left = reviewed.left.saturating_add(1);
                        false
                    }
                    AboutAGap::Stop => true,
                },
                Standing::Unsettled(why) => match answers.about_the_unsettled(spot, why, &drawn) {
                    AboutTheUnsettled::Leave => {
                        reviewed.left = reviewed.left.saturating_add(1);
                        false
                    }
                    AboutTheUnsettled::Stop => true,
                },
            };
            if stop {
                reviewed.stopped_at = Some(spot.locator.clone());
                return reviewed;
            }
        }
    }
    reviewed
}

/// One place, drawn with the one spot a reviewer is being asked about marked on it.
///
/// The same drawing the whole run gets, narrowed to one mark. A review that
/// showed a different picture from the report would be a second account of
/// the same fact, which is what `Told` exists to prevent.
fn one(place: &Place, spot: &Spot, telling: Telling) -> String {
    let narrowed = Place {
        spots: vec![spot.clone()],
        ..place.clone()
    };
    let told = Told {
        headline: super::Headline {
            verdict: crate::report::Verdict::Insufficient,
            project: String::new(),
            cataloged: 0,
            killed: 0,
            survived: 0,
            unreached: 0,
            timed_out: 0,
            duration_ms: 0,
            kept: String::new(),
        },
        places: vec![narrowed],
        diagnostics: Vec::new(),
        limitations: Vec::new(),
    };
    let drawn = super::human::draw(&told, telling.terminal());
    drawn
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n")
}
