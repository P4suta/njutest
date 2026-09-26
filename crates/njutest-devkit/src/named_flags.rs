// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command-line flags a sentence tells a reader to pass, so a test can hold every one to the parser of the program it names.

/// Every `--flag` `text` names as one of `program`'s: in prose, or in a code span that starts with `program`; a code span that starts with another command names that command's flags.
#[must_use]
pub fn named_flags(text: &str, program: &str) -> Vec<String> {
    text.split('`')
        .enumerate()
        .filter(|(at, part)| at % 2 == 0 || part.trim_start().starts_with(program))
        .flat_map(|(_, part)| flags(part))
        .collect()
}

/// Every `--name` in `part` that starts a word, without what follows an `=`.
fn flags(part: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = part;
    while let Some(at) = rest.find("--") {
        let before = rest.get(..at).and_then(|head| head.chars().next_back());
        let after = rest.get(at..).unwrap_or_default();
        let name: String = after
            .chars()
            .skip(2)
            .take_while(|next| next.is_ascii_lowercase() || next.is_ascii_digit() || *next == '-')
            .collect();
        let starts_a_word =
            before.is_none_or(|previous| !(previous.is_alphanumeric() || previous == '-'));
        if starts_a_word
            && name
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_lowercase())
        {
            found.push(format!("--{name}"));
        }
        rest = after.get(2..).unwrap_or_default();
    }
    found
}
