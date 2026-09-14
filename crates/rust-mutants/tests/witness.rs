// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The witness tree: what the compiler is asked, and that asking it costs no line.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::instrument::witness::{Asking, Claimed, MARKER, Placed, Probing, witness_file};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, discover_file};

/// Everything `source` puts to the compiler, numbered as a catalog would number it.
fn asked(source: &str) -> Vec<Claimed> {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    discover_file("src/lib.rs", source.as_bytes(), &selection)
        .expect("the source parses")
        .candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, found)| {
            let (condition, body, witnesses) = match (&found.branch, &found.comparable) {
                (Some(claim), _) => (claim.condition, Some(claim.body), claim.witnesses.clone()),
                (None, Some(one)) => (one.condition, None, one.witnesses.clone()),
                (None, None) => return None,
            };
            Some(Claimed {
                index: u32::try_from(index).unwrap_or(0),
                condition,
                body,
                witnesses,
                super_depth: found.hint.super_depth,
            })
        })
        .collect()
}

/// Every returned value of `source` a probe would rest on, numbered as a catalog would number it.
fn probed(source: &str) -> Vec<Probing> {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    discover_file("src/lib.rs", source.as_bytes(), &selection)
        .expect("the source parses")
        .candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, found)| {
            Some(Probing {
                index: u32::try_from(index).unwrap_or(0),
                value: found.hint.site,
                question: found.probe?,
                super_depth: found.hint.super_depth,
            })
        })
        .collect()
}

/// What one file is asked when only its conditions are.
const fn conditions(claims: &[Claimed]) -> Asking<'_> {
    Asking {
        conditions: claims,
        probes: &[],
    }
}

/// Every one of them that names a body, which is what a branch proof rests on.
fn claimed(source: &str) -> Vec<Claimed> {
    asked(source)
        .into_iter()
        .filter(|one| one.claim().is_some())
        .collect()
}

#[test]
fn a_file_with_no_claim_comes_back_byte_for_byte() {
    let source = "pub fn f(a: i32) -> i32 { a + 1 }\n";
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &conditions(&[])).expect("witnessed");
    assert_eq!(written.text, source);
    assert!(!written.witnessed);
    assert!(written.sites.is_empty());
}

#[test]
fn the_compiler_is_asked_about_the_operands_and_the_answer_costs_no_line() {
    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    if a <= b { return 1; }\n    0\n}\n";
    let claims = claimed(source);
    assert!(!claims.is_empty(), "the source yields a claim");
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &conditions(&claims)).expect("witnessed");
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
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &conditions(&claims)).expect("witnessed");
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
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &conditions(&claimed(source)),
    )
    .expect("witnessed");
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
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &conditions(&claimed(source)),
    )
    .expect("witnessed");
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
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &conditions(&claimed(source)),
    )
    .expect("witnessed");
    assert!(written.text.starts_with("//! A crate."), "{}", written.text);
    assert!(written.text.contains("/// Doubles."), "{}", written.text);
    assert!(written.text.contains("return 1;"), "{}", written.text);
}

#[test]
fn an_edit_no_branch_proof_is_about_still_has_its_condition_put_to_the_compiler() {
    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    if a < b { return 1; }\n    0\n}\n";
    let asked = asked(source);
    assert!(
        asked.iter().any(|one| one.claim().is_none()),
        "widening a comparison proves nothing about the body, and the guard still compares"
    );
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &conditions(&asked)).expect("witnessed");
    assert!(
        written.text.contains("__rmw::w_ord(&(a), &(b));"),
        "the operands are what makes evaluating either branch run none of the program: {}",
        written.text
    );
}

#[test]
fn a_proof_and_a_comparison_about_one_condition_are_one_rewrite() {
    let source = "pub fn f(a: i32, b: i32, c: i32, d: i32) -> i32 {\n    \
                  if a <= b && c < d { return 1; }\n    0\n}\n";
    let asked = asked(source);
    let proofs = asked.iter().filter(|one| one.claim().is_some()).count();
    assert!(
        proofs > 0,
        "`<=` narrows, so it proves something about the body"
    );
    assert!(
        asked.len() > proofs,
        "`&&` and `<` prove nothing about it and still compare: {asked:?}"
    );
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &conditions(&asked)).expect("witnessed");
    let witnessed = written
        .sites
        .iter()
        .filter(|site| site.placed == Placed::Witnesses)
        .count();
    assert_eq!(
        witnessed, 1,
        "one condition is witnessed once however many questions rest on it: {:?}",
        written.sites
    );
    let carried = written
        .sites
        .iter()
        .find(|site| site.placed == Placed::Witnesses)
        .map(|site| site.claims.len())
        .expect("the condition was witnessed");
    assert_eq!(carried, asked.len(), "and carries every one of them");
    assert!(
        written.text.contains("__rmw::w_ord(&(a), &(b));"),
        "{}",
        written.text
    );
    assert!(
        written.text.contains("__rmw::w_ord(&(c), &(d));"),
        "{}",
        written.text
    );
}

#[test]
fn a_returned_value_is_put_to_the_compiler_through_a_binding_of_its_own() {
    let source = "pub fn f(a: i32) -> i32 {\n    return a;\n}\n";
    let probes = probed(source);
    assert_eq!(
        probes.len(),
        1,
        "one return replacement of this is probeable: {probes:?}"
    );
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &Asking {
            conditions: &[],
            probes: &probes,
        },
    )
    .expect("witnessed");
    assert!(
        written.text.contains("__rmw_value = a;"),
        "the value is evaluated once and bound, which is the shape the guard will hold: {}",
        written.text
    );
    assert!(
        written.text.contains("__rmw::w_default(&__rmw_value)"),
        "and the question is which type it is: {}",
        written.text
    );
    assert_eq!(
        written
            .sites
            .iter()
            .filter(|site| site.placed == Placed::Probe)
            .count(),
        1,
        "a diagnostic here costs the probe and not the mutant: {:?}",
        written.sites
    );
}

#[test]
fn a_probe_of_a_type_the_trait_does_not_name_is_refused_by_the_compiler_rather_than_by_the_syntax()
{
    let source = "pub struct Own;\npub fn f(a: Own) -> Own {\n    return a;\n}\n";
    let probes = probed(source);
    assert_eq!(
        probes.len(),
        1,
        "the syntax offers it: a path is effect free whatever it names"
    );
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &Asking {
            conditions: &[],
            probes: &probes,
        },
    )
    .expect("witnessed");
    assert!(
        written.text.contains("__rmw::w_default(&__rmw_value)"),
        "so the question goes to the compiler, which is the one that can answer it: {}",
        written.text
    );
}

#[test]
fn probing_a_value_costs_the_file_no_line() {
    let source = "\
pub fn f(a: i32) -> i32 {
    return a;
}

pub fn g(b: bool) -> bool {
    return b;
}
";
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &Asking {
            conditions: &[],
            probes: &probed(source),
        },
    )
    .expect("witnessed");
    let body = written
        .text
        .split("#[doc(hidden)]")
        .next()
        .unwrap_or_default();
    assert_eq!(
        body.lines().count(),
        source.lines().count(),
        "every position the catalog reports still points where it did: {body}"
    );
    assert!(
        written.text.contains("__rmw::w_true(&__rmw_value)"),
        "a bool return asks whether it is already true: {}",
        written.text
    );
}

#[test]
fn a_value_written_over_several_lines_keeps_every_one_of_them() {
    let source = "\
pub struct Pair {
    pub a: i32,
    pub b: i32,
}

pub fn f() -> Pair {
    Pair {
        a: 1,
        b: 2,
    }
}
";
    let probes = probed(source);
    assert!(
        !probes.is_empty(),
        "a struct literal is effect free, so the syntax offers the probe"
    );
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &Asking {
            conditions: &[],
            probes: &probes,
        },
    )
    .expect("a witness that moved a line would be refused outright, which vouches for nothing");
    let body = written
        .text
        .split("#[doc(hidden)]")
        .next()
        .unwrap_or_default();
    assert_eq!(
        body.lines().count(),
        source.lines().count(),
        "the value is kept verbatim and only what surrounds it is written: {body}"
    );
}

#[test]
fn a_witnessed_file_is_still_a_program() {
    let source = "pub fn f(a: i64, b: i32) -> i32 {\n    if a as\ni32 <= b\n    {\n        return 1;\n    }\n    0\n}\n";
    let written = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &conditions(&claimed(source)),
    )
    .expect("witnessed");
    assert!(
        written.text.contains("w_prim"),
        "the cast's operand is what the compiler is asked about: {}",
        written.text
    );
    syn::parse_file(&written.text).unwrap_or_else(|error| {
        panic!(
            "an operand spelled over two lines is written on one, and what separates two tokens \
             has to survive that: {error}\n{}",
            written.text
        )
    });
}

#[test]
fn the_marker_of_a_body_names_the_lowest_mutant_that_rests_on_it() {
    let source = "\
pub fn f(a: i32, b: i32, c: i32, d: i32) -> i32 {
    if a <= b && c <= d {
        return 1;
    }
    0
}
";
    let claims = claimed(source);
    assert!(
        claims.len() > 1,
        "two narrowing comparisons rest on this one body: {claims:?}"
    );
    let lowest = claims
        .iter()
        .map(|one| one.index)
        .min()
        .expect("a claim to be lowest");
    let written =
        witness_file("src/lib.rs", source.as_bytes(), &conditions(&claims)).expect("witnessed");
    assert!(
        written.text.contains(&format!("{{__rmw::body({lowest});")),
        "one body carries one marker however many claims rest on it, it names the lowest of \
         them so the log that records it needs no second numbering, and it sits on the byte \
         after the opening brace so that entering the body is what records it: {}",
        written.text
    );
}

#[test]
fn a_value_that_is_not_in_the_source_is_refused_rather_than_written_around() {
    let source = "pub fn f(a: i32) -> i32 {\n    return a;\n}\n";
    let mut probes = probed(source);
    let past = u32::try_from(source.len()).unwrap_or(u32::MAX);
    probes[0].value = rust_mutants::span::Span::new(past, past.saturating_add(4)).expect("a span");

    let error = witness_file(
        "src/lib.rs",
        source.as_bytes(),
        &Asking {
            conditions: &[],
            probes: &probes,
        },
    )
    .expect_err("a value outside the source is not one the tree can be written around");
    assert_eq!(
        error.kind(),
        rust_mutants::instrument::InstrumentErrorKind::SourceMismatch,
        "and it says which of the refusals it is, because the caller decides what a file it \
         could not write costs: {error}"
    );
}

#[test]
fn the_marker_names_the_lowest_claim_however_they_arrive() {
    let source = "\
pub fn f(a: i32, b: i32, c: i32, d: i32) -> i32 {
    if a <= b && c <= d {
        return 1;
    }
    0
}
";
    let mut claims = claimed(source);
    claims.sort_by_key(|one| std::cmp::Reverse(one.index));
    let lowest = claims
        .iter()
        .map(|one| one.index)
        .min()
        .expect("a claim to be lowest");
    assert_ne!(
        claims.first().map(|one| one.index),
        Some(lowest),
        "handed to it highest first, so an order the writer did not impose is one it took"
    );

    let written =
        witness_file("src/lib.rs", source.as_bytes(), &conditions(&claims)).expect("witnessed");
    assert!(
        written.text.contains(&format!("__rmw::body({lowest});")),
        "the marker names the lowest claim whatever order they arrived in, because the log that \
         records it is numbered by the catalog and not by the caller: {}",
        written.text
    );
}

#[test]
fn two_claims_over_one_span_are_one_rewrite_rather_than_two() {
    let source = "pub fn f(a: i32, b: i32) -> i32 { if a < b { 1 } else { 0 } }\n";
    let mut claims = asked(source);
    claims.extend(asked(source));
    assert!(
        claims.len() > 1,
        "the same condition twice is what two claims over one span looks like"
    );
    for (at, one) in claims.iter_mut().enumerate() {
        one.index = u32::try_from(at).unwrap_or(0);
    }
    let written = witness_file("src/lib.rs", source.as_bytes(), &conditions(&claims))
        .expect("one rewrite of the span both claims name");
    let over: Vec<(u32, u32)> = written
        .sites
        .iter()
        .map(|site| (site.span.start, site.span.end))
        .collect();
    assert_eq!(
        over.len(),
        1,
        "the claims name one span, so the file is rewritten there once: {over:?}"
    );
    assert_eq!(
        written.sites[0].claims,
        vec![0, 1],
        "and the one rewrite carries both of them, or a claim would be vouched for by a witness \
         nobody wrote"
    );
}
