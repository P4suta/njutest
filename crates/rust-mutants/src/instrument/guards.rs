// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Composing the four guard forms from a site's alternatives.

use std::fmt::Write as _;

use crate::syntax::Form;

/// The path a guard calls the runtime through: `super::` once per inline module between the site and the file, since the runtime lives at the file's top level.
#[must_use]
pub(super) fn path(module: &str, super_depth: u32) -> String {
    named(module, super_depth, "active")
}

/// The path one of the runtime's functions is called by from a site `super_depth` inline modules down.
#[must_use]
pub(super) fn named(module: &str, super_depth: u32, function: &str) -> String {
    let mut path = String::new();
    for _ in 0..super_depth {
        path.push_str("super::");
    }
    path.push_str(module);
    path.push_str("::");
    path.push_str(function);
    path
}

/// One alternative at a site: which mutant it is, what it reads, and whether the compiler vouched for comparing it with what it replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Alternative {
    /// The mutant's dense catalog index.
    pub(super) index: u32,
    /// The text the guard writes when this mutant is the live one.
    pub(super) text: String,
    /// Whether a run may evaluate this beside the original and record whether the two ever differed.
    pub(super) comparable: bool,
}

/// A composed guard: its text, where each alternative's own text sits in it, and where the original branch does. The offsets are relative to the start of the text.
pub(super) struct Composed {
    pub(super) text: String,
    /// One entry per alternative, in the order given: the mutant index and the byte range its text occupies.
    pub(super) alternatives: Vec<(u32, std::ops::Range<usize>)>,
    /// Where the original branch's text starts.
    pub(super) original_at: usize,
    /// Every mutant this guard evaluates beside what it replaces, ascending. A form that cannot compare reports none, whatever it was offered.
    pub(super) compared: Vec<u32>,
}

/// Composes one guard from its alternatives (index and text, in catalog order) and the original branch.
#[must_use]
pub(super) fn compose(
    form: Form,
    paths: &Paths<'_>,
    alternatives: &[Alternative],
    original: &str,
) -> Composed {
    let path = paths.active;
    match form {
        Form::C => selector(paths, alternatives, original),
        Form::E => {
            let mut composed = chain(path, alternatives, original);
            let (open, close) = wrapping(original);
            composed.text.insert(0, open);
            composed.text.push(close);
            for (_, range) in &mut composed.alternatives {
                range.start = range.start.saturating_add(1);
                range.end = range.end.saturating_add(1);
            }
            composed.original_at = composed.original_at.saturating_add(1);
            composed
        }
        Form::S => chain(path, alternatives, original),
        Form::M => arm(paths, alternatives, original),
    }
}

/// What a value-position guard is wrapped in, which is a block wherever the site already was one.
///
/// Parentheses are what makes a guard one expression wherever the site was
/// one. They are the wrong wrapper for a site that is already a block: a match
/// arm whose body is a block needs no comma after it, and one whose body is a
/// parenthesised expression does — so wrapping a block-bodied arm in
/// parentheses turns a file that parsed into one that does not, at the arm
/// *after* it. A block wrapped in a block is still a block, and is an
/// expression everywhere the original block was one.
fn wrapping(original: &str) -> (char, char) {
    let trimmed = original.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        ('{', '}')
    } else {
        ('(', ')')
    }
}

/// Form M: the guard an arm did not have, written after the pattern that did not need one.
///
/// Every other form replaces bytes with bytes that say something else. This
/// one keeps the site — the arm's pattern — exactly as it is and writes a
/// guard after it, because there is nothing at an unguarded arm to replace.
/// The branch that keeps the arm as it was is the guard it did without:
/// `true`.
fn arm(paths: &Paths<'_>, alternatives: &[Alternative], original: &str) -> Composed {
    const KEPT: &str = "true";
    let mut composed = selector(paths, alternatives, KEPT);
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

/// The two runtime functions a guard calls, each by the path its site reaches the module through.
#[derive(Debug, Clone, Copy)]
pub(super) struct Paths<'a> {
    /// The function that says whether this mutant is the live one.
    pub(super) active: &'a str,
    /// The function that records a mutant whose branch differed from what it replaces, and answers what it replaces.
    pub(super) differing: &'a str,
}

/// Form C: a boolean selector with no block, so the site introduces no temporary scope of its own. The outer parentheses are load bearing: a nested Form C site sits inside its parent's `&&` chain, where `&&` binds tighter than the `||` this composes.
///
/// A comparable alternative wraps the original branch in a call that answers
/// what the original answers and records the mutant when the two differ. It is
/// a call and not a block for the same reason the rest of this form is an
/// expression: a block here would be a temporary scope the site did not have.
fn selector(paths: &Paths<'_>, alternatives: &[Alternative], original: &str) -> Composed {
    let path = paths.active;
    let mut text = String::from("(");
    let mut spans = Vec::with_capacity(alternatives.len());
    for one in alternatives {
        let written = write!(text, "{path}({}) && (", one.index);
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        let start = text.len();
        text.push_str(&one.text);
        spans.push((one.index, start..text.len()));
        text.push_str(") || ");
    }
    for one in alternatives {
        let written = write!(text, "!({path}({})) && ", one.index);
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    let comparable: Vec<&Alternative> = alternatives.iter().filter(|one| one.comparable).collect();
    for one in &comparable {
        let written = write!(text, "{}({}, ", paths.differing, one.index);
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push('(');
    let original_at = text.len();
    text.push_str(original);
    text.push(')');
    for one in comparable.iter().rev() {
        let written = write!(text, ", || ({}))", one.text);
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push(')');
    Composed {
        text,
        alternatives: spans,
        original_at,
        compared: comparable.iter().map(|one| one.index).collect(),
    }
}

/// Forms E and S: a branch chain. Both are the same text; only Form E is parenthesised, because it stands where a value does.
fn chain(path: &str, alternatives: &[Alternative], original: &str) -> Composed {
    let mut text = String::new();
    let mut spans = Vec::with_capacity(alternatives.len());
    for one in alternatives {
        let (index, alternative) = (one.index, &one.text);
        text.push_str(if text.is_empty() { "if " } else { " else if " });
        let written = write!(text, "{path}({index}) {{ ");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
        let start = text.len();
        text.push_str(alternative);
        spans.push((index, start..text.len()));
        text.push_str(if alternative.is_empty() { "}" } else { " }" });
    }
    if text.is_empty() {
        return Composed {
            text: original.to_owned(),
            alternatives: spans,
            original_at: 0,
            compared: Vec::new(),
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
        compared: Vec::new(),
    }
}
