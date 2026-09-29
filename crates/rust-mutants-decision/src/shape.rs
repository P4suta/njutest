// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a site's text ends at its own closing brace, which decides whether the identity macro can hold a guard over it.

/// The words an expression that ends at its own closing brace can begin with.
const OPENERS: [&str; 8] = [
    "if", "match", "loop", "while", "for", "unsafe", "const", "async",
];

/// Whether `text` begins the way an expression ending at its own closing brace does, which ends a statement or an arm there.
///
/// A guard over such an expression has to end the same way, and what a guard places at the start of a block has to be held so it cannot: the identity macro holds it, and the guard is a chain rather than a macro call.
/// Reading only the first token errs toward holding: an expression that merely starts with a block, like `{ a } + b`, is held too, which is harmless wherever it stood.
#[must_use]
pub fn opens_a_block(text: &str) -> bool {
    if text.starts_with(['{', '#', '\'']) {
        return true;
    }
    let word = match text
        .split_once(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
    {
        Some((word, _)) => word,
        None => text,
    };
    OPENERS.contains(&word)
}

#[cfg(test)]
mod tests;
