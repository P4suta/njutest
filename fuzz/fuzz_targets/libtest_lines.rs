// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The per-test lines of a harness say which test noticed a mutation, which is the sentence a report hands a person. A reader that invents a name, or reads one test as two, credits a test that never ran. It never panics, every name it reports is text that was there, and no name is counted twice for one line.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::execute::parse_lines;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(lines) = parse_lines(data) else {
        return;
    };
    let counted = lines
        .passed
        .len()
        .checked_add(lines.failed.len())
        .and_then(|count| count.checked_add(lines.ignored.len()))
        .unwrap_or(usize::MAX);
    assert!(
        counted <= text.lines().count(),
        "{counted} verdicts from {} lines",
        text.lines().count()
    );
    for name in lines
        .passed
        .iter()
        .chain(&lines.failed)
        .chain(&lines.ignored)
    {
        assert!(!name.is_empty(), "a test with no name");
        assert!(text.contains(name.as_str()), "{name:?} is in no line");
    }
    assert!(
        matches!(parse_lines(data), Ok(again) if again == lines),
        "reading is a function of the bytes"
    );
});
