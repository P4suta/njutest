// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The path matching language that decides which files are mutated.

use std::fmt;

/// A pattern [`Pattern::compile`] refused, with the column of the byte that made it invalid so a command line can underline it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid glob pattern {pattern:?}: {message} (column {column})")]
pub struct GlobError {
    /// The pattern exactly as given.
    pub pattern: String,
    /// The 1-based byte position of the offending byte; 1 for the empty pattern, which has no byte to point at.
    pub column: usize,
    /// The problem, without repeating the pattern.
    pub message: String,
}

/// One `/`-separated piece of a compiled pattern, classified once so the matcher never re-inspects pattern bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Element {
    /// No wildcard at all: compares as a whole string.
    Literal(String),
    /// At least one `*` or `?`: runs the byte matcher.
    Wildcard(String),
    /// Exactly `**`: matches zero or more whole path elements.
    DoubleStar,
}

impl Element {
    fn classify(part: &str) -> Self {
        if part == "**" {
            Self::DoubleStar
        } else if part.contains(['*', '?']) {
            Self::Wildcard(part.to_owned())
        } else {
            Self::Literal(part.to_owned())
        }
    }

    /// Whether a non-`**` element matches one path element, which holds no separator.
    fn matches(&self, segment: &str) -> bool {
        match self {
            Self::Literal(text) => text == segment,
            Self::Wildcard(text) => match_wildcard(text.as_bytes(), segment.as_bytes()),
            Self::DoubleStar => false,
        }
    }
}

/// A compiled matcher, immutable once compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    source: String,
    elements: Vec<Element>,
}

impl Pattern {
    /// Parses a pattern.
    ///
    /// # Errors
    /// Refuses the empty pattern, a leading `/`, a trailing `/`, and an empty element in the middle such as `a//b`.
    pub fn compile(pattern: &str) -> Result<Self, GlobError> {
        let refuse = |column: usize, message: &str| GlobError {
            pattern: pattern.to_owned(),
            column,
            message: message.to_owned(),
        };
        if pattern.is_empty() {
            return Err(refuse(1, "empty pattern"));
        }
        if pattern.starts_with('/') {
            return Err(refuse(
                1,
                "leading '/': patterns are relative to the workspace root",
            ));
        }
        if pattern.ends_with('/') {
            return Err(refuse(
                pattern.len(),
                "trailing '/': write \"/**\" to match a directory and everything under it",
            ));
        }
        let mut elements = Vec::new();
        let mut column = 1usize;
        for part in pattern.split('/') {
            if part.is_empty() {
                return Err(refuse(column, "empty path element between two '/'"));
            }
            elements.push(Element::classify(part));
            column = column.saturating_add(part.len()).saturating_add(1);
        }
        Ok(Self {
            source: pattern.to_owned(),
            elements,
        })
    }

    /// Whether `path` matches.
    /// `path` uses `/` as its only separator; an empty path, or one holding an empty element, matches nothing.
    /// Total: never an error, never a panic.
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.iter().any(|segment| segment.is_empty()) {
            return false;
        }
        match_elements(&self.elements, &segments)
    }
}

/// `next[j]` answers "do the elements from `i + 1` onward match the path from element `j` onward", and `current[j]` the same for `i`.
/// Sweeping `i` backwards over two rows bounds the whole matcher at O(pattern × path); the obvious recursive reading of `**` explores an exponential number of splits on a pattern such as `**/**/**/*a`.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "the two rows have length count + 1 and every index is j <= count by construction"
)]
fn match_elements(elements: &[Element], segments: &[&str]) -> bool {
    let count = segments.len();
    let mut next = vec![false; count + 1];
    let mut current = vec![false; count + 1];
    next[count] = true;
    for element in elements.iter().rev() {
        if *element == Element::DoubleStar {
            current[count] = next[count];
            for j in (0..count).rev() {
                current[j] = next[j] || current[j + 1];
            }
        } else {
            current[count] = false;
            for j in (0..count).rev() {
                current[j] = next[j + 1] && element.matches(segments[j]);
            }
        }
        std::mem::swap(&mut next, &mut current);
    }
    next[0]
}

/// The same two-row dynamic programme one level down: `current[j]` answers "does `pattern[..=i]` match `segment[..j]`".
/// A table rather than a greedy scan keeps `a*a*a*a*b` linear in the product of the lengths instead of exponential in the number of stars.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "the two rows have length len + 1 and every index is j <= len by construction"
)]
fn match_wildcard(pattern: &[u8], segment: &[u8]) -> bool {
    let length = segment.len();
    let mut previous = vec![false; length + 1];
    let mut current = vec![false; length + 1];
    previous[0] = true;
    for &byte in pattern {
        current[0] = byte == b'*' && previous[0];
        for j in 1..=length {
            current[j] = match byte {
                b'*' => previous[j] || current[j - 1],
                b'?' => previous[j - 1],
                other => previous[j - 1] && segment[j - 1] == other,
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[length]
}

impl fmt::Display for Pattern {
    /// The pattern text as compiled, for diagnostics.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}
