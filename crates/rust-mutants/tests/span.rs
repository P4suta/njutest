// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Half-open byte spans: the arithmetic every splice rests on.

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use proptest::prelude::*;
use rust_mutants::span::{Span, SpanError};

fn span(start: u32, end: u32) -> Span {
    Span::new(start, end).expect("well formed")
}

#[test]
fn a_reversed_span_is_refused_and_an_empty_one_is_an_insertion_point() {
    assert_eq!(
        Span::new(9, 4),
        Err(SpanError::Reversed { start: 9, end: 4 })
    );
    let empty = span(3, 3);
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert_eq!(Span::default(), span(0, 0));
}

#[test]
fn len_counts_the_bytes_covered() {
    assert_eq!(span(10, 15).len(), 5);
    assert!(!span(10, 15).is_empty());
}

#[test]
fn contains_includes_itself_and_empty_spans_on_its_boundaries() {
    let outer = span(10, 20);
    assert!(outer.contains(outer));
    assert!(outer.contains(span(12, 18)));
    assert!(outer.contains(span(10, 10)));
    assert!(outer.contains(span(20, 20)));
    assert!(!outer.contains(span(9, 12)));
    assert!(!outer.contains(span(18, 21)));
    assert!(outer.strictly_contains(span(12, 18)));
    assert!(!outer.strictly_contains(outer));
}

#[test]
fn overlaps_needs_a_shared_byte_so_empty_and_touching_spans_never_overlap() {
    assert!(span(0, 10).overlaps(span(5, 15)));
    assert!(span(5, 15).overlaps(span(0, 10)));
    assert!(!span(0, 5).overlaps(span(5, 9)));
    assert!(!span(3, 3).overlaps(span(0, 10)));
    assert!(!span(3, 3).overlaps(span(3, 3)));
}

#[test]
fn spans_order_by_start_then_end() {
    let mut spans = vec![span(5, 9), span(0, 10), span(0, 5), span(5, 6)];
    spans.sort();
    assert_eq!(spans, [span(0, 5), span(0, 10), span(5, 6), span(5, 9)]);
}

#[test]
fn slice_returns_the_covered_bytes_and_refuses_to_reach_past_the_source() {
    let source = b"hello, world";
    assert_eq!(span(7, 12).slice(source).expect("in range"), b"world");
    assert_eq!(span(12, 12).slice(source).expect("empty at the end"), b"");
    assert_eq!(
        span(7, 13).slice(source),
        Err(SpanError::OutOfRange {
            start: 7,
            end: 13,
            len: 12
        })
    );
    assert_eq!(
        Span { start: 9, end: 4 }.slice(source),
        Err(SpanError::Reversed { start: 9, end: 4 })
    );
}

#[test]
fn a_span_renders_in_half_open_notation() {
    assert_eq!(span(12, 20).to_string(), "[12,20)");
    assert_eq!(
        SpanError::Reversed { start: 9, end: 4 }.to_string(),
        "span end byte 4 precedes start byte 9"
    );
}

proptest! {
    #[test]
    fn contains_and_overlaps_agree_with_the_arithmetic(a in 0u32..100, b in 0u32..100, c in 0u32..100, d in 0u32..100) {
        let (s1, e1) = if a <= b { (a, b) } else { (b, a) };
        let (s2, e2) = if c <= d { (c, d) } else { (d, c) };
        let x = span(s1, e1);
        let y = span(s2, e2);
        prop_assert_eq!(x.contains(y), s1 <= s2 && e2 <= e1);
        prop_assert_eq!(x.overlaps(y), s1 < e1 && s2 < e2 && s1 < e2 && s2 < e1);
        prop_assert_eq!(u64::from(x.len()), u64::from(e1) - u64::from(s1));
        prop_assert_eq!(x.strictly_contains(y), x.contains(y) && x != y);
        prop_assert_eq!(x.overlaps(y), y.overlaps(x));
    }
}
