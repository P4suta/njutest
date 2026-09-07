// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The unified diff of one edit, so a reader sees the mutation as a change rather than as two quoted strings.

use std::fmt::Write as _;

/// How many unchanged lines are shown either side of a change.
pub const CONTEXT: usize = 3;

/// The unified diff between two texts, with `CONTEXT` lines either side of what differs.
///
/// One mutation is one contiguous edit, so the diff is one hunk: what the
/// engine has to show is the lines the edit is on, in the place a reader would
/// open the file to. It compares whole lines, which is what a reviewer reads.
#[must_use]
pub fn unified(path: &str, before: &str, after: &str) -> String {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let head = old
        .iter()
        .zip(&new)
        .take_while(|(one, other)| one == other)
        .count();
    if head == old.len() && old.len() == new.len() {
        return String::new();
    }
    let tail = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(one, other)| one == other)
        .count()
        .min(old.len().saturating_sub(head))
        .min(new.len().saturating_sub(head));
    let from = head.saturating_sub(CONTEXT);
    let old_end = old
        .len()
        .saturating_sub(tail)
        .saturating_add(CONTEXT)
        .min(old.len());
    let new_end = new
        .len()
        .saturating_sub(tail)
        .saturating_add(CONTEXT)
        .min(new.len());
    let mut text = format!("--- a/{path}\n+++ b/{path}\n");
    let written = writeln!(
        text,
        "@@ -{},{} +{},{} @@",
        from.saturating_add(1),
        old_end.saturating_sub(from),
        from.saturating_add(1),
        new_end.saturating_sub(from)
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    for line in old.get(from..head).unwrap_or_default() {
        let written = writeln!(text, " {line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    for line in old
        .get(head..old.len().saturating_sub(tail))
        .unwrap_or_default()
    {
        let written = writeln!(text, "-{line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    for line in new
        .get(head..new.len().saturating_sub(tail))
        .unwrap_or_default()
    {
        let written = writeln!(text, "+{line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    for line in old
        .get(old.len().saturating_sub(tail)..old_end)
        .unwrap_or_default()
    {
        let written = writeln!(text, " {line}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text
}

/// The text one mutation makes of `source`, or nothing when the source is not the one the mutation was taken from.
#[must_use]
pub fn mutated(source: &str, start: u32, end: u32, replacement: &str) -> Option<String> {
    let from = usize::try_from(start).ok()?;
    let to = usize::try_from(end).ok()?;
    let head = source.get(..from)?;
    let tail = source.get(to..)?;
    Some(format!("{head}{replacement}{tail}"))
}
