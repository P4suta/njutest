// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compiling any pattern and matching any path never panics, and a pattern
//! with no wildcard matches exactly its own spelling.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rust_mutants::glob::Pattern;

#[derive(Debug, Arbitrary)]
struct Input<'a> {
    pattern: &'a str,
    path: &'a str,
}

fuzz_target!(|input: Input<'_>| {
    let Ok(pattern) = Pattern::compile(input.pattern) else {
        return;
    };
    let _matched = pattern.matches(input.path);
    let literal = !input.pattern.contains(['*', '?', '[', '\\']);
    if literal {
        assert!(pattern.matches(input.pattern), "{:?}", input.pattern);
    }
});
