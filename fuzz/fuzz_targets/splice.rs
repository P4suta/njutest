// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Applying any set of splices never panics; an accepted set produces an output whose offset map is monotone and whose length matches.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rust_mutants::span::Span;
use rust_mutants::splice::{Splice, apply};

#[derive(Debug, Arbitrary)]
struct Edit {
    start: u8,
    len: u8,
    replacement: Vec<u8>,
}

#[derive(Debug, Arbitrary)]
struct Input {
    source: Vec<u8>,
    edits: Vec<Edit>,
}

fuzz_target!(|input: Input| {
    let source_len = u32::try_from(input.source.len()).unwrap_or(u32::MAX);
    let splices: Vec<Splice> = input
        .edits
        .iter()
        .take(8)
        .map(|edit| {
            let start = u32::from(edit.start);
            let end = start.saturating_add(u32::from(edit.len));
            let original = input
                .source
                .get(start as usize..end.min(source_len) as usize)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            Splice {
                span: Span::new(start, end).expect("end >= start by construction"),
                original,
                replacement: edit.replacement.clone(),
            }
        })
        .collect();
    let Ok((output, map)) = apply(&input.source, &splices) else {
        return;
    };
    assert_eq!(map.out_len() as usize, output.len());
    assert_eq!(map.src_len(), source_len);
    let mut last = 0;
    for offset in 0..=source_len {
        let (mapped, _exact) = map.to_output(offset);
        assert!(mapped >= last, "offset map is monotone");
        assert!(mapped <= map.out_len());
        last = mapped;
    }
});
