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

#[test]
fn every_gate_that_needs_no_argument_is_one_all_runs() {
    let root = gates::workspace_root();
    let source = std::fs::read_to_string(root.join("xtask/src/lib.rs"))
        .unwrap_or_else(|error| panic!("xtask/src/lib.rs: {error}"));
    let declaration = source
        .find("enum Gate {")
        .and_then(|at| source.get(at..))
        .unwrap_or_else(|| panic!("xtask/src/lib.rs no longer declares the gates"));
    let declaration = declaration
        .find("\n}\n")
        .and_then(|end| declaration.get(..end))
        .unwrap_or(declaration);

    let bare: Vec<String> = declaration
        .lines()
        .filter_map(|line| line.trim().strip_suffix(','))
        .filter(|name| {
            name.chars().next().is_some_and(char::is_uppercase)
                && name.chars().all(char::is_alphanumeric)
        })
        .filter(|name| *name != "All")
        .map(kebab)
        .collect();
    assert!(
        bare.len() >= 5,
        "the gates that need no argument are the ones a person runs as a set: {bare:?}"
    );

    let report = gates::all(&root).unwrap_or_else(|failure| panic!("{failure}"));
    let unrun: Vec<&String> = bare
        .iter()
        .filter(|name| !report.contains(&format!("{name}:")))
        .collect();
    assert!(
        unrun.is_empty(),
        "a gate `all` does not run is a gate `mise run check` does not run and continuous \
         integration does not run: it holds nothing, and the only sign is that it is \
         still in the help. {unrun:?} is declared and `all` never calls it:\n{report}"
    );
}

/// The name a gate answers to on the command line, from the name of its variant.
fn kebab(variant: &str) -> String {
    let mut said = String::new();
    for (at, character) in variant.char_indices() {
        if character.is_uppercase() && at > 0 {
            said.push('-');
        }
        said.extend(character.to_lowercase());
    }
    said
}
