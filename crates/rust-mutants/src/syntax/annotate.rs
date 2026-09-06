// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The markers an author writes into the source to say what a run should pass over, and why.

use proc_macro2::{TokenStream, TokenTree};

use super::position::LineIndex;

/// The word a marker opens with.
pub(super) const PREFIX: &str = "rust-mutants:";

/// The one directive a marker may carry.
pub(super) const DIRECTIVE: &str = "skip";

/// One marker, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Marker {
    /// The byte offset the marker's comment starts at.
    pub offset: u32,
    /// The 1-based line the comment sits on.
    pub line: u32,
    /// The line the marker speaks about: its own when something else shares it, the next one when it does not.
    pub scope: u32,
    /// Whether the marker had the line to itself, which is what makes it speak about what follows rather than about what shares it.
    pub own_line: bool,
    /// The reason its author wrote.
    pub reason: String,
}

/// A marker the engine cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MarkerError {
    /// The marker names no reason.
    WithoutReason {
        /// The 1-based line.
        line: u32,
    },
    /// The marker names a directive this release does not know.
    Unknown {
        /// The 1-based line.
        line: u32,
        /// The directive as written.
        directive: String,
    },
}

/// The file a scan reads markers out of.
#[derive(Debug, Clone, Copy)]
struct Source<'a> {
    /// The whole text, byte order mark and shebang included.
    text: &'a str,
    /// The byte offset the parsed remainder starts at, which is what token spans are relative to.
    base: usize,
    /// The lines of the whole text.
    index: &'a LineIndex,
}

/// Every marker in one file, in source order.
///
/// A comment is what the lexer threw away, so the markers are read out of the
/// gaps between the tokens rather than out of the text: bytes inside a string
/// literal are a token's, and a marker spelled there is a string that says the
/// words. A documentation comment is a `#[doc]` attribute by the time the
/// tokens exist, so its bytes are a token's too and it is never a marker.
///
/// # Errors
/// Returns the first marker that names no reason or an unknown directive.
pub(super) fn markers(
    text: &str,
    base: u32,
    stream: &TokenStream,
    index: &LineIndex,
) -> Result<Vec<Marker>, MarkerError> {
    let source = Source {
        text,
        base: usize::try_from(base).unwrap_or(usize::MAX),
        index,
    };
    let mut covered = Vec::new();
    collect(stream, &mut covered);
    covered.sort_unstable();
    let mut found = Vec::new();
    let mut cursor = 0usize;
    let last = text.len().saturating_sub(source.base);
    for (start, end) in covered {
        if start > cursor {
            read_gap(source, cursor..start, &mut found)?;
        }
        cursor = cursor.max(end);
    }
    if cursor < last {
        read_gap(source, cursor..last, &mut found)?;
    }
    Ok(found)
}

/// Every leaf token's byte range, group delimiters included.
fn collect(stream: &TokenStream, into: &mut Vec<(usize, usize)>) {
    for tree in stream.clone() {
        match tree {
            TokenTree::Group(group) => {
                let range = group.span().byte_range();
                let open = group.span_open().byte_range();
                let close = group.span_close().byte_range();
                if open.start == close.start {
                    into.push((range.start, range.end));
                } else {
                    into.push((open.start, open.end));
                    into.push((close.start, close.end));
                }
                collect(&group.stream(), into);
            }
            other => {
                let range = other.span().byte_range();
                into.push((range.start, range.end));
            }
        }
    }
}

/// The markers in one stretch of text no token covers.
fn read_gap(
    source: Source<'_>,
    range: std::ops::Range<usize>,
    into: &mut Vec<Marker>,
) -> Result<(), MarkerError> {
    let from = source.base.saturating_add(range.start);
    let to = source.base.saturating_add(range.end).min(source.text.len());
    let Some(gap) = source.text.get(from..to) else {
        return Ok(());
    };
    let mut at = 0usize;
    while at < gap.len() {
        let rest = gap.get(at..).unwrap_or_default();
        if let Some(body) = rest.strip_prefix("//") {
            let length = body.find('\n').unwrap_or(body.len());
            let content = body.get(..length).unwrap_or_default();
            read_marker(source, from.saturating_add(at), content, into)?;
            at = at.saturating_add(2).saturating_add(length);
        } else if let Some(body) = rest.strip_prefix("/*") {
            let length = body.find("*/").unwrap_or(body.len());
            let content = body.get(..length).unwrap_or_default();
            read_marker(source, from.saturating_add(at), content, into)?;
            at = at.saturating_add(4).saturating_add(length);
        } else {
            at = at.saturating_add(rest.chars().next().map_or(1, char::len_utf8));
        }
    }
    Ok(())
}

/// One comment, which is a marker when it opens with the word.
fn read_marker(
    source: Source<'_>,
    at: usize,
    content: &str,
    into: &mut Vec<Marker>,
) -> Result<(), MarkerError> {
    let Some(rest) = content.trim_start().strip_prefix(PREFIX) else {
        return Ok(());
    };
    let offset = u32::try_from(at).unwrap_or(u32::MAX);
    let line = source.index.position(source.text, offset).line;
    let said = rest.trim();
    let (directive, reason) = said.split_once(char::is_whitespace).unwrap_or((said, ""));
    if directive != DIRECTIVE {
        if directive.is_empty() {
            return Err(MarkerError::WithoutReason { line });
        }
        return Err(MarkerError::Unknown {
            line,
            directive: directive.to_owned(),
        });
    }
    let reason = reason.trim().trim_end_matches('*').trim_end();
    if reason.is_empty() {
        return Err(MarkerError::WithoutReason { line });
    }
    let own_line = source
        .text
        .get(..at)
        .and_then(|before| {
            before
                .rsplit_once('\n')
                .map_or(Some(before), |(_, last)| Some(last))
        })
        .is_some_and(|last| last.trim().is_empty());
    into.push(Marker {
        offset,
        line,
        scope: if own_line {
            line.saturating_add(1)
        } else {
            line
        },
        own_line,
        reason: reason.to_owned(),
    });
    Ok(())
}
