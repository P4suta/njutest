// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a model checker establishes about one mutation, and the two unlike reasons it establishes nothing.

use std::time::Duration;

/// What a model checker said about one mutation.
///
/// The question is not *did a test notice* but *does any input distinguish
/// the two renderings of this function*, which is a stronger answer than any
/// test run can give and the one `docs/roadmap.md` named and left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modelled {
    /// No input distinguishes them, so no observer of any kind could notice.
    Proved,
    /// An input distinguishes them, so something could.
    Noticed,
    /// Nothing was established.
    Undecided(Undecided),
}

/// Why nothing was established, which is two unlike things rather than three shades of one.
///
/// [`Self::Unaskable`] is a fact about the subject: the code was asked about
/// and the question was too hard, or it cannot be asked in this form at all.
/// [`Self::Cutoff`] is a fact about this run: it stopped watching. There is
/// no fact about the code in it, not even a negative one.
///
/// Split here rather than in the sentence, because a reader shown the second
/// beside the first concludes *this survivor resisted proof* from something
/// that means *we stopped watching* — a run concluding from how it measured,
/// which is the one thing this product exists to prevent. A renderer cannot
/// make that mistake with these two, because it cannot reach the same words
/// from both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undecided {
    /// The code could not be asked, or not at this setting.
    Unaskable(Unaskable),
    /// The run stopped before the checker answered.
    Cutoff(Cutoff),
}

/// What about the code stopped the question being answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unaskable {
    /// The checker reached its bound before it reached the end of a loop.
    TooDeep {
        /// The bound it was given.
        bound: u32,
    },
    /// An argument has no arbitrary value, so no symbolic input can be made for it.
    NotArbitrary {
        /// The argument, as the signature spells it.
        argument: String,
    },
}

impl Unaskable {
    /// What a reader would change to get an answer.
    ///
    /// Exists because for both of these there is something to change. Its
    /// absence on [`Cutoff`] is the point: a reader whose run was cut off
    /// cannot reach an answer by changing anything about the code, and a
    /// renderer offering them a knob would be telling them their program is
    /// the reason it does not know.
    #[must_use]
    pub fn change(&self) -> String {
        match self {
            Self::TooDeep { bound } => {
                format!("raise the bound above {bound}, or bound the loop in the code")
            }
            Self::NotArbitrary { argument } => {
                format!("give {argument} an arbitrary value, or ask about a function without it")
            }
        }
    }

    /// Whether an answer is available at all, at some setting.
    ///
    /// `TooDeep` says the proof is there and the run did not reach it.
    /// `NotArbitrary` says there is nothing to reach in this form.
    #[must_use]
    pub const fn available(&self) -> bool {
        match self {
            Self::TooDeep { .. } => true,
            Self::NotArbitrary { .. } => false,
        }
    }
}

/// The run stopped before the checker answered.
///
/// No `change`, on purpose. Nothing was established about the code, so there
/// is nothing about the code to change, and the only honest sentence is about
/// the run. A model checker that hangs is not one that answers slowly: an
/// unbounded loop over a symbolic argument produces nothing at all, which is
/// why this is reached by the caller's clock rather than by the checker
/// declining.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cutoff {
    /// How long the run waited before it stopped.
    pub waited: Duration,
}

impl Modelled {
    /// The name a report and a trace use.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Proved => "proved",
            Self::Noticed => "noticed",
            Self::Undecided(_) => "undecided",
        }
    }

    /// Whether this establishes anything about the code at all.
    ///
    /// A cut-off run does not, and neither does an unaskable one — but they
    /// are unalike in what a reader does next, which is why they are separate
    /// cases rather than one.
    #[must_use]
    pub const fn about_the_code(&self) -> bool {
        match self {
            Self::Proved | Self::Noticed | Self::Undecided(Undecided::Unaskable(_)) => true,
            Self::Undecided(Undecided::Cutoff(_)) => false,
        }
    }
}
