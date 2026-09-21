// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The unified diff of one edit, so a reader sees the mutation as a change rather than as two quoted strings.

use std::fmt::Write as _;

/// How many unchanged lines are shown either side of a change.
pub const CONTEXT: usize = 3;

/// Why one diff could not be represented exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the unified diff's line geometry is inconsistent or exceeds usize")]
pub struct DiffError;

/// The unified diff between two texts, with `CONTEXT` lines either side of what differs.
///
/// # Errors
/// Refuses inconsistent or overflowing line geometry rather than emitting a
/// valid-looking truncated hunk.
pub fn unified(path: &str, before: &str, after: &str) -> Result<String, DiffError> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let head = old
        .iter()
        .zip(&new)
        .take_while(|(one, other)| one == other)
        .count();
    if head == old.len() && old.len() == new.len() {
        return Ok(String::new());
    }
    let old_after_head = old.len().checked_sub(head).ok_or(DiffError)?;
    let new_after_head = new.len().checked_sub(head).ok_or(DiffError)?;
    let tail = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(one, other)| one == other)
        .count()
        .min(old_after_head)
        .min(new_after_head);
    let from = match head.checked_sub(CONTEXT) {
        Some(from) => from,
        None => 0,
    };
    let old_tail_start = old.len().checked_sub(tail).ok_or(DiffError)?;
    let new_tail_start = new.len().checked_sub(tail).ok_or(DiffError)?;
    let old_end = context_end(old_tail_start, old.len());
    let new_end = context_end(new_tail_start, new.len());
    let first_line = from.checked_add(1).ok_or(DiffError)?;
    let old_count = old_end.checked_sub(from).ok_or(DiffError)?;
    let new_count = new_end.checked_sub(from).ok_or(DiffError)?;
    let mut text = format!("--- a/{path}\n+++ b/{path}\n");
    let written = writeln!(
        text,
        "@@ -{first_line},{old_count} +{first_line},{new_count} @@"
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    for line in old.get(from..head).ok_or(DiffError)? {
        let written = writeln!(text, " {line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    for line in old.get(head..old_tail_start).ok_or(DiffError)? {
        let written = writeln!(text, "-{line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    for line in new.get(head..new_tail_start).ok_or(DiffError)? {
        let written = writeln!(text, "+{line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    for line in old.get(old_tail_start..old_end).ok_or(DiffError)? {
        let written = writeln!(text, " {line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    Ok(text)
}

/// Extends a hunk to its requested context, clipping only at the real end of
/// the file. Arithmetic overflow means the real end is necessarily nearer
/// than the requested context and therefore has the same clipped answer.
fn context_end(change_end: usize, file_end: usize) -> usize {
    match change_end.checked_add(CONTEXT) {
        Some(with_context) => with_context.min(file_end),
        None => file_end,
    }
}

/// The text one mutation makes of `source`, or nothing when the source is not the one the mutation was taken from.
#[must_use]
pub fn mutated(source: &str, start: u32, end: u32, replacement: &str) -> Option<String> {
    let Ok(from) = usize::try_from(start) else {
        return None;
    };
    let Ok(to) = usize::try_from(end) else {
        return None;
    };
    let head = source.get(..from)?;
    let tail = source.get(to..)?;
    Some(format!("{head}{replacement}{tail}"))
}
