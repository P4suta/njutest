// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which functions a model checker can be asked about, read from a signature and nothing else.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking, and asserts with panics"
)]

use njutest_cli::askable::{Reach, askable};
use njutest_cli::modelled::Unaskable;

/// The signature of `written`, parsed.
fn signature(written: &str) -> syn::Signature {
    let parsed: syn::ItemFn =
        syn::parse_str(&format!("{written} {{ unimplemented!() }}")).expect("a function");
    parsed.sig
}

/// What `askable` says about `written`, as the argument it named or nothing.
fn refused(written: &str) -> Option<String> {
    match askable(&signature(written)) {
        Ok(()) => None,
        Err(Unaskable::NotArbitrary { argument }) => Some(argument),
        Err(Unaskable::TooDeep { bound }) => panic!("a signature has no bound: {bound}"),
    }
}

#[test]
fn a_function_of_primitives_is_one_a_checker_can_be_asked_about() {
    for written in [
        "fn f(a: i32) -> i32",
        "fn f(a: u8, b: bool, c: char) -> u8",
        "fn f(a: f64) -> f64",
        "fn f() -> i32",
        "fn f(a: (i32, u8)) -> i32",
        "fn f(a: [u8; 4]) -> u8",
        "fn f(a: Option<i32>) -> i32",
        "fn f(a: core::primitive::i32) -> i32",
    ] {
        assert_eq!(
            refused(written),
            None,
            "a symbolic value can be made for every argument of {written}"
        );
    }
}

#[test]
fn a_function_whose_argument_has_no_symbolic_value_names_that_argument() {
    assert_eq!(
        refused("fn place(order: Order) -> bool").as_deref(),
        Some("order: Order"),
        "the argument and the head of its type, because that is what a reader goes \
         and looks at; the whole type would put the answer further from the question"
    );
    assert_eq!(
        refused("fn take(name: String) -> bool").as_deref(),
        Some("name: String")
    );
    assert_eq!(
        refused("fn scan(items: &[u8]) -> bool").as_deref(),
        Some("items: &[u8]"),
        "a reference is not minted, and saying so conservatively costs a test run \
         where claiming it would cost a harness that does not compile"
    );
    assert_eq!(
        refused("fn nested(a: Option<String>) -> bool").as_deref(),
        Some("a: Option"),
        "a wrapper is minted only when what is inside it is"
    );
}

#[test]
fn the_first_argument_that_cannot_be_asked_is_the_one_named() {
    assert_eq!(
        refused("fn f(a: i32, b: String, c: Vec<u8>) -> bool").as_deref(),
        Some("b: String"),
        "one argument to go and look at rather than a list, because fixing the \
         first is what a reader would do next and the rest may follow from it"
    );
}

#[test]
fn a_method_taking_self_is_not_one_a_checker_can_be_handed() {
    assert_eq!(
        refused("fn total(&self, a: i32) -> i32").as_deref(),
        Some("self"),
        "there is no symbolic receiver, and a harness cannot invent one"
    );
}

#[test]
fn what_a_run_can_say_before_it_starts_a_checker_is_a_count() {
    let mut reach = Reach::default();
    for written in [
        "fn a(a: i32) -> i32",
        "fn b(a: u8) -> u8",
        "fn c(a: String) -> bool",
    ] {
        reach.counted(askable(&signature(written)).is_ok());
    }
    assert_eq!(reach.askable, 2);
    assert_eq!(reach.unaskable, 1);
    assert_eq!(
        reach.considered(),
        3,
        "the number that says whether this observer is worth its contract on a \
         project, arrived at without starting anything"
    );
}
