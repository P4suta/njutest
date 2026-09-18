// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a thing a run establishes looks like, decided once for every surface either product draws.

use crate::outcome::Outcome;

/// What a piece of text is, which is how it is painted.
///
/// A surface asks for what a thing *is* and never for a colour. Two surfaces
/// that each named a colour would drift, and they did: the same run drew a
/// timed-out mutation amber in its progress line and green in its dashboard,
/// because two modules had each decided what a timeout was worth
/// (ADR 0023).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// A gap in the tests: the thing a run is for.
    Gap,
    /// Something the run could not establish.
    Limitation,
    /// Something that stopped the run.
    Refusal,
    /// A run that found nothing.
    Well,
    /// A part of a catalog, which assures nothing on its own.
    Partial,
    /// The rules and gutters a drawing is made of, which the eye should pass over.
    Frame,
    /// A command a reader is meant to type.
    Command,
    /// A name a reader is meant to notice.
    Subject,
    /// The bytes a run replaced, lit where they are in the code.
    Changed,
    /// A word the language reserves.
    Keyword,
    /// A type, which in Rust begins with a capital.
    Type,
    /// A string or a character.
    Text,
    /// A number.
    Number,
    /// A comment.
    Aside,
    /// A name being called.
    Call,
    /// A lifetime or an attribute.
    Marker,
    /// Code that is none of the above.
    Code,
}

/// What a style means, for a screen that has its own palette rather than escapes.
///
/// A terminal library has sixteen names and no idea what any of them are for.
/// This is the layer between: a style says what a thing is, a hue says what
/// that is worth, and a screen program turns a hue into whatever its own
/// palette calls that. Six rather than sixteen, because the question is how
/// much of a reader's attention something deserves and there are not sixteen
/// answers to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hue {
    /// A gap: the thing a run is for.
    Alarm,
    /// Something the run could not establish.
    Caution,
    /// Something that stopped the run.
    Refusal,
    /// A run that found nothing.
    Settled,
    /// Something a reader is meant to notice but not worry about.
    Named,
    /// What the eye passes over.
    Quiet,
}

impl Style {
    /// How much of a reader's attention this deserves, for a screen with its own palette.
    #[must_use]
    pub const fn hue(self) -> Hue {
        match self {
            Self::Gap | Self::Changed => Hue::Alarm,
            Self::Limitation | Self::Partial => Hue::Caution,
            Self::Refusal => Hue::Refusal,
            Self::Well => Hue::Settled,
            Self::Subject | Self::Command | Self::Type | Self::Call => Hue::Named,
            Self::Frame
            | Self::Keyword
            | Self::Text
            | Self::Number
            | Self::Aside
            | Self::Marker
            | Self::Code => Hue::Quiet,
        }
    }

    /// What to write before the text, as the parameters of one escape.
    ///
    /// Amber, teal and rose rather than the sixteen a theme redefines: a
    /// palette chosen once and read the same on every terminal that has 256
    /// colours, which is every terminal anybody has used this decade.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Gap => "1;38;5;214",
            Self::Limitation => "38;5;73",
            Self::Refusal => "1;38;5;203",
            Self::Well => "1;38;5;114",
            Self::Partial => "38;5;110",
            Self::Frame => "38;5;244",
            Self::Command => "38;5;252",
            Self::Subject => "1;38;5;253",
            Self::Changed => "1;4;38;5;204",
            Self::Keyword => "38;5;176",
            Self::Type => "38;5;79",
            Self::Text => "38;5;150",
            Self::Number => "38;5;179",
            Self::Aside => "3;38;5;243",
            Self::Call => "38;5;111",
            Self::Marker => "38;5;139",
            Self::Code => "38;5;250",
        }
    }

    /// What one mutation's outcome looks like, wherever this engine draws it.
    ///
    /// Exhaustive on purpose: an outcome added later is one somebody is made
    /// to place, rather than one that inherits whatever the last arm said.
    ///
    /// `TimedOut` here is a *confirmed* timeout — over the budget, retried
    /// alone, over it again — which this engine counts as a detection, so it
    /// is painted like one. njutest's assurance layer spells a differently
    /// shaped fact with the same English word: a bound that expired, which
    /// establishes nothing. Two facts, two products, and the reason this
    /// function is about `rust_mutants::outcome::Outcome` and answers for
    /// nothing else.
    #[must_use]
    pub const fn of(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Killed | Outcome::TimedOut => Self::Well,
            Outcome::Survived => Self::Gap,
            Outcome::Inconclusive | Outcome::Errored => Self::Limitation,
            Outcome::NotRun => Self::Frame,
        }
    }

    /// `text` painted, or `text`, depending on what the stream will take.
    ///
    /// The one place in the workspace that turns a style into bytes, which is
    /// what `cargo xtask lints` refuses everywhere else.
    #[must_use]
    pub fn painted(self, text: &str, colour: bool) -> String {
        if !colour {
            return text.to_owned();
        }
        format!("\u{1b}[{}m{text}\u{1b}[0m", self.code())
    }
}

/// `text` as a link to `target`, where the terminal follows one.
///
/// A path a reader can click is a file they do not have to find, and a
/// terminal that does not know the sequence shows the text and drops the
/// rest, so this costs nothing where it does nothing. Here beside the
/// painting because it is the same job: turning something a surface meant
/// into bytes a terminal reads, in the one place that does.
#[must_use]
pub fn linked(target: &str, text: &str, colour: bool) -> String {
    if !colour {
        return text.to_owned();
    }
    format!("\u{1b}]8;;{target}\u{7}{text}\u{1b}]8;;\u{7}")
}
