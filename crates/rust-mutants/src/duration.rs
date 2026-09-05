// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a duration is spelled where a person writes one: a run of numbers, each with a unit.

use std::fmt::Write as _;
use std::time::Duration;

/// Why some text is not a duration. Every case names the text, so a configuration error points at the line a person wrote.
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
    /// A number no duration can hold.
    #[error("{text:?} holds a number too large to be a duration")]
    TooLarge {
        /// The text as given.
        text: String,
    },
}

/// The units, longest name first so `ms` is never read as `m` followed by `s`.
const UNITS: [(&str, Duration); 7] = [
    ("ns", Duration::from_nanos(1)),
    ("us", Duration::from_micros(1)),
    ("\u{b5}s", Duration::from_micros(1)),
    ("ms", Duration::from_millis(1)),
    ("s", Duration::from_secs(1)),
    ("m", Duration::from_secs(60)),
    ("h", Duration::from_secs(3600)),
];

/// Reads a duration: one or more `<number><unit>` pairs, added together.
///
/// # Errors
/// See [`DurationError`].
pub fn parse(text: &str) -> Result<Duration, DurationError> {
    if text.is_empty() {
        return Err(DurationError::Empty);
    }
    let mut total = Duration::ZERO;
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
        let value: u64 = number.parse().map_err(|_error| DurationError::TooLarge {
            text: text.to_owned(),
        })?;
        let unit_length = tail
            .find(|character: char| character.is_ascii_digit())
            .unwrap_or(tail.len());
        let (unit, tail) = tail.split_at(unit_length);
        if unit.is_empty() {
            return Err(DurationError::NumberWithoutUnit {
                text: text.to_owned(),
            });
        }
        let scale = UNITS
            .iter()
            .find(|(name, _)| *name == unit)
            .map(|(_, scale)| *scale)
            .ok_or_else(|| DurationError::UnknownUnit {
                text: text.to_owned(),
                unit: unit.to_owned(),
            })?;
        total =
            total.saturating_add(scale.saturating_mul(u32::try_from(value).map_err(|_error| {
                DurationError::TooLarge {
                    text: text.to_owned(),
                }
            })?));
        rest = tail;
    }
    Ok(total)
}

/// Writes a duration in the spelling [`parse`] reads, largest unit first and exact.
#[must_use]
pub fn render(value: Duration) -> String {
    let mut nanos = value.as_nanos();
    let mut text = String::new();
    for (name, scale) in [
        ("h", 3_600_000_000_000u128),
        ("m", 60_000_000_000),
        ("s", 1_000_000_000),
        ("ms", 1_000_000),
        ("us", 1_000),
        ("ns", 1),
    ] {
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
