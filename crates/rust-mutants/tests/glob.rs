// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The glob language, case by case, and against a naive reference matcher.

#![expect(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use proptest::prelude::*;
use rust_mutants::glob::{GlobError, Pattern};

fn matches(pattern: &str, path: &str) -> bool {
    Pattern::compile(pattern)
        .unwrap_or_else(|e| panic!("{e}"))
        .matches(path)
}

#[test]
fn literal_elements_match_exactly_and_case_sensitively() {
    assert!(matches("src/lib.rs", "src/lib.rs"));
    assert!(!matches("src/lib.rs", "src/Lib.rs"));
    assert!(!matches("src/lib.rs", "src/lib.rs/x"));
    assert!(!matches("lib.rs", "src/lib.rs"));
}

#[test]
fn a_star_matches_a_possibly_empty_run_of_non_separator_bytes() {
    assert!(matches("*.rs", "lib.rs"));
    assert!(matches("*.rs", ".rs"));
    assert!(
        !matches("*.rs", "src/lib.rs"),
        "a star never crosses a separator"
    );
    assert!(!matches("*", "a/b"));
    assert!(matches("a/*", "a/b"));
    assert!(!matches("a/*", "a/b/c"));
    assert!(matches("*", ".hidden"), "a leading dot is not special");
}

#[test]
fn a_question_mark_matches_exactly_one_byte() {
    assert!(matches("lib.r?", "lib.rs"));
    assert!(!matches("lib.r?", "lib.r"));
    assert!(!matches("lib.r?", "lib.rss"));
    assert!(!matches("?", "日"), "one byte, not one character");
    assert!(matches("???", "日"));
}

#[test]
fn double_star_matches_zero_or_more_whole_elements() {
    assert!(matches("**/*.rs", "a.rs"));
    assert!(matches("**/*.rs", "x/y/a.rs"));
    assert!(matches("vendor/**", "vendor"));
    assert!(matches("vendor/**", "vendor/x/y"));
    assert!(!matches("vendor/**", "vendorx"));
    assert!(matches("**", ".git/config"));
    assert!(matches("a/**/b", "a/b"));
    assert!(matches("a/**/b", "a/x/y/b"));
    assert!(!matches("a/**/b", "a/x/y/c"));
}

#[test]
fn double_star_is_special_only_as_a_complete_element() {
    assert!(matches("a**b", "axxb"));
    assert!(!matches("a**b", "a/b"));
    assert!(matches("**.rs", "lib.rs"));
    assert!(!matches("**.rs", "src/lib.rs"));
}

#[test]
fn a_pattern_that_took_exponential_time_by_backtracking_is_still_fast() {
    let pattern = Pattern::compile("**/**/**/**/**/**/**/**/*a").expect("compiles");
    let path = format!("{}b", "b/".repeat(40));
    assert!(!pattern.matches(&path));
    let stars = Pattern::compile("a*a*a*a*a*a*a*a*a*a*b").expect("compiles");
    assert!(!stars.matches(&"a".repeat(200)));
    assert!(stars.matches(&format!("{}b", "a".repeat(30))));
}

#[test]
fn malformed_paths_match_nothing() {
    let pattern = Pattern::compile("**").expect("compiles");
    assert!(!pattern.matches(""));
    assert!(!pattern.matches("a//b"));
    assert!(!pattern.matches("/a"));
    assert!(!pattern.matches("a/"));
}

#[test]
fn rejected_patterns_name_the_offending_column() {
    let cases = [
        ("", 1, "empty pattern"),
        ("/src/lib.rs", 1, "leading '/'"),
        ("src/", 4, "trailing '/'"),
        ("a//b", 3, "empty path element"),
    ];
    for (pattern, column, message) in cases {
        let error = Pattern::compile(pattern).expect_err(pattern);
        assert_eq!(error.pattern, pattern);
        assert_eq!(error.column, column, "{pattern:?}");
        assert!(
            error.message.contains(message),
            "{pattern:?}: {}",
            error.message
        );
        assert!(error.to_string().contains("(column "), "{error}");
    }
    let typed: GlobError = Pattern::compile("").expect_err("typed error");
    assert_eq!(typed.column, 1);
}

#[test]
fn a_pattern_renders_as_it_was_compiled() {
    assert_eq!(
        Pattern::compile("crates/**/*.rs")
            .expect("compiles")
            .to_string(),
        "crates/**/*.rs"
    );
}

/// A naive, obviously correct reference: backtracking over elements and bytes.
fn reference(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.first(), path.first()) {
        (None, None) => true,
        (Some(&"**"), _) => {
            reference(&pattern[1..], path) || (!path.is_empty() && reference(pattern, &path[1..]))
        }
        (Some(element), Some(segment)) => {
            reference_element(element.as_bytes(), segment.as_bytes())
                && reference(&pattern[1..], &path[1..])
        }
        (None, Some(_)) | (Some(_), None) => false,
    }
}

fn reference_element(pattern: &[u8], segment: &[u8]) -> bool {
    match (pattern.first(), segment.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            reference_element(&pattern[1..], segment)
                || (!segment.is_empty() && reference_element(pattern, &segment[1..]))
        }
        (Some(b'?'), Some(_)) => reference_element(&pattern[1..], &segment[1..]),
        (Some(p), Some(s)) => p == s && reference_element(&pattern[1..], &segment[1..]),
        (None, Some(_)) | (Some(_), None) => false,
    }
}

proptest! {
    #[test]
    fn the_matcher_agrees_with_the_naive_reference(
        pattern_elements in proptest::collection::vec("(\\*\\*|[ab*?]{1,3})", 1..5),
        path_elements in proptest::collection::vec("[ab]{1,3}", 1..6),
    ) {
        let pattern_text = pattern_elements.join("/");
        let path_text = path_elements.join("/");
        let pattern = Pattern::compile(&pattern_text).expect("elements are non-empty");
        let want = reference(&pattern_elements.iter().map(String::as_str).collect::<Vec<_>>(), &path_elements.iter().map(String::as_str).collect::<Vec<_>>());
        prop_assert_eq!(pattern.matches(&path_text), want, "pattern {} path {}", pattern_text, path_text);
    }
}
