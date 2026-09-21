// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The parts of a line of Rust, so a reader's eye goes to the code rather than across it.

/// What one run of characters in a line of Rust is.
///
/// Enough of the language to colour a line and no more: a diagnostic shows a handful of lines, and a reader looking at them wants the string literals to look like string literals.
/// Anything that needs a parser to tell apart is left as ordinary code, because guessing wrong is worse than not colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// A word the language reserves.
    Keyword,
    /// A name that begins with a capital, which in Rust is a type by convention.
    Type,
    /// A string or a character.
    Text,
    /// A number.
    Number,
    /// A comment.
    Aside,
    /// A name being called, which is what a line of code is usually about.
    Call,
    /// A lifetime or an attribute.
    Marker,
    /// Everything else.
    Code,
}

/// The words Rust reserves, which a reader recognises before they read.
const RESERVED: [&str; 39] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "union",
    "unsafe", "use", "where", "while",
];

/// `line` cut into the parts a reader tells apart at a glance.
#[must_use]
pub fn parts(line: &str) -> Vec<(Part, String)> {
    let characters: Vec<char> = line.chars().collect();
    let mut found: Vec<(Part, String)> = Vec::new();
    let mut at = 0usize;
    while at < characters.len() {
        let one = characters.get(at).copied().unwrap_or_default();
        let next = characters.get(at.saturating_add(1)).copied();
        if one == '/' && next == Some('/') {
            found.push((
                Part::Aside,
                characters.get(at..).unwrap_or_default().iter().collect(),
            ));
            break;
        }
        if one == '"' || one == '\'' {
            let (text, to) = quoted(&characters, at, one);
            found.push((part_of_quote(one, &text), text));
            at = to;
            continue;
        }
        if one.is_ascii_digit() {
            let to = run(&characters, at, |it| {
                it.is_ascii_alphanumeric() || it == '_' || it == '.'
            });
            found.push((
                Part::Number,
                characters.get(at..to).unwrap_or_default().iter().collect(),
            ));
            at = to;
            continue;
        }
        if one == '#' {
            let to = run(&characters, at, |it| it != ' ');
            found.push((
                Part::Marker,
                characters.get(at..to).unwrap_or_default().iter().collect(),
            ));
            at = to;
            continue;
        }
        if one.is_alphabetic() || one == '_' {
            let to = run(&characters, at, |it| it.is_alphanumeric() || it == '_');
            let word: String = characters.get(at..to).unwrap_or_default().iter().collect();
            found.push((worded(&word, characters.get(to).copied()), word));
            at = to;
            continue;
        }
        let to = run(&characters, at, |it| {
            !it.is_alphanumeric() && it != '_' && it != '"' && it != '\'' && it != '#' && it != '/'
        });
        let to = if to == at { at.saturating_add(1) } else { to };
        found.push((
            Part::Code,
            characters.get(at..to).unwrap_or_default().iter().collect(),
        ));
        at = to;
    }
    found
}

/// Which part one word is, which its shape and what follows it decide.
fn worded(word: &str, after: Option<char>) -> Part {
    if RESERVED.contains(&word) {
        return Part::Keyword;
    }
    if after == Some('(') || after == Some('!') {
        return Part::Call;
    }
    if word.starts_with(char::is_uppercase) {
        return Part::Type;
    }
    Part::Code
}

/// Whether a quote opened a lifetime or a piece of text.
fn part_of_quote(opening: char, text: &str) -> Part {
    if opening == '\'' && !text.ends_with('\'') {
        Part::Marker
    } else {
        Part::Text
    }
}

/// The quoted run beginning at `from`, and where it ends.
fn quoted(characters: &[char], from: usize, opening: char) -> (String, usize) {
    let mut at = from.saturating_add(1);
    let mut escaped = false;
    while at < characters.len() {
        let one = characters.get(at).copied().unwrap_or_default();
        at = at.saturating_add(1);
        if escaped {
            escaped = false;
            continue;
        }
        if one == '\\' {
            escaped = true;
            continue;
        }
        if one == opening {
            break;
        }
        if opening == '\'' && !(one.is_alphanumeric() || one == '_') {
            at = at.saturating_sub(1);
            break;
        }
    }
    (
        characters
            .get(from..at)
            .unwrap_or_default()
            .iter()
            .collect(),
        at,
    )
}

/// Where the run beginning at `from` stops holding.
fn run(characters: &[char], from: usize, holds: impl Fn(char) -> bool) -> usize {
    let mut at = from;
    while characters.get(at).copied().is_some_and(&holds) {
        at = at.saturating_add(1);
    }
    at
}
