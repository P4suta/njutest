// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Composing the four guard forms from a site's alternatives.

use std::fmt::Write as _;

use crate::syntax::Form;

/// The path a guard calls the runtime through: `super::` once per inline module between the site and the file, since the runtime lives at the file's top level.
#[must_use]
pub(super) fn path(module: &str, super_depth: u32) -> String {
    let mut path = String::new();
    for _ in 0..super_depth {
        path.push_str("super::");
    }
    path.push_str(module);
    path.push_str("::active");
    path
}

/// A composed guard: its text, where each alternative's own text sits in it, and where the original branch does. The offsets are relative to the start of the text.
pub(super) struct Composed {
    pub(super) text: String,
    /// One entry per alternative, in the order given: the mutant index and the byte range its text occupies.
    pub(super) alternatives: Vec<(u32, std::ops::Range<usize>)>,
    /// Where the original branch's text starts.
    pub(super) original_at: usize,
}

/// Composes one guard from its alternatives (index and text, in catalog order) and the original branch.
#[must_use]
pub(super) fn compose(
    form: Form,
    path: &str,
    alternatives: &[(u32, String)],
    original: &str,
) -> Composed {
    match form {
        Form::C => selector(path, alternatives, original),
        Form::E => {
            let mut composed = chain(path, alternatives, original);
            composed.text.insert(0, '(');
            composed.text.push(')');
            for (_, range) in &mut composed.alternatives {
                range.start = range.start.saturating_add(1);
                range.end = range.end.saturating_add(1);
            }
            composed.original_at = composed.original_at.saturating_add(1);
            composed
        }
        Form::S => chain(path, alternatives, original),
        Form::M => arm(path, alternatives, original),
    }
}

/// Form M: the guard an arm did not have, written after the pattern that did not need one.
///
/// Every other form replaces bytes with bytes that say something else. This
/// one keeps the site — the arm's pattern — exactly as it is and writes a
/// guard after it, because there is nothing at an unguarded arm to replace.
/// The branch that keeps the arm as it was is the guard it did without:
/// `true`.
fn arm(path: &str, alternatives: &[(u32, String)], original: &str) -> Composed {
    const KEPT: &str = "true";
    let mut composed = selector(path, alternatives, KEPT);
    let prefix = original.len().saturating_add(" if ".len());
    composed.text.insert_str(0, " if ");
    composed.text.insert_str(0, original);
    for (_, range) in &mut composed.alternatives {
        range.start = range.start.saturating_add(prefix);
        range.end = range.end.saturating_add(prefix);
    }
    composed.original_at = 0;
    composed
}

/// Form C: a boolean selector with no block, so the site introduces no temporary scope of its own. The outer parentheses are load bearing: a nested Form C site sits inside its parent's `&&` chain, where `&&` binds tighter than the `||` this composes.
fn selector(path: &str, alternatives: &[(u32, String)], original: &str) -> Composed {
    let mut text = String::from("(");
    let mut spans = Vec::with_capacity(alternatives.len());
    for (index, alternative) in alternatives {
        let written = write!(text, "{path}({index}) && (");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        let start = text.len();
        text.push_str(alternative);
        spans.push((*index, start..text.len()));
        text.push_str(") || ");
    }
    for (index, _) in alternatives {
        let written = write!(text, "!({path}({index})) && ");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push('(');
    let original_at = text.len();
    text.push_str(original);
    text.push_str("))");
    Composed {
        text,
        alternatives: spans,
        original_at,
    }
}

/// Forms E and S: a branch chain. Both are the same text; only Form E is parenthesised, because it stands where a value does.
fn chain(path: &str, alternatives: &[(u32, String)], original: &str) -> Composed {
    let mut text = String::new();
    let mut spans = Vec::with_capacity(alternatives.len());
    for (index, alternative) in alternatives {
        text.push_str(if text.is_empty() { "if " } else { " else if " });
        let written = write!(text, "{path}({index}) {{ ");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        let start = text.len();
        text.push_str(alternative);
        spans.push((*index, start..text.len()));
        text.push_str(if alternative.is_empty() { "}" } else { " }" });
    }
    if text.is_empty() {
        return Composed {
            text: original.to_owned(),
            alternatives: spans,
            original_at: 0,
        };
    }
    text.push_str(" else { ");
    let original_at = text.len();
    text.push_str(original);
    text.push_str(" }");
    Composed {
        text,
        alternatives: spans,
        original_at,
    }
}
