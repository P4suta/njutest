// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a branch proof rests on: which edits get a claim, what the compiler is asked to vouch for, and everywhere the syntax refuses to claim anything.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::branch::{Claim, DECREASING, WitnessKind, is_decreasing};
use rust_mutants::syntax::{Selection, discover_file};

/// Every candidate of `source`, as the rule that proposed it and what it claims.
fn claims(source: &str) -> Vec<(String, Option<Claim>)> {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    discover_file("src/lib.rs", source.as_bytes(), &selection)
        .expect("the source parses")
        .candidates
        .into_iter()
        .map(|found| (found.candidate.rule.name.to_owned(), found.branch))
        .collect()
}

/// What the candidate proposed by `rule` claims.
fn claim_of(source: &str, rule: &str) -> Option<Claim> {
    claims(source)
        .into_iter()
        .find(|(name, _)| name == rule)
        .unwrap_or_else(|| panic!("{rule} proposed nothing in {source}"))
        .1
}

#[test]
fn only_an_edit_that_makes_a_condition_less_often_true_claims_anything() {
    for rule in DECREASING {
        assert!(is_decreasing(rule), "{rule}");
    }
    assert!(
        !is_decreasing("lt-to-le"),
        "widening a condition proves nothing"
    );
    assert!(!is_decreasing("and-to-or"));
    assert!(!is_decreasing("eq-to-neq"));

    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    if a <= b { return 1; }\n    0\n}\n";
    assert!(claim_of(source, "le-to-lt").is_some());

    let widening = "pub fn f(a: i32, b: i32) -> i32 {\n    if a < b { return 1; }\n    0\n}\n";
    assert!(
        claim_of(widening, "lt-to-le").is_none(),
        "`<` becoming `<=` makes the condition more often true, and proves nothing"
    );
}

#[test]
fn a_claim_names_the_body_the_condition_gates_from_brace_to_brace() {
    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    if a <= b { return 1; }\n    0\n}\n";
    let claim = claim_of(source, "le-to-lt").expect("a claim");
    let body = &source[claim.body.start as usize..claim.body.end as usize];
    assert!(body.starts_with('{') && body.ends_with('}'), "{body:?}");
    assert!(body.contains("return 1;"), "{body:?}");

    let condition = &source[claim.condition.start as usize..claim.condition.end as usize];
    assert_eq!(condition, "a <= b");
}

#[test]
fn the_compiler_is_asked_to_vouch_for_what_the_syntax_cannot_decide() {
    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    if a <= b { return 1; }\n    0\n}\n";
    let claim = claim_of(source, "le-to-lt").expect("a claim");
    assert_eq!(claim.witnesses.len(), 1);
    assert_eq!(claim.witnesses[0].kind, WitnessKind::Ordered);
    let operands: Vec<&str> = claim.witnesses[0]
        .operands
        .iter()
        .map(|span| &source[span.start as usize..span.end as usize])
        .collect();
    assert_eq!(operands, ["a", "b"], "`a < b` on a user type is a call");

    let cast = "pub fn f(a: i64, b: i32) -> i32 {\n    if a as i32 <= b { return 1; }\n    0\n}\n";
    let claim = claim_of(cast, "le-to-lt").expect("a claim");
    let kinds: Vec<WitnessKind> = claim.witnesses.iter().map(|one| one.kind).collect();
    assert!(kinds.contains(&WitnessKind::Primitive), "{kinds:?}");
}

#[test]
fn an_edit_reached_through_the_connectives_claims_and_one_reached_otherwise_does_not() {
    let source = "\
pub fn f(a: i32, b: i32, c: bool) -> i32 {
    if a <= b && c { return 1; }
    0
}
";
    assert!(
        claim_of(source, "le-to-lt").is_some(),
        "an edit under && is reached from the condition"
    );

    let called = "\
pub fn f(a: i32, b: i32) -> i32 {
    if helper(a <= b) { return 1; }
    0
}

fn helper(x: bool) -> bool { x }
";
    assert!(
        claim_of(called, "le-to-lt").is_none(),
        "an edit inside a call's argument is not one the proof is about"
    );
}

#[test]
fn a_condition_that_runs_any_of_the_program_s_code_claims_nothing() {
    for source in [
        "pub fn f(v: &[i32]) -> i32 {\n    if v.len() <= 2 { return 1; }\n    0\n}\n",
        "pub fn f(a: i32, b: i32) -> i32 {\n    if a + 1 <= b { return 1; }\n    0\n}\n",
        "pub fn f(v: &[i32]) -> i32 {\n    if v[0] <= 2 { return 1; }\n    0\n}\n",
    ] {
        assert!(
            claim_of(source, "le-to-lt").is_none(),
            "a condition that can panic or have effects proves nothing: {source}"
        );
    }
}

#[test]
fn a_body_that_runs_nothing_says_nothing_by_not_running() {
    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    if a <= b { }\n    0\n}\n";
    assert!(
        claim_of(source, "le-to-lt").is_none(),
        "a target's silence about an empty body means nothing"
    );
}

#[test]
fn an_edit_outside_any_condition_claims_nothing() {
    let source = "pub fn f(a: i32, b: i32) -> bool {\n    let r = a <= b;\n    r\n}\n";
    assert!(
        claim_of(source, "le-to-lt").is_none(),
        "an edit in a value position gates no body"
    );
}

#[test]
fn a_while_condition_gates_its_body_the_way_an_if_condition_does() {
    let source = "\
pub fn f(mut a: i32, b: i32) -> i32 {
    while a <= b { a += 1; }
    a
}
";
    let claim = claim_of(source, "le-to-lt").expect("a claim");
    let body = &source[claim.body.start as usize..claim.body.end as usize];
    assert!(body.contains("a += 1;"), "{body:?}");
}

#[test]
fn an_or_that_becomes_an_and_claims_and_the_edits_beside_it_claim_on_their_own_terms() {
    let source = "\
pub fn f(a: i32, b: i32, c: i32) -> i32 {
    if a <= b || b >= c { return 1; }
    0
}
";
    assert!(claim_of(source, "or-to-and").is_some());
    assert!(claim_of(source, "le-to-lt").is_some());
    assert!(claim_of(source, "ge-to-gt").is_some());
    assert!(
        claim_of(source, "negate-condition").is_none(),
        "an edit that replaces the whole condition is not one that narrows it"
    );
}
