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
    let lines = parse_lines(data);
    let counted = lines
        .passed
        .len()
        .saturating_add(lines.failed.len())
        .saturating_add(lines.ignored.len());
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
    assert_eq!(parse_lines(data), lines, "reading is a function of the bytes");
});
