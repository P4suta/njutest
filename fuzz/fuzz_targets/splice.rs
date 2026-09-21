// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Applying any set of splices never panics; an accepted set produces an output whose offset map is monotone and whose length matches.

#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use rust_mutants::span::Span;
use rust_mutants::splice::{Splice, apply};

#[derive(Debug)]
struct Edit {
    start: u8,
    len: u8,
    replacement: Vec<u8>,
}

impl<'a> Arbitrary<'a> for Edit {
    fn arbitrary(input: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            start: u8::arbitrary(input)?,
            len: u8::arbitrary(input)?,
            replacement: Vec::<u8>::arbitrary(input)?,
        })
    }
}

#[derive(Debug)]
struct Input {
    source: Vec<u8>,
    edits: Vec<Edit>,
}

impl<'a> Arbitrary<'a> for Input {
    fn arbitrary(input: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            source: Vec::<u8>::arbitrary(input)?,
            edits: Vec::<Edit>::arbitrary(input)?,
        })
    }
}

fuzz_target!(|input: Input| {
    #[expect(
        clippy::manual_unwrap_or,
        reason = "the explicit match documents that inputs larger than the offset format saturate"
    )]
    let source_len = match u32::try_from(input.source.len()) {
        Ok(length) => length,
        Err(_too_long) => u32::MAX,
    };
    let splices: Vec<Splice> = input
        .edits
        .iter()
        .take(8)
        .map(|edit| {
            let start = u32::from(edit.start);
            let end = start.saturating_add(u32::from(edit.len));
            let start_index = usize::from(edit.start);
            let end_index = start_index
                .saturating_add(usize::from(edit.len))
                .min(input.source.len());
            let original = input
                .source
                .get(start_index..end_index)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            #[expect(
                clippy::expect_used,
                reason = "u8 addition above constructs end greater than or equal to start"
            )]
            let span = Span::new(start, end).expect("end >= start by construction");
            Splice {
                span,
                original,
                replacement: edit.replacement.clone(),
            }
        })
        .collect();
    let Ok((output, map)) = apply(&input.source, &splices) else {
        return;
    };
    let Ok(output_length) = usize::try_from(map.out_len()) else {
        return;
    };
    assert_eq!(output_length, output.len());
    assert_eq!(map.src_len(), source_len);
    let mut last = 0;
    for offset in 0..=source_len {
        let (mapped, exact) = map.to_output(offset);
        std::hint::black_box(exact);
        assert!(mapped >= last, "offset map is monotone");
        assert!(mapped <= map.out_len());
        last = mapped;
    }
});
