// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What names a waived catch-all, and whether that name tells any two of them apart.

#![expect(clippy::indexing_slicing, reason = "a test reads its cases as a table")]

use njutest_devkit::result::{ResultState, result_state};
use xtask::lints::{Wildcard, wildcards_over as checked_wildcards_over};

fn wildcards_over(source: &str, ours: &[String]) -> Vec<Wildcard> {
    let parsed = checked_wildcards_over(source, ours);
    assert_eq!(
        result_state(&parsed),
        ResultState::Returned,
        "the literal source did not parse: {parsed:?}"
    );
    match parsed {
        Ok(wildcards) => wildcards,
        Err(_already_reported) => Vec::new(),
    }
}

#[test]
fn imported_or_aliased_variants_do_not_hide_the_closed_set() {
    for source in [
        "enum Decision { Tests, Steps, More }\nuse Decision::Tests;\nfn read(one: Decision) -> bool { match one { Tests => true, _ => false } }\n",
        "enum Decision { Tests, Steps, More }\nuse Decision as D;\nfn read(one: Decision) -> bool { match one { D::Tests => true, _ => false } }\n",
        "mod model { pub enum Decision { Tests, Steps, More } }\nuse model::Decision::*;\nfn read(one: model::Decision) -> bool { match one { Tests => true, _ => false } }\n",
        "enum Decision { Tests, Steps, More }\nfn read(one: Decision) -> bool { match one { Decision::Tests | Decision::Steps => true, _ => false } }\n",
    ] {
        let found = wildcards_over(source, &["Decision".to_owned()]);
        assert_eq!(found.len(), 1, "{source}");
        assert_eq!(found[0].over, "Decision", "{source}");
    }
}

#[test]
fn malformed_source_is_not_an_empty_waiver_set() {
    let parsed = checked_wildcards_over("fn (", &[]);
    assert_eq!(
        result_state(&parsed),
        ResultState::Refused,
        "malformed source became a waiver inventory: {parsed:?}"
    );
}

#[test]
fn the_scrutinee_type_exposes_an_all_wildcard_match() {
    let source = "enum Decision { Tests, Steps }\nfn read(one: Decision) -> bool { match one { _ => false } }\n";
    let found = wildcards_over(source, &["Decision".to_owned()]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].over, "Decision");
}

#[test]
fn an_arm_added_beside_a_waived_one_is_a_name_nobody_has_granted() {
    let one = wildcards_over(
        "fn read(a: Decision) {\n\
         match a {\n\
         Decision::Tests => (),\n\
         _ => (),\n\
         }\n\
         }\n",
        &["Decision".to_owned()],
    );
    let two = wildcards_over(
        "fn read(a: Decision) {\n\
         match a {\n\
         Decision::Tests => (),\n\
         _ => (),\n\
         }\n\
         match a {\n\
         Decision::Steps => (),\n\
         _ => (),\n\
         }\n\
         }\n",
        &["Decision".to_owned()],
    );
    assert_eq!((one.len(), two.len()), (1, 2));
    assert_ne!(
        one[0].key("a.rs", one.len()),
        two[0].key("a.rs", two.len()),
        "three arms absorbing one set in one item are one claim -- the rest of \
         this set, here -- and a fourth is a claim nobody read. Without the \
         count in the name, the waiver granted for the first would cover every \
         one written after it, which is the defect this key exists to remove"
    );
    assert_eq!(
        one[0].key("a.rs", one.len()),
        "a.rs::read over Decision, 1 arm"
    );
    assert_eq!(
        two[0].key("a.rs", two.len()),
        "a.rs::read over Decision, 2 arms"
    );
}

#[test]
fn a_name_says_what_is_waived_rather_than_where_it_sits() {
    let wild = wildcards_over(
        "enum Decision { Tests, Steps }\n\
         fn read(one: Decision) -> bool {\n\
         match one {\n\
         Decision::Tests => true,\n\
         _ => false,\n\
         }\n\
         }\n",
        &["Decision".to_owned()],
    );
    assert_eq!(wild.len(), 1, "one catch-all: {wild:?}");
    assert_eq!(wild[0].key("a.rs", 1), "a.rs::read over Decision, 1 arm");

    let moved = wildcards_over(
        "enum Decision { Tests, Steps }\n\
         \n\
         \n\
         fn read(one: Decision) -> bool {\n\
         match one {\n\
         Decision::Tests => true,\n\
         _ => false,\n\
         }\n\
         }\n",
        &["Decision".to_owned()],
    );
    assert_eq!(
        moved[0].key("a.rs", 1),
        wild[0].key("a.rs", 1),
        "three blank lines move the coordinate and change nothing about what is \
         waived, so a reviewer is not asked again"
    );
    assert_ne!(
        moved[0].line, wild[0].line,
        "and the line really did move, so the first assertion is about the key \
         rather than about nothing"
    );
}

#[test]
fn swapping_the_set_above_a_catch_all_is_a_different_waiver() {
    let ours = ["Message".to_owned(), "Decision".to_owned()];
    let before = wildcards_over(
        "fn read(one: Message) -> bool {\n\
         match one {\n\
         Message::BuildFinished { .. } => true,\n\
         _ => false,\n\
         }\n\
         }\n",
        &ours,
    );
    let after = wildcards_over(
        "fn read(one: Message) -> bool {\n\
         match one {\n\
         Decision::Tests => true,\n\
         _ => false,\n\
         }\n\
         }\n",
        &ours,
    );
    assert_eq!(before[0].line, after[0].line, "the coordinate is unmoved");
    assert_ne!(
        before[0].key("a.rs", 1),
        after[0].key("a.rs", 1),
        "this is the edit that fooled the ledger: one line above an untouched \
         catch-all, and the arm went from absorbing the rest of Message to \
         absorbing the rest of Decision. A ledger keyed by the line saw nothing \
         to review, because nothing it was looking at had changed"
    );
    assert_eq!(before[0].key("a.rs", 1), "a.rs::read over Message, 1 arm");
    assert_eq!(after[0].key("a.rs", 1), "a.rs::read over Decision, 1 arm");
}

#[test]
fn a_catch_all_is_named_by_every_item_it_sits_inside() {
    let wild = wildcards_over(
        "enum Decision { Tests }\n\
         mod outer {\n\
         struct Held;\n\
         impl Held {\n\
         fn read(one: super::Decision) -> bool {\n\
         match one {\n\
         Decision::Tests => true,\n\
         _ => false,\n\
         }\n\
         }\n\
         }\n\
         }\n",
        &["Decision".to_owned()],
    );
    assert_eq!(
        wild[0].key("a.rs", 1),
        "a.rs::outer::Held::read over Decision, 1 arm",
        "two functions of one name in one file are two places, and a name that \
         could not tell them apart would waive whichever the ledger met first"
    );
}
