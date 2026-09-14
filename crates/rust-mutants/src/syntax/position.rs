// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Byte offsets to lines and columns.

use serde::{Deserialize, Serialize};

/// Where a byte offset falls in a file, for the console, the report, and the editors that consume `file:line:col`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    /// 1-based line.
    pub line: u32,
    /// 1-based byte offset within the line.
    pub byte_column: u32,
    /// 1-based character (Unicode scalar) offset within the line.
    pub char_column: u32,
}

/// The byte offset of the start of every line, for offset-to-position lookups in constant-ish time.
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<u32>,
}

impl LineIndex {
    /// Indexes `source`.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut starts = vec![0];
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(u32::try_from(offset.saturating_add(1)).unwrap_or(u32::MAX));
            }
        }
        Self { starts }
    }

    /// The position of byte `offset` in `source`, which must be the text the index was built from. An offset past the end lands on the last line.
    #[must_use]
    pub fn position(&self, source: &str, offset: u32) -> Position {
        let line = self.starts.partition_point(|&start| start <= offset);
        let line_start = line
            .checked_sub(1)
            .and_then(|index| self.starts.get(index))
            .copied()
            .unwrap_or(0);
        let byte_column = offset.saturating_sub(line_start);
        let prefix = source
            .get(
                usize::try_from(line_start).unwrap_or(usize::MAX)
                    ..usize::try_from(offset).unwrap_or(usize::MAX),
            )
            .unwrap_or_default();
        let char_column = u32::try_from(prefix.chars().count()).unwrap_or(u32::MAX);
        Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            byte_column: byte_column.saturating_add(1),
            char_column: char_column.saturating_add(1),
        }
    }
}
