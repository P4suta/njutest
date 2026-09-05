// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Composing the three guard forms from a site's alternatives.

use std::fmt::Write as _;

use crate::syntax::Form;

/// The path a guard calls the runtime through: `super::` once per inline
/// module between the site and the file, since the runtime lives at the
/// file's top level.
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

/// Composes one guard from its alternatives (index and text, in catalog
/// order) and the original branch.
#[must_use]
pub(super) fn compose(
    form: Form,
    path: &str,
    alternatives: &[(u32, String)],
    original: &str,
) -> String {
    match form {
        Form::C => selector(path, alternatives, original),
        Form::E => format!("({})", chain(path, alternatives, original)),
        Form::S => chain(path, alternatives, original),
    }
}

/// Form C: a boolean selector with no block, so the site introduces no
/// temporary scope of its own. The outer parentheses are load bearing: a
/// nested Form C site sits inside its parent's `&&` chain, where `&&` binds
/// tighter than the `||` this composes.
fn selector(path: &str, alternatives: &[(u32, String)], original: &str) -> String {
    let mut clauses: Vec<String> = alternatives
        .iter()
        .map(|(index, text)| format!("{path}({index}) && ({text})"))
        .collect();
    let mut fallback: Vec<String> = alternatives
        .iter()
        .map(|(index, _)| format!("!({path}({index}))"))
        .collect();
    fallback.push(format!("({original})"));
    clauses.push(fallback.join(" && "));
    format!("({})", clauses.join(" || "))
}

/// Forms E and S: a branch chain. Both are the same text; only Form E is
/// parenthesised by its caller, because it stands where a value does.
fn chain(path: &str, alternatives: &[(u32, String)], original: &str) -> String {
    let mut text = String::new();
    for (index, alternative) in alternatives {
        if text.is_empty() {
            text.push_str("if ");
        } else {
            text.push_str(" else if ");
        }
        if alternative.is_empty() {
            // A deletion: the branch that runs nothing.
            let written = write!(text, "{path}({index}) {{ }}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        } else {
            let written = write!(text, "{path}({index}) {{ {alternative} }}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    if text.is_empty() {
        return original.to_owned();
    }
    let written = write!(text, " else {{ {original} }}");
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    text
}
