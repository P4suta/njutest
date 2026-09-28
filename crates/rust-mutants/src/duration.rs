// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a duration is spelled where a person writes one: a run of numbers, each with a unit.

use std::fmt::Write as _;
use std::time::Duration;

/// Why some text is not a duration.
/// Every case names the text, so a configuration error points at the line a person wrote.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DurationError {
    /// There was nothing to read.
    #[error("a duration cannot be empty")]
    Empty,
    /// A unit stands where a number should be.
    #[error("{text:?} has a unit without a number")]
    UnitWithoutNumber {
        /// The text as given.
        text: String,
    },
    /// A number ends the text with no unit after it.
    #[error("{text:?} has a number without a unit; write ns, us, ms, s, m, or h")]
    NumberWithoutUnit {
        /// The text as given.
        text: String,
    },
    /// A unit that names no scale.
    #[error("{text:?} holds the unknown unit {unit:?}; write ns, us, ms, s, m, or h")]
    UnknownUnit {
        /// The text as given.
        text: String,
        /// The unit that was not understood.
        unit: String,
    },
    /// A duration longer than the longest one a [`Duration`] holds, however it was spelled.
    #[error("{text:?} is longer than any duration can be")]
    TooLarge {
        /// The text as given.
        text: String,
    },
}

/// The units a duration is written in, largest first, each with the nanoseconds one of it is: the one table both [`parse`] and [`render`] read, so what one writes the other reads.
const UNITS: [(&str, u128); 6] = [
    ("h", 3_600_000_000_000),
    ("m", 60_000_000_000),
    ("s", 1_000_000_000),
    ("ms", 1_000_000),
    ("us", 1_000),
    ("ns", 1),
];

/// A second spelling a person may write for a unit, with the unit it is.
const ALIASES: [(&str, &str); 1] = [("\u{b5}s", "us")];

/// The nanoseconds in one second, which is how a [`Duration`] is made from a count of them.
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Reads a duration: one or more `<number><unit>` pairs, added together exactly.
///
/// # Errors
/// See [`DurationError`]: [`DurationError::TooLarge`] is about the sum, never about one number of it.
pub fn parse(text: &str) -> Result<Duration, DurationError> {
    if text.is_empty() {
        return Err(DurationError::Empty);
    }
    let too_large = || DurationError::TooLarge {
        text: text.to_owned(),
    };
    let mut total: u128 = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let digits = rest
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return Err(DurationError::UnitWithoutNumber {
                text: text.to_owned(),
            });
        }
        let (number, tail) = rest.split_at(digits);
        let count = number.parse::<u128>().map_err(|_error| too_large())?;
        let unit_length = tail
            .find(|character: char| character.is_ascii_digit())
            .unwrap_or(tail.len());
        let (unit, tail) = tail.split_at(unit_length);
        if unit.is_empty() {
            return Err(DurationError::NumberWithoutUnit {
                text: text.to_owned(),
            });
        }
        let scale = scale_of(unit).ok_or_else(|| DurationError::UnknownUnit {
            text: text.to_owned(),
            unit: unit.to_owned(),
        })?;
        total = count
            .checked_mul(scale)
            .and_then(|nanos| total.checked_add(nanos))
            .ok_or_else(too_large)?;
        rest = tail;
    }
    exactly(total).ok_or_else(too_large)
}

/// How many nanoseconds one of `unit` is, under its own name or an alias.
fn scale_of(unit: &str) -> Option<u128> {
    let unit = ALIASES
        .iter()
        .find(|(alias, _)| *alias == unit)
        .map_or(unit, |(_, named)| *named);
    UNITS
        .iter()
        .find(|(name, _)| *name == unit)
        .map(|(_, scale)| *scale)
}

/// The duration of exactly `nanos` nanoseconds, when a [`Duration`] can be that long.
fn exactly(nanos: u128) -> Option<Duration> {
    let seconds = nanos.checked_div(NANOS_PER_SECOND)?;
    let under = nanos.checked_rem(NANOS_PER_SECOND)?;
    match (u64::try_from(seconds), u32::try_from(under)) {
        (Ok(seconds), Ok(under)) => Some(Duration::new(seconds, under)),
        (Err(_), _) | (_, Err(_)) => None,
    }
}

/// Writes a duration in the spelling [`parse`] reads, largest unit first and exact.
#[must_use]
pub fn render(value: Duration) -> String {
    let mut nanos = value.as_nanos();
    let mut text = String::new();
    for (name, scale) in UNITS {
        let count = nanos.checked_div(scale).unwrap_or_default();
        if count > 0 {
            let written = write!(text, "{count}{name}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
            nanos = nanos.saturating_sub(count.saturating_mul(scale));
        }
    }
    if text.is_empty() {
        text.push_str("0s");
    }
    text
}
