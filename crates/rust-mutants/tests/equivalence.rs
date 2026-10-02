// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether the compiler renders a mutation identically to the program it mutates.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants::cargo::Provenance;
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
            changed,
            Provenance::Compiler,
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
            changed,
            Provenance::Compiler,
        ),
        Recompiled::Reused,
        "one unit that read the file and was reused is an executable built before the change"
    );
    assert_eq!(
        recompiled(
            &[unit("left", &["/tree/left/src/lib.rs"], false)],
            changed,
            Provenance::Compiler,
        ),
        Recompiled::Unread,
        "a build no unit of which read the file compiled nothing from it"
    );
}

#[test]
fn a_verified_reuse_compiled_the_bytes_but_is_no_independent_witness() {
    let changed = std::path::Path::new("/tree/src/lib.rs");
    let reused = [unit("left", &["/tree/src/lib.rs"], true)];
    assert_eq!(
        recompiled(&reused, changed, Provenance::Compiler),
        Recompiled::Reused,
        "a real compiler process that says it reused the unit's artifact is one that compared \
         nothing compiled from the changed bytes: cargo's fresh bit is the honesty this check \
         exists to hold, and a verified record does not excuse it"
    );
    assert_eq!(
        recompiled(&reused, changed, Provenance::VerifiedReuse),
        Recompiled::Every,
        "the engine's record answers only after verifying every bound input digest, and its \
         artifacts were produced by a real compiler run over exactly those bytes, so a reading \
         unit of the record was compiled from the changed file however the record marks it"
    );
    assert_eq!(
        recompiled(
            &[unit("left", &["/tree/left/src/lib.rs"], false)],
            changed,
            Provenance::VerifiedReuse,
        ),
        Recompiled::Unread,
        "verified or not, a build no unit of which read the file compiled nothing from it"
    );
}
