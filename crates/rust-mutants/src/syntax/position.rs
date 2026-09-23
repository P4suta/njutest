// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Byte offsets to lines and columns.

use serde::{Deserialize, Serialize};

/// Where a byte offset falls in a file, for the console, the report, and the editors that consume `file:line:col`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    /// 1-based line.
    pub line: u32,
    /// 1-based byte offset within the line.
    pub byte_column: u32,
    /// 1-based character (Unicode scalar) offset within the line.
    pub char_column: u32,
}

/// Why an exact source position could not be represented.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PositionError {
    /// The source leaves no room for a one-based u32 line or column.
    #[error("a {bytes}-byte source does not fit one-based u32 source positions")]
    SourceTooLarge {
        /// The exact source length.
        bytes: usize,
    },
    /// A platform integer could not carry an addressable source offset.
    #[error("source byte offset {offset} is not addressable on this platform")]
    OffsetUnrepresentable {
        /// The exact wire offset.
        offset: u32,
    },
    /// The index's internal ordering invariant was contradicted.
    #[error("the line index invariant failed: {detail}")]
    Invariant {
        /// The invariant that failed closed.
        detail: &'static str,
    },
}

/// The byte offset of the start of every line, for offset-to-position lookups in constant-ish time.
#[derive(Debug, Clone)]
pub struct LineIndex<'source> {
    source: &'source str,
    starts: Vec<u32>,
    source_len_u32: u32,
}

impl<'source> LineIndex<'source> {
    /// Indexes `source` without truncating any line or column.
    ///
    /// # Errors
    /// Returns [`PositionError::SourceTooLarge`] when one-based u32 positions cannot represent every byte of the source.
    pub fn new(source: &'source str) -> Result<Self, PositionError> {
        let source_len_u32 = bounded_source_len(source.len())?;
        let mut starts = vec![0];
        for (offset, byte) in source.bytes().enumerate() {
            if byte != b'\n' {
                continue;
            }
            let next = offset.checked_add(1).ok_or(PositionError::Invariant {
                detail: "a source iterator offset can advance by one",
            })?;
            starts.push(u32::try_from(next).map_err(|_overflow| {
                PositionError::SourceTooLarge {
                    bytes: source.len(),
                }
            })?);
        }
        Ok(Self {
            source,
            starts,
            source_len_u32,
        })
    }

    /// The position of byte `offset` in `source`.
    /// An offset past the end lands at the end of the last line; an offset inside a UTF-8 scalar uses that scalar's column.
    ///
    /// # Errors
    /// Returns an exact source/index mismatch, platform conversion failure, or internal ordering contradiction.
    /// No saturated position is produced.
    pub fn position(&self, offset: u32) -> Result<Position, PositionError> {
        let bounded = offset.min(self.source_len_u32);
        let line = self.starts.partition_point(|&start| start <= bounded);
        let line_index = line.checked_sub(1).ok_or(PositionError::Invariant {
            detail: "the zero line start precedes every offset",
        })?;
        let line_start = *self
            .starts
            .get(line_index)
            .ok_or(PositionError::Invariant {
                detail: "partition_point names an existing line start",
            })?;
        let line = u32::try_from(line).map_err(|_overflow| PositionError::Invariant {
            detail: "a bounded source has at most u32::MAX lines",
        })?;
        let byte_column = bounded
            .checked_sub(line_start)
            .and_then(|zero_based| zero_based.checked_add(1))
            .ok_or(PositionError::Invariant {
                detail: "a line start does not follow its bounded offset",
            })?;
        let start = usize::try_from(line_start)
            .map_err(|_overflow| PositionError::OffsetUnrepresentable { offset: line_start })?;
        let mut end = usize::try_from(bounded)
            .map_err(|_overflow| PositionError::OffsetUnrepresentable { offset: bounded })?;
        while end > start && !self.source.is_char_boundary(end) {
            end = end.checked_sub(1).ok_or(PositionError::Invariant {
                detail: "a positive character boundary cursor can retreat",
            })?;
        }
        let prefix = self
            .source
            .get(start..end)
            .ok_or(PositionError::Invariant {
                detail: "bounded line offsets delimit valid source bytes",
            })?;
        let scalar_count = u32::try_from(prefix.chars().count()).map_err(|_overflow| {
            PositionError::Invariant {
                detail: "a bounded source has at most u32::MAX Unicode scalars",
            }
        })?;
        let char_column = scalar_count
            .checked_add(1)
            .ok_or(PositionError::Invariant {
                detail: "a bounded line has a representable one-based character column",
            })?;
        Ok(Position {
            line,
            byte_column,
            char_column,
        })
    }
}

fn bounded_source_len(bytes: usize) -> Result<u32, PositionError> {
    let represented =
        u32::try_from(bytes).map_err(|_overflow| PositionError::SourceTooLarge { bytes })?;
    if represented == u32::MAX {
        return Err(PositionError::SourceTooLarge { bytes });
    }
    Ok(represented)
}

#[cfg(test)]
mod tests {
    use njutest_devkit::result::{ResultState, result_state};

    use super::{PositionError, bounded_source_len};

    fn returned<T: std::fmt::Debug, E: std::fmt::Debug>(result: Result<T, E>) -> Option<T> {
        assert_eq!(
            result_state(&result),
            ResultState::Returned,
            "the closed boundary fixture was refused: {result:?}"
        );
        match result {
            Ok(value) => Some(value),
            Err(_already_reported) => None,
        }
    }

    fn refused<T: std::fmt::Debug, E: std::fmt::Debug>(result: Result<T, E>) -> Option<E> {
        assert_eq!(
            result_state(&result),
            ResultState::Refused,
            "the out-of-range boundary fixture was accepted: {result:?}"
        );
        match result {
            Ok(_already_reported) => None,
            Err(error) => Some(error),
        }
    }

    #[test]
    fn the_one_based_wire_boundary_is_checked_without_allocating_a_giant_source() {
        let largest_accepted =
            usize::try_from(u32::MAX - 1).map_err(|_unrepresentable| PositionError::Invariant {
                detail: "a supported platform represents u32 source offsets as usize",
            });
        let Some(largest_accepted) = returned(largest_accepted) else {
            return;
        };
        let first_refused =
            usize::try_from(u32::MAX).map_err(|_unrepresentable| PositionError::Invariant {
                detail: "a supported platform represents u32 source offsets as usize",
            });
        let Some(first_refused) = returned(first_refused) else {
            return;
        };
        let Some(zero) = returned(bounded_source_len(0)) else {
            return;
        };
        assert_eq!(zero, 0);
        let Some(largest) = returned(bounded_source_len(largest_accepted)) else {
            return;
        };
        assert_eq!(largest, u32::MAX - 1);
        let Some(refusal) = refused(bounded_source_len(first_refused)) else {
            return;
        };
        assert_eq!(
            refusal,
            PositionError::SourceTooLarge {
                bytes: first_refused,
            }
        );
    }
}
