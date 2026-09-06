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
        reasons("pub fn f(v: &[i32]) -> &[i32] {\n    &v[1..]\n}\n"),
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
