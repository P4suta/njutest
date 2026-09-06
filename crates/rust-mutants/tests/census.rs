// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every place the rules target has a decision: a candidate, or a reason.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, SkipReason, discover_file};

static REGISTRY: Registry = Registry::canonical();

/// What one file's walk decided, as counts.
fn decided(source: &str) -> (usize, usize, usize) {
    let found = discover_file(
        "src/lib.rs",
        source.as_bytes(),
        &Selection::tier(&REGISTRY, Tier::All),
    )
    .expect("the file walks");
    let skipped: usize = found
        .skips
        .iter()
        .map(|skip| usize::try_from(skip.count).unwrap_or(usize::MAX))
        .sum();
    (found.decisions.len(), found.candidates.len(), skipped)
}

/// Every reason the decisions of one file name.
fn reasons(source: &str) -> Vec<&'static str> {
    let found = discover_file(
        "src/lib.rs",
        source.as_bytes(),
        &Selection::tier(&REGISTRY, Tier::All),
    )
    .expect("the file walks");
    let mut named: Vec<&'static str> = found
        .decisions
        .iter()
        .filter_map(|one| one.skip.map(SkipReason::name))
        .collect();
    named.sort_unstable();
    named.dedup();
    named
}

#[test]
fn every_site_the_rules_target_has_a_decision() {
    for source in [
        "pub fn f(a: i32, b: i32) -> i32 {\n    if a > b { a } else { b }\n}\n",
        "pub const fn f(a: i32) -> i32 {\n    a + 1\n}\n",
        "pub fn f(v: &[i32]) -> &[i32] {\n    &v[1..]\n}\n",
        "pub fn f(x: Option<i32>) -> bool {\n    if let Some(n) = x && n > 0 { true } else { false }\n}\n",
    ] {
        let (decisions, candidates, skipped) = decided(source);
        assert_eq!(
            decisions,
            candidates.saturating_add(skipped),
            "every decision is a candidate or a skip, and a place with neither is one the \
             walker passed over without saying so: {source:?}"
        );
    }
}

#[test]
fn a_const_fn_body_is_its_own_reason_and_a_const_block_stays_const_context() {
    assert_eq!(
        reasons("pub const fn f(a: i32) -> i32 {\n    a + 1\n}\n"),
        ["const-fn-body"],
        "the compiler may evaluate any call of a const fn, and a runtime guard cannot live \
         where it does"
    );
    assert_eq!(
        reasons("pub static N: i32 = 1 + 2;\n"),
        ["const-context"],
        "an initializer the compiler evaluates is not the body of a function"
    );
}

#[test]
fn an_open_range_states_open_range() {
    assert_eq!(
        reasons("pub fn f(v: &[i32]) -> usize {\n    v[1..].len()\n}\n"),
        ["open-range"],
        "a range with no end has no other form to become, and passing over it silently is a \
         place a reader cannot ask about"
    );
}

#[test]
fn an_if_let_states_let_condition_for_every_rule_that_declined() {
    let said = reasons(
        "pub fn f(x: Option<i32>) -> bool {\n    if let Some(n) = x && n > 0 { true } else { false }\n}\n",
    );
    assert!(
        said.contains(&"let-condition"),
        "what a guard would have to rearrange is what the binding is in scope for: {said:?}"
    );
}

#[test]
fn a_return_type_the_syntax_cannot_default_is_stated_rather_than_guessed() {
    for source in [
        "pub fn f() -> impl Iterator<Item = i32> {\n    std::iter::empty()\n}\n",
        "pub fn f(v: &mut i32) -> &mut i32 {\n    v\n}\n",
        "pub fn f(p: *const i32) -> *const i32 {\n    p\n}\n",
        "pub fn f() -> fn(i32) -> i32 {\n    |x| x\n}\n",
        "pub fn f<T>(t: T) -> T {\n    t\n}\n",
        "macro_rules! ty { () => { i32 } }\npub fn f() -> ty!() {\n    1\n}\n",
        "pub fn f<F: std::future::Future>(_: F) -> <F as std::future::Future>::Output {\n    todo!()\n}\n",
    ] {
        assert!(
            reasons(source).contains(&"unstated-return-type"),
            "a return replacement here is a candidate the compiler refuses, and predicting the \
             refusal is what keeps a reader from reading one as a fact about the program: \
             {source:?}"
        );
    }
}

#[test]
fn a_bounded_generic_and_a_plain_path_still_get_return_default() {
    for source in [
        "pub fn f<T: Default>(t: T) -> T {\n    t\n}\n",
        "pub fn f<T>(t: T) -> T where T: Default {\n    t\n}\n",
        "pub struct Thing;\npub fn f() -> Thing {\n    Thing\n}\n",
    ] {
        assert!(
            !reasons(source).contains(&"unstated-return-type"),
            "the syntax says this one has a default: {source:?}"
        );
    }
}

#[test]
fn each_branch_of_a_returned_if_or_match_is_a_return_site_and_the_whole_keeps_its_id() {
    let source = "pub fn f(n: i32) -> i32 {\n    if n > 0 { n } else { -n }\n}\n";
    let found = discover_file(
        "src/lib.rs",
        source.as_bytes(),
        &Selection::tier(&REGISTRY, Tier::All),
    )
    .expect("the file walks");
    let replaced: Vec<String> = found
        .candidates
        .iter()
        .filter(|one| one.candidate.rule.name == "return-default")
        .map(|one| String::from_utf8_lossy(&one.candidate.original).into_owned())
        .collect();
    assert_eq!(
        replaced,
        ["if n > 0 { n } else { -n }", "n", "-n"],
        "a replacement of the whole expression is one mutation and a replacement of one branch \
         is another, and a suite that notices the first may notice nothing about the second"
    );

    let whole = found
        .candidates
        .iter()
        .find(|one| one.candidate.original.starts_with(b"if "))
        .expect("the whole expression");
    assert_eq!(
        whole.candidate.id().expect("an identity"),
        "eefdf186018442c99e227fe3f7b7e825030a4913ec558f25746d9a1652b66caa",
        "an identity is minted from the bytes an edit replaces, and those bytes have not moved"
    );
}
