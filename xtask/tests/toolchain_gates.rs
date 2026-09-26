// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The repository gates run by `all`, with coverage measured separately in CI.

use xtask::gates;

mod claims_oracle;

#[test]
fn the_typed_repository_set_runs_in_all_and_coverage_is_separate() {
    let root = gates::workspace_root();
    let report = gates::all(&root).expect("every gate passes on this tree");
    assert!(report.contains("skipped:"), "{report}");
    assert!(!report.contains("coverage-ratchet:"), "{report}");

    let workflow = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect(".github/workflows/ci.yml");
    let (regular, coverage) = workflow
        .split_once("\n  coverage:\n")
        .expect("CI has a separate coverage job");
    assert!(regular.contains("run: cargo xtask all"));
    assert!(coverage.contains("run: cargo xtask coverage-ratchet"));
}
