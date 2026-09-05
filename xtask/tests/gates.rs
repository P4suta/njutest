// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gates, applied to this repository. This is the ratchet inside `cargo test`: a seam without a ledger line, a dependency in the wrong direction, a fixture without its lock file, or a version that drifted fails the suite, not only `cargo xtask`.

use xtask::gates;

#[test]
fn the_seam_ledger_agrees_with_the_tree() {
    let report =
        gates::devgates(&gates::workspace_root()).unwrap_or_else(|failure| panic!("{failure}"));
    assert!(report.starts_with("devgates: "), "{report}");
}

#[test]
fn every_internal_dependency_points_in_the_allowed_direction() {
    let report =
        gates::deps(&gates::workspace_root()).unwrap_or_else(|failure| panic!("{failure}"));
    assert!(report.starts_with("deps: "), "{report}");
}

#[test]
fn every_fixture_follows_the_conventions() {
    let report =
        gates::fixtures(&gates::workspace_root()).unwrap_or_else(|failure| panic!("{failure}"));
    assert!(report.starts_with("fixtures: "), "{report}");
}

#[test]
fn the_release_versions_agree() {
    let report = gates::release_check(&gates::workspace_root())
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert!(report.starts_with("release-check: "), "{report}");
}

#[test]
fn production_sources_exclude_test_support() {
    let files: Vec<String> = gates::production_sources(&gates::workspace_root())
        .iter()
        .map(|p| {
            p.strip_prefix(gates::workspace_root())
                .expect("inside")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        files.iter().any(|f| f == "crates/rust-mutants/src/lib.rs"),
        "{files:?}"
    );
    assert!(
        files.iter().any(|f| f == "xtask/src/devgates.rs"),
        "{files:?}"
    );
    assert!(
        files
            .iter()
            .all(|f| !f.starts_with("crates/mjutest-devkit/")),
        "{files:?}"
    );
    assert!(files.iter().all(|f| !f.contains("/tests/")), "{files:?}");
}
