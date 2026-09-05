// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Half-open byte ranges into one source file.

use std::fmt;

/// A span that is not well formed or does not fit its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SpanError {
    /// The end byte precedes the start byte.
    #[error("span end byte {end} precedes start byte {start}")]
    Reversed {
        /// The start offset.
        start: u32,
        /// The end offset.
        end: u32,
    },
    /// The span reaches past the end of the buffer it is applied to.
    #[error("span [{start},{end}) is out of range for {len} bytes of source")]
    OutOfRange {
        /// The start offset.
        start: u32,
        /// The end offset.
        end: u32,
        /// The length of the buffer.
        len: usize,
    },
}

/// A half-open byte range `[start, end)` into one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Span {
    /// The first byte covered.
    pub start: u32,
    /// One past the last byte covered.
    pub end: u32,
}

impl Span {
    /// The half-open span `[start, end)`.
    ///
    /// # Errors
    /// Returns [`SpanError::Reversed`] when `end` precedes `start`.
    pub const fn new(start: u32, end: u32) -> Result<Self, SpanError> {
        if end < start {
            return Err(SpanError::Reversed { start, end });
        }
        Ok(Self { start, end })
    }

    /// Whether the span is well formed.
    ///
    /// # Errors
    /// Returns [`SpanError::Reversed`] when the end precedes the start.
    pub const fn validate(self) -> Result<(), SpanError> {
        if self.end < self.start {
            return Err(SpanError::Reversed {
                start: self.start,
                end: self.end,
            });
        }
        Ok(())
    }

    /// The number of bytes the span covers.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span covers no bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Whether `other` lies entirely within `self`. A span contains itself, and an empty span sitting on either boundary counts as contained.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Whether `other` lies within `self` and is not `self`.
    #[must_use]
    pub const fn strictly_contains(self, other: Self) -> bool {
        self.contains(other) && !(self.start == other.start && self.end == other.end)
    }

    /// Whether the spans share at least one byte. Empty spans overlap nothing, including each other, wherever they sit.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        let starts_before_other_ends = self.start < other.end;
        let other_starts_before_end = other.start < self.end;
        !self.is_empty() && !other.is_empty() && starts_before_other_ends && other_starts_before_end
    }

    /// The bytes the span covers in `source`, without copying.
    ///
    /// # Errors
    /// Returns [`SpanError::Reversed`] or [`SpanError::OutOfRange`] rather than
    /// panicking: spans travel through caches and reports and may outlive the
    /// source they were minted from.
    pub fn slice(self, source: &[u8]) -> Result<&[u8], SpanError> {
        self.validate()?;
        let out_of_range = || SpanError::OutOfRange {
            start: self.start,
            end: self.end,
            len: source.len(),
        };
        let start = usize::try_from(self.start).map_err(|_overflow| out_of_range())?;
        let end = usize::try_from(self.end).map_err(|_overflow| out_of_range())?;
        source.get(start..end).ok_or_else(out_of_range)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{},{})", self.start, self.end)
    }
}
