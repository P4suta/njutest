// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compiling any pattern and matching any path never panics, and a pattern with no wildcard matches exactly its own spelling.

#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use rust_mutants::glob::Pattern;

#[derive(Debug)]
struct Input<'a> {
    pattern: &'a str,
    path: &'a str,
}

impl<'a> Arbitrary<'a> for Input<'a> {
    fn arbitrary(input: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            pattern: <&str>::arbitrary(input)?,
            path: <&str>::arbitrary(input)?,
        })
    }
}

fuzz_target!(|input: Input<'_>| {
    let Ok(pattern) = Pattern::compile(input.pattern) else {
        return;
    };
    let matched = pattern.matches(input.path);
    let literal = !input.pattern.contains(['*', '?', '[', '\\']);
    if literal {
        assert!(matched == (input.path == input.pattern), "{:?}", input.pattern);
        assert!(pattern.matches(input.pattern), "{:?}", input.pattern);
    } else {
        std::hint::black_box(matched);
    }
});
