// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which return replacements a probe can be stated for, and which expressions it is sound to evaluate a second time.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::probe::{PROBED, Question, is_effect_free, is_probed};

fn effect_free(source: &str) -> bool {
    let expr: syn::Expr = syn::parse_str(source).expect("the expression parses");
    is_effect_free(&expr)
}

#[test]
fn only_a_return_replacement_can_be_probed() {
    for rule in PROBED {
        assert!(is_probed(rule), "{rule}");
        assert!(Question::of(rule).is_some(), "{rule}");
    }
    for rule in ["le-to-lt", "add-to-sub", "negate-condition", "eq-to-neq"] {
        assert!(!is_probed(rule), "{rule}");
        assert!(Question::of(rule).is_none(), "{rule}");
    }
}

#[test]
fn each_rule_asks_its_own_question_and_says_which() {
    let questions: Vec<&str> = PROBED
        .iter()
        .filter_map(|rule| Question::of(rule))
        .map(Question::name)
        .collect();
    assert_eq!(
        questions,
        ["is-default", "is-ok-default", "is-some-default", "is-true"]
    );
}

#[test]
fn an_expression_a_probe_may_evaluate_twice_is_one_that_does_nothing_the_first_time() {
    for source in [
        "x",
        "1",
        "\"a\"",
        "self.field",
        "self.a.b.c",
        "&x",
        "x as u8",
        "!flag",
        "a == b",
        "a < b && c",
        "(a, b)",
        "[a, b]",
        "Some(x)",
        "Ok(())",
        "Err(e)",
        "None",
        "Default::default()",
        "std::default::Default::default()",
    ] {
        assert!(effect_free(source), "{source} runs nothing");
    }
}

#[test]
fn everything_that_could_be_an_event_the_second_time_is_refused() {
    for source in [
        "f(x)",
        "x.len()",
        "a + b",
        "a - b",
        "a * b",
        "a / b",
        "a % b",
        "-x",
        "items[0]",
        "*pointer",
        "x?",
        "future.await",
        "a << b",
        "{ let y = 1; y }",
        "if a { 1 } else { 2 }",
        "loop {}",
    ] {
        assert!(
            !effect_free(source),
            "{source} could panic, have effects, or fail to terminate"
        );
    }
}

#[test]
fn a_call_inside_something_otherwise_inert_is_still_a_call() {
    assert!(!effect_free("Some(f(x))"));
    assert!(!effect_free("(a, f(b))"));
    assert!(!effect_free("self.a.b == f(c)"));
    assert!(!effect_free("&f(x)"));
}

#[test]
fn the_standard_library_types_a_guard_may_compare_are_ones_the_syntax_reaches() {
    for source in [
        "self.name",
        "self.items",
        "self.maybe",
        "&self.name",
        "Some(3)",
        "Ok(0)",
        "None",
    ] {
        assert!(
            effect_free(source),
            "{source} is a value the syntax offers a probe for, which is what makes the \
             trait's widening past the primitives reachable at all"
        );
    }
}
