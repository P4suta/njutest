// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is running, in each of the three ways it can say it.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_cli::cli::Ui;
use njutest_cli::ui::Notes;

fn said(kind: Ui, say: impl FnOnce(&mut Notes<'_>)) -> String {
    let mut out = Vec::new();
    {
        let mut notes = Notes::of(kind, &mut out);
        say(&mut notes);
    }
    String::from_utf8(out).expect("what a run says is text")
}

#[test]
fn the_plain_interface_writes_one_line_for_each_thing_that_happened() {
    let text = said(Ui::Plain, |notes| {
        notes.phase("baseline");
        notes.progress("demo/lib/demo tests::works", 1, 2);
        notes.note("cache", "a layer this run did not have");
    });
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.contains("== baseline"), "{text}");
    assert!(text.contains("[1/2]"), "{text}");
}

#[test]
fn the_jsonl_interface_writes_one_object_for_each_thing_that_happened() {
    let text = said(Ui::Jsonl, |notes| {
        notes.phase("mutation");
        notes.progress("abcd", 3, 9);
    });
    let objects: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("one object per line"))
        .collect();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0]["type"], "phase");
    assert_eq!(objects[1]["done"], 3);
}

#[test]
fn the_dashboard_keeps_one_block_and_redraws_it_rather_than_scrolling() {
    let text = said(Ui::Dashboard, |notes| {
        notes.phase("baseline");
        notes.progress("first", 1, 3);
        notes.progress("second", 2, 3);
        notes.progress("third", 3, 3);
    });
    assert!(
        text.contains('\r'),
        "a dashboard rewrites where it already wrote: {text:?}"
    );
    assert!(text.contains("baseline"), "{text:?}");
    assert!(
        text.contains("3/3"),
        "the last thing it says is where the run got to: {text:?}"
    );
    assert_eq!(
        text.bytes().filter(|byte| *byte == b'\n').count(),
        1,
        "redraws do not scroll, and dropping the dashboard ends its one line: {text:?}"
    );
    assert!(
        text.split('\r').filter(|part| !part.is_empty()).count() >= 4,
        "each step returns to the margin before it writes: {text:?}"
    );
}

#[test]
fn the_dashboard_leaves_the_terminal_as_it_found_it() {
    let text = said(Ui::Dashboard, |notes| {
        notes.phase("mutation");
        notes.progress("abcd", 1, 4);
    });
    assert!(
        text.ends_with('\n'),
        "the line after a dashboard starts at the left margin: {text:?}"
    );
}

#[test]
fn a_note_is_worth_a_line_of_its_own_even_on_a_dashboard() {
    let text = said(Ui::Dashboard, |notes| {
        notes.phase("open");
        notes.note("kept", "/tmp/njutest-run-abc");
    });
    assert!(
        text.contains("kept: /tmp/njutest-run-abc"),
        "a note says something the progress line never will: {text:?}"
    );
}

#[test]
fn every_interface_survives_a_message_that_would_break_a_line() {
    for kind in [Ui::Plain, Ui::Jsonl, Ui::Dashboard] {
        let text = said(kind, |notes| {
            notes.note("odd", "a\nb\tc\r\\d");
        });
        assert!(!text.is_empty(), "{kind:?}");
        assert!(
            text.lines().count() <= 2,
            "a message with a newline in it does not become two records: {kind:?} {text:?}"
        );
    }
}
