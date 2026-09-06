// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a marker hides, wherever an author puts one.

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{FileDiscovery, Selection, SkipReason, discover_file};

static REGISTRY: Registry = Registry::canonical();

/// A file of `functions` functions, each one line of body, with a marker on `at`.
fn generated(functions: usize, at: usize, inside_a_literal: bool) -> String {
    use std::fmt::Write as _;
    let marker = if inside_a_literal {
        "let _said = \"rust-mutants: skip nothing\";"
    } else {
        "// rust-mutants: skip a reason"
    };
    let mut text = String::from("//! A generated module.\n");
    for index in 0..functions {
        let _written = writeln!(
            text,
            "pub fn f{index}(a: i32, b: i32) -> i32 {{\n    {}\n    a + b\n}}",
            if index == at { marker } else { "let _n = 1;" }
        );
    }
    text
}

fn discovered(source: &str) -> Option<FileDiscovery> {
    let selection = Selection::tier(&REGISTRY, Tier::All);
    discover_file("src/lib.rs", source.as_bytes(), &selection).ok()
}

proptest::proptest! {
    /// Wherever a marker sits, it hides the places that start where it says and nothing else, and a marker spelled inside a string literal hides nothing at all.
    #[test]
    fn a_marker_hides_exactly_what_starts_where_it_says(
        functions in 1usize..6,
        at in 0usize..6,
        inside_a_literal in proptest::bool::ANY
    ) {
        let at = at.min(functions.saturating_sub(1));
        let source = generated(functions, at, inside_a_literal);
        let plain = generated(functions, functions, false);
        let marked = discovered(&source).expect("the generated file walks");
        let unmarked = discovered(&plain).expect("the generated file walks");
        if inside_a_literal {
            proptest::prop_assert!(marked.annotations.is_empty());
            proptest::prop_assert_eq!(
                marked
                    .skips
                    .iter()
                    .filter(|skip| skip.reason == SkipReason::Annotated)
                    .count(),
                0
            );
        } else {
            proptest::prop_assert_eq!(marked.annotations.len(), 1);
            proptest::prop_assert!(
                marked.annotations.first().is_some_and(|claim| claim.matched)
            );
            proptest::prop_assert!(
                marked.candidates.len() < unmarked.candidates.len(),
                "a marker that hid nothing would leave the count where it was"
            );
        }
    }
}
