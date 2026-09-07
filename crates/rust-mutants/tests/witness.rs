// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The witness tree: what the compiler is asked, and that asking it costs no line.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::instrument::witness::{Claimed, MARKER, Placed, witness_file};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, discover_file};

/// Every claim `source` yields, ready to be witnessed.
fn claimed(source: &str) -> Vec<Claimed> {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    discover_file("src/lib.rs", source.as_bytes(), &selection)
        .expect("the source parses")
        .candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, found)| {
            found.branch.map(|claim| Claimed {
                index: u32::try_from(index).unwrap_or(0),
                claim,
                super_depth: found.hint.super_depth,
            })
        })
        .collect()
}

#[test]
fn a_file_with_no_claim_comes_back_byte_for_byte() {
    let source = "pub fn f(a: i32) -> i32 { a + 1 }\n";
    let written = witness_file("src/lib.rs", source.as_bytes(), &[]).expect("witnessed");
    assert_eq!(written.text, source);
    assert!(!written.witnessed);
    assert!(written.sites.is_empty());
}

#[test]
fn the_compiler_is_asked_about_the_operands_and_the_answer_costs_no_line() {
    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    if a <= b { return 1; }\n    0\n}\n";
    let claims = claimed(source);
    assert!(!claims.is_empty(), "the source yields a claim");
    let written = witness_file("src/lib.rs", source.as_bytes(), &claims).expect("witnessed");
    assert!(written.witnessed);
    assert!(
        written.text.contains("__rmw::w_ord(&(a), &(b));"),
        "{}",
        written.text
    );
    assert!(written.text.contains(MARKER), "{}", written.text);

    let before = source.lines().count();
    let after = written
        .text
        .lines()
        .take_while(|line| !line.contains(MARKER))
        .count();
    assert_eq!(
        after, before,
        "the witnesses go in front of the condition and the module after the last line"
    );
}

#[test]
fn one_condition_carries_every_claim_that_rests_on_it() {
    let source = "\
pub fn f(a: i32, b: i32, c: i32) -> i32 {
    if a <= b || b >= c { return 1; }
    0
}
";
    let claims = claimed(source);
    assert!(claims.len() >= 3, "{claims:?}");
    let written = witness_file("src/lib.rs", source.as_bytes(), &claims).expect("witnessed");
    let witnessed: Vec<&rust_mutants::instrument::witness::Site> = written
        .sites
        .iter()
        .filter(|site| site.placed == Placed::Witnesses)
        .collect();
    assert_eq!(
        witnessed.len(),
        1,
        "one condition is rewritten once, however many claims rest on it"
    );
    assert_eq!(
        witnessed[0].claims.len(),
        claims.len(),
        "and a diagnostic inside it is about all of them"
    );
    let inside = &written.text[witnessed[0].span.start as usize..witnessed[0].span.end as usize];
    assert!(inside.starts_with("({ "), "{inside}");
    assert!(inside.ends_with(" })"), "{inside}");
    assert!(inside.contains("a <= b || b >= c"), "{inside}");

    let marked: Vec<&rust_mutants::instrument::witness::Site> = written
        .sites
        .iter()
        .filter(|site| site.placed == Placed::Marker)
        .collect();
    assert_eq!(
        marked.len(),
        1,
        "and the body it gates carries one marker, however many claims rest on it"
    );
    assert_eq!(
        marked[0].claims.len(),
        claims.len(),
        "a diagnostic in the marker is about the marker of all of them"
    );
    let call = &written.text[marked[0].span.start as usize..marked[0].span.end as usize];
    assert!(call.contains("::body("), "{call}");
    assert!(
        !written.text.contains("\n    if a <= b || b >= c { __rmw"),
        "the marker goes after the brace, and no line moves: {}",
        written.text
    );
}

#[test]
fn a_cast_asks_whether_its_operand_is_a_primitive() {
    let source =
        "pub fn f(a: i64, b: i32) -> i32 {\n    if a as i32 <= b { return 1; }\n    0\n}\n";
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &claimed(source)).expect("witnessed");
    assert!(
        written.text.contains("__rmw::w_prim(&(a));"),
        "{}",
        written.text
    );
}

#[test]
fn a_condition_inside_a_module_calls_the_module_the_witnesses_live_in() {
    let source = "\
pub mod inner {
    pub fn f(a: i32, b: i32) -> i32 {
        if a <= b { return 1; }
        0
    }
}
";
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &claimed(source)).expect("witnessed");
    assert!(
        written.text.contains("super::__rmw::w_ord"),
        "the module is at the file root, and the condition is one module in: {}",
        written.text
    );
}

#[test]
fn the_witnessed_file_still_holds_what_it_held() {
    let source = "\
//! A crate.

/// Doubles.
pub fn f(a: i32, b: i32) -> i32 {
    if a <= b { return 1; }
    0
}
";
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &claimed(source)).expect("witnessed");
    assert!(written.text.starts_with("//! A crate."), "{}", written.text);
    assert!(written.text.contains("/// Doubles."), "{}", written.text);
    assert!(written.text.contains("return 1;"), "{}", written.text);
}
