// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Milestone references cannot silently become plausible, undefined names.

use std::collections::BTreeSet;

use njutest_devkit::result::{ResultState, result_state};

#[test]
fn the_roadmap_table_is_the_registry() {
    let roadmap = "| # | name |\n| --- | --- |\n| M1 ✓ | one |\n| K1 | proof |\n";
    assert_eq!(
        xtask::milestones::registry(roadmap),
        Ok(BTreeSet::from(["K1".to_owned(), "M1".to_owned()]))
    );
}

#[test]
fn prose_finds_milestones_but_not_compiler_diagnostics_or_shard_letters() {
    assert_eq!(
        xtask::milestones::references("M14 follows E12; E0369 and K/N are not milestones"),
        BTreeSet::from(["E12".to_owned(), "M14".to_owned()])
    );
}

#[test]
fn a_duplicate_registry_entry_is_refused() {
    let repeated = "| M1 | one |\n| M1 ✓ | again |\n";
    let result = xtask::milestones::registry(repeated);
    assert_eq!(
        result_state(&result),
        ResultState::Refused,
        "M1 appeared twice but produced {result:?}"
    );
    let Err(error) = result else { return };
    assert!(error.to_string().contains("M1"), "{error}");
}
