// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one line of a file a command shows beside a guard.

use rust_mutants_cli::app::line_around;

#[test]
fn the_line_an_offset_sits_on_is_the_whole_line_and_none_of_its_ending() {
    let text = "first\nsecond\nthird\n";
    assert_eq!(line_around(text, 0).as_deref(), Some("first"));
    assert_eq!(line_around(text, 3).as_deref(), Some("first"));
    assert_eq!(
        line_around(text, 5).as_deref(),
        Some("first"),
        "an offset on the newline belongs to the line it ends"
    );
    assert_eq!(line_around(text, 6).as_deref(), Some("second"));
    assert_eq!(line_around(text, 13).as_deref(), Some("third"));
}

#[test]
fn a_line_of_a_crlf_tree_carries_no_carriage_return() {
    let text = "first\r\nsecond\r\n";
    assert_eq!(
        line_around(text, 7).as_deref(),
        Some("second"),
        "a \\r reaches the terminal as a move back to column zero, so a line that \
         carries one is a line the next thing written takes the place of"
    );
    assert_eq!(line_around(text, 0).as_deref(), Some("first"));
}

#[test]
fn a_file_that_ends_without_a_newline_still_has_a_last_line() {
    assert_eq!(line_around("only", 2).as_deref(), Some("only"));
    assert_eq!(line_around("a\nb", 2).as_deref(), Some("b"));
}

#[test]
fn an_offset_that_names_no_place_in_the_file_has_no_line() {
    assert_eq!(line_around("short", 99), None, "past the end");
    assert_eq!(
        line_around("あ", 1),
        None,
        "and inside a character rather than at one, where a guessed line would be \
         bytes nobody wrote"
    );
    assert_eq!(line_around("", 0).as_deref(), Some(""));
}
