// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Splicing: refuses to edit what it cannot verify, applies in span order,
//! and answers offset questions in both directions.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use proptest::prelude::*;
use rust_mutants::span::{Span, SpanError};
use rust_mutants::splice::{OffsetMap, Splice, SpliceError, apply, count_lines, line_preserving};

fn span(start: u32, end: u32) -> Span {
    Span::new(start, end).expect("well formed")
}

fn splice(start: u32, end: u32, original: &[u8], replacement: &[u8]) -> Splice {
    Splice {
        span: span(start, end),
        original: original.to_vec(),
        replacement: replacement.to_vec(),
    }
}

const SRC: &[u8] = b"let x = a + b;\n";

#[test]
fn no_splices_copy_the_source_and_build_the_identity_map() {
    let (out, map) = apply(SRC, &[]).expect("applies");
    assert_eq!(out, SRC);
    assert_eq!(map.src_len(), 15);
    assert_eq!(map.out_len(), 15);
    assert_eq!(map.splices(), 0);
    assert_eq!(map.to_output(7), (7, true));
    assert_eq!(map.to_original(7), (7, true));
    assert_eq!(map.to_output(16), (15, false), "past the end is inexact");
    assert_eq!(OffsetMap::default().to_output(0), (0, true));
}

#[test]
fn splices_apply_in_span_order_whichever_order_they_are_given() {
    let sub = splice(10, 11, b"+", b"-");
    let name = splice(4, 5, b"x", b"total");
    let (forward, _) = apply(SRC, &[name.clone(), sub.clone()]).expect("applies");
    let (backward, _) = apply(SRC, &[sub, name]).expect("applies");
    assert_eq!(forward, b"let total = a - b;\n");
    assert_eq!(forward, backward);
}

#[test]
fn an_empty_span_inserts_and_an_empty_replacement_deletes() {
    let (out, _) =
        apply(SRC, &[splice(8, 8, b"", b"("), splice(13, 13, b"", b")")]).expect("inserts");
    assert_eq!(out, b"let x = (a + b);\n");
    let (out, _) = apply(SRC, &[splice(8, 14, b"a + b;", b"")]).expect("deletes");
    assert_eq!(out, b"let x = \n");
}

#[test]
fn a_splice_whose_original_does_not_match_is_refused_naming_both_sides() {
    let error = apply(SRC, &[splice(10, 11, b"-", b"+")]).expect_err("mismatch");
    match error {
        SpliceError::Mismatch {
            index,
            span: got,
            covered,
            original,
        } => {
            assert_eq!(index, 0);
            assert_eq!(got, span(10, 11));
            assert_eq!(covered, "\"+\"");
            assert_eq!(original, "\"-\"");
        }
        other => panic!("{other:?}"),
    }
    let long = vec![b'x'; 100];
    let error = apply(&long, &[splice(0, 100, &[b'y'; 100], b"")]).expect_err("mismatch");
    let SpliceError::Mismatch { covered, .. } = error else {
        panic!("{error:?}")
    };
    assert!(
        covered.ends_with("\u{2026} (100 bytes)"),
        "long spans are shortened: {covered}"
    );
}

#[test]
fn a_splice_that_does_not_fit_the_source_is_refused_with_its_index() {
    let error = apply(SRC, &[splice(0, 1, b"l", b"L"), splice(14, 20, b"", b"")])
        .expect_err("out of range");
    assert!(
        matches!(
            error,
            SpliceError::Span {
                index: 1,
                source: SpanError::OutOfRange { .. }
            }
        ),
        "{error:?}"
    );
    let reversed = Splice {
        span: Span { start: 5, end: 2 },
        original: Vec::new(),
        replacement: Vec::new(),
    };
    let error = apply(SRC, &[reversed]).expect_err("reversed");
    assert!(
        matches!(
            error,
            SpliceError::Span {
                index: 0,
                source: SpanError::Reversed { .. }
            }
        ),
        "{error:?}"
    );
}

#[test]
fn overlapping_splices_are_refused() {
    let error =
        apply(SRC, &[splice(4, 5, b"x", b"y"), splice(4, 5, b"x", b"z")]).expect_err("same span");
    assert!(
        matches!(
            error,
            SpliceError::Overlap {
                first: 0,
                second: 1,
                ..
            }
        ),
        "{error:?}"
    );
    let error = apply(
        SRC,
        &[splice(8, 11, b"a +", b""), splice(10, 13, b"+ b", b"")],
    )
    .expect_err("partial");
    assert!(matches!(error, SpliceError::Overlap { .. }), "{error:?}");
    // An enclosing span is caught against every later span, not just the first.
    let error = apply(
        SRC,
        &[
            splice(0, 14, b"let x = a + b;", b""),
            splice(4, 5, b"x", b""),
            splice(10, 11, b"+", b""),
        ],
    )
    .expect_err("enclosing");
    assert!(
        matches!(
            error,
            SpliceError::Overlap {
                first: 0,
                second: 1,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn the_offset_map_translates_exactly_outside_replaced_bytes() {
    // "let x = a + b;\n" -> "let total = a - b;\n"
    let (out, map) = apply(
        SRC,
        &[splice(4, 5, b"x", b"total"), splice(10, 11, b"+", b"-")],
    )
    .expect("applies");
    assert_eq!(map.splices(), 2);
    for offset in [0u32, 3, 5, 6, 9, 11, 14, 15] {
        let (mapped, exact) = map.to_output(offset);
        assert!(exact, "offset {offset}");
        if offset < 15 {
            assert_eq!(
                out[mapped as usize], SRC[offset as usize],
                "offset {offset} keeps its byte"
            );
        }
        assert_eq!(
            map.to_original(mapped),
            (offset, true),
            "round trip of {offset}"
        );
    }
    assert_eq!(
        map.to_output(4),
        (4, true),
        "the start of a replaced range is its replacement's start, exactly"
    );
    assert_eq!(
        map.to_output(5),
        (9, true),
        "the end of a replaced range is the byte after the replacement"
    );
    assert_eq!(
        map.to_original(6),
        (4, false),
        "inside a replacement: the start of the range it replaced"
    );
}

#[test]
fn an_offset_inside_a_deleted_range_maps_inexactly_to_the_replacement_start() {
    let (_, map) = apply(SRC, &[splice(8, 14, b"a + b;", b"")]).expect("deletes");
    assert_eq!(map.to_output(10), (8, false));
    assert_eq!(map.to_output(8), (8, true));
    assert_eq!(map.to_output(14), (8, true));
    // Both ends of the deleted range map onto output offset 8; the reverse
    // lookup answers with the surviving byte after the deletion.
    assert_eq!(map.to_original(8), (14, true));
}

#[test]
fn an_offset_at_an_insertion_translates_past_the_inserted_text() {
    let (out, map) = apply(SRC, &[splice(8, 8, b"", b"(")]).expect("inserts");
    assert_eq!(map.to_output(8), (9, true));
    assert_eq!(out[9], b'a');
    assert_eq!(
        map.to_original(8),
        (8, true),
        "the start of the insertion is the insertion point"
    );
    assert_eq!(map.to_original(9), (8, true), "and so is the byte after it");
    let (_, wide) = apply(SRC, &[splice(8, 8, b"", b"(((")]).expect("inserts");
    assert_eq!(
        wide.to_original(9),
        (8, false),
        "strictly inside the insertion there is nothing to return to"
    );
    assert_eq!(wide.to_original(10), (8, false));
    assert_eq!(wide.to_original(11), (8, true));
}

#[test]
fn map_span_carries_a_span_that_encloses_splices_and_refuses_one_that_straddles() {
    let (_, map) = apply(
        SRC,
        &[splice(4, 5, b"x", b"total"), splice(10, 11, b"+", b"-")],
    )
    .expect("applies");
    assert_eq!(map.map_span(span(0, 14)).expect("encloses"), span(0, 18));
    assert_eq!(
        map.map_span(span(8, 13)).expect("encloses the operator"),
        span(12, 17)
    );
    assert_eq!(map.map_span(span(12, 12)).expect("empty"), span(16, 16));
    assert!(matches!(
        map.map_span(span(4, 5))
            .expect("exactly the replaced range"),
        Span { start: 4, end: 9 }
    ));
    let inside = Span::new(0, 5).expect("ok");
    assert_eq!(
        map.map_span(inside).expect("ends at a boundary"),
        span(0, 9)
    );
    let (_, wide) = apply(SRC, &[splice(4, 9, b"x = a", b"z")]).expect("applies");
    assert!(matches!(
        wide.map_span(span(6, 12)),
        Err(SpliceError::Straddles { at_end: false, .. })
    ));
    assert!(matches!(
        wide.map_span(span(0, 6)),
        Err(SpliceError::Straddles { at_end: true, .. })
    ));
    assert!(matches!(
        wide.map_span(span(0, 99)),
        Err(SpliceError::OutOfRange { len: 15, .. })
    ));
    assert!(matches!(
        wide.map_span(Span { start: 9, end: 4 }),
        Err(SpliceError::Span { .. })
    ));
}

#[test]
fn count_lines_counts_newlines_only_so_crlf_and_lf_agree() {
    assert_eq!(count_lines(b""), 0);
    assert_eq!(count_lines(b"a\nb\nc"), 2);
    assert_eq!(count_lines(b"a\r\nb\r\n"), 2);
    assert_eq!(count_lines(b"a\rb"), 0);
}

#[test]
fn line_preserving_is_per_splice_equality_of_line_counts() {
    assert!(line_preserving(&[]));
    assert!(line_preserving(&[splice(0, 1, b"+", b"-")]));
    assert!(line_preserving(&[splice(
        0,
        7,
        b"a +\n b",
        b"if g { a - b } else { a +\n b }"
    )]));
    assert!(!line_preserving(&[splice(0, 7, b"a +\n b", b"a - b")]));
    assert!(!line_preserving(&[splice(0, 1, b"x", b"x\n")]));
    assert!(!line_preserving(&[
        splice(0, 1, b"+", b"-"),
        splice(2, 3, b"\n", b" ")
    ]));
}

fn manual(src: &[u8], splices: &[Splice]) -> Vec<u8> {
    let mut sorted = splices.to_vec();
    sorted.sort_by_key(|s| s.span);
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for s in sorted {
        out.extend_from_slice(&src[cursor..s.span.start as usize]);
        out.extend_from_slice(&s.replacement);
        cursor = s.span.end as usize;
    }
    out.extend_from_slice(&src[cursor..]);
    out
}

proptest! {
    #[test]
    fn apply_agrees_with_a_manual_construction_and_the_map_is_monotone_and_round_trips(
        src in proptest::collection::vec(any::<u8>(), 0..40),
        cuts in proptest::collection::btree_set(0u32..41, 0..8),
        replacements in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..5), 0..8),
    ) {
        // Non-overlapping spans from sorted cut points: [c0,c1), [c2,c3), ...
        let len = u32::try_from(src.len()).expect("small");
        let cuts: Vec<u32> = cuts.into_iter().filter(|c| *c <= len).collect();
        let mut splices = Vec::new();
        for (pair, replacement) in cuts.chunks(2).zip(replacements) {
            if let [start, end] = pair {
                splices.push(Splice { span: span(*start, *end), original: src[*start as usize..*end as usize].to_vec(), replacement });
            }
        }
        let (out, map) = apply(&src, &splices).expect("non-overlapping splices apply");
        prop_assert_eq!(&out, &manual(&src, &splices));
        prop_assert_eq!(map.src_len(), len);
        prop_assert_eq!(map.out_len(), u32::try_from(out.len()).expect("small"));
        let mut previous = 0u32;
        for offset in 0..=len {
            let (mapped, exact) = map.to_output(offset);
            prop_assert!(mapped >= previous, "monotone at {}", offset);
            previous = mapped;
            let inside = splices.iter().any(|s| s.span.start < offset && offset < s.span.end);
            let covered = splices.iter().any(|s| s.span.start <= offset && offset < s.span.end);
            prop_assert_eq!(exact, !inside, "exactness at {}", offset);
            if !covered {
                if offset < len {
                    prop_assert_eq!(out[mapped as usize], src[offset as usize], "byte identity at {}", offset);
                }
                prop_assert_eq!(map.to_original(mapped), (offset, true), "round trip of {}", offset);
            }
        }
    }
}
