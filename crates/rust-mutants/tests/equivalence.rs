// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether the compiler renders a mutation identically to the program it mutates.

use rust_mutants::equivalence::artifacts::{
    Artifacts, DIFFERENT_TARGETS, Identity, NOTHING_TO_COMPARE, compare,
};

fn artifacts(entries: &[(&str, &str)]) -> Artifacts {
    entries
        .iter()
        .map(|(id, digest)| ((*id).to_owned(), (*digest).to_owned()))
        .collect()
}

#[test]
fn an_empty_artifact_set_is_not_a_proof() {
    assert_eq!(
        compare(&Artifacts::new(), &Artifacts::new()),
        Identity::NotEstablished(NOTHING_TO_COMPARE),
        "two empty sets are equal and are not two equal programs; a build that produced \
         nothing is a build this run learned nothing from"
    );
    assert_eq!(
        compare(&artifacts(&[("a", "1")]), &Artifacts::new()),
        Identity::NotEstablished(NOTHING_TO_COMPARE)
    );
}

#[test]
fn two_builds_of_different_targets_are_not_two_programs_to_compare() {
    assert_eq!(
        compare(
            &artifacts(&[("a", "1")]),
            &artifacts(&[("a", "1"), ("b", "2")])
        ),
        Identity::NotEstablished(DIFFERENT_TARGETS)
    );
}

#[test]
fn the_same_bytes_are_the_same_program_and_different_bytes_are_not() {
    assert_eq!(
        compare(
            &artifacts(&[("a", "1"), ("b", "2")]),
            &artifacts(&[("a", "1"), ("b", "2")])
        ),
        Identity::Identical
    );
    assert_eq!(
        compare(
            &artifacts(&[("a", "1"), ("b", "2")]),
            &artifacts(&[("a", "1"), ("b", "3")])
        ),
        Identity::Differs
    );
}
