// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The infection log, read fail-closed: everything it says or nothing at all.

#![expect(
    clippy::format_push_string,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::probe::log::{LogError, SCHEMA, header_line, read};

fn catalog() -> String {
    "a".repeat(64)
}

fn log(indices: &[u32], count: u32) -> String {
    let mut text = header_line(&catalog(), count);
    for index in indices {
        text.push_str(&format!("{index}\n"));
    }
    text
}

#[test]
fn a_log_says_which_mutants_the_test_reached_with_a_state_it_would_have_changed() {
    let found = read(&log(&[3, 1, 3], 8), &catalog(), 8).expect("a log this run is about");
    assert_eq!(found.into_iter().collect::<Vec<u32>>(), [1, 3]);
}

#[test]
fn an_empty_log_is_a_test_that_infected_nothing_rather_than_a_test_that_did_not_run() {
    assert!(read("", &catalog(), 8).expect("nothing written").is_empty());
    assert!(
        read(&header_line(&catalog(), 8), &catalog(), 8)
            .expect("a header and no index")
            .is_empty()
    );
}

#[test]
fn several_processes_appending_to_one_log_read_as_one_answer() {
    let mut text = log(&[1], 8);
    text.push_str(&log(&[5], 8));
    let found = read(&text, &catalog(), 8).expect("two processes");
    assert_eq!(found.into_iter().collect::<Vec<u32>>(), [1, 5]);
}

#[test]
fn a_log_about_another_catalog_says_nothing_at_all() {
    let other = "b".repeat(64);
    let error = read(&log(&[1], 8), &other, 8).expect_err("a stale log");
    assert!(matches!(error, LogError::OtherCatalog { .. }), "{error}");
}

#[test]
fn a_log_written_against_a_different_number_of_mutants_says_nothing() {
    let error = read(&log(&[1], 8), &catalog(), 9).expect_err("a stale log");
    assert!(matches!(error, LogError::Malformed { .. }), "{error}");
}

#[test]
fn an_index_no_mutant_answers_to_says_nothing_rather_than_the_ones_before_it() {
    let error = read(&log(&[1, 99], 8), &catalog(), 8).expect_err("an index beyond the catalog");
    assert!(
        matches!(error, LogError::BeyondCatalog { index: 99, .. }),
        "{error}"
    );
}

#[test]
fn a_line_a_dying_process_did_not_finish_says_nothing_rather_than_the_prefix() {
    let mut text = log(&[1, 2], 8);
    text.push_str("3x\n");
    let error = read(&text, &catalog(), 8).expect_err("a truncated line");
    assert!(
        matches!(error, LogError::Malformed { line: 4, .. }),
        "{error}"
    );
}

#[test]
fn an_index_before_any_header_says_nothing_about_which_catalog_it_is_about() {
    let error = read("7\n", &catalog(), 8).expect_err("no header");
    assert!(matches!(error, LogError::Headless { line: 1 }), "{error}");
}

#[test]
fn a_header_that_is_not_a_header_says_nothing() {
    for text in [
        format!("{SCHEMA}\n"),
        format!("{SCHEMA} {}\n", catalog()),
        format!("{SCHEMA} {} 8 extra\n", catalog()),
        format!("{SCHEMA} {} many\n", catalog()),
    ] {
        assert!(
            read(&text, &catalog(), 8).is_err(),
            "a header that is not one: {text:?}"
        );
    }
}
