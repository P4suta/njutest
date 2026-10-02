// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether the compiler renders a mutation identically to the program it mutates.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants::equivalence::artifacts::{
    Artifacts, DIFFERENT_TARGETS, Identity, NOTHING_TO_COMPARE, Recompiled, compare, recompiled,
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

/// A unit of `package` that read `inputs`, compiled again unless `fresh`.
fn unit(package: &str, inputs: &[&str], fresh: bool) -> rust_mutants::cargo::Unit {
    rust_mutants::cargo::Unit {
        package_id: package.to_owned(),
        target: serde_json::from_value(serde_json::json!({
            "name": package,
            "kind": ["lib"],
            "src_path": format!("/tree/{package}/src/lib.rs"),
        }))
        .expect("a target"),
        test: false,
        sources: Vec::new(),
        inputs: inputs.iter().map(std::path::PathBuf::from).collect(),
        env: std::collections::BTreeMap::new(),
        fresh,
    }
}

#[test]
fn a_comparison_speaks_only_for_a_build_that_compiled_the_changed_file_again() {
    let changed = std::path::Path::new("/tree/shared/util.rs");
    let spelled_by_dep_info = "/tree/left/src/../../shared/util.rs";
    assert_eq!(
        recompiled(
            &[
                unit(
                    "left",
                    &["/tree/left/src/lib.rs", spelled_by_dep_info],
                    false
                ),
                unit("right", &["/tree/right/src/util.rs"], true),
            ],
            changed
        ),
        Recompiled::Every,
        "the unit that read the file was compiled, however its dep-info spells the file, and a \
         unit cargo reused that read another file of the same name is no part of the question"
    );
    assert_eq!(
        recompiled(
            &[
                unit("left", &[spelled_by_dep_info], false),
                unit("right", &["/tree/right/src/../../shared/util.rs"], true),
            ],
            changed
        ),
        Recompiled::Reused,
        "one unit that read the file and was reused is an executable built before the change"
    );
    assert_eq!(
        recompiled(&[unit("left", &["/tree/left/src/lib.rs"], false)], changed),
        Recompiled::Unread,
        "a build no unit of which read the file compiled nothing from it"
    );
}
