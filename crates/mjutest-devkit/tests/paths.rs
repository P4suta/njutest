// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The paths every suite resolves through the devkit.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use mjutest_devkit::paths::{cargo_binary, fixtures_dir, workspace_root};

#[test]
fn workspace_root_holds_the_workspace_manifest() {
    let root = workspace_root();
    assert!(root.is_absolute(), "{}", root.display());
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("root Cargo.toml");
    assert!(
        manifest.contains("[workspace]"),
        "not the workspace root: {}",
        root.display()
    );
}

#[test]
fn fixtures_dir_is_the_fixtures_directory_of_the_workspace() {
    assert_eq!(fixtures_dir(), workspace_root().join("fixtures"));
    assert!(fixtures_dir().is_dir(), "{}", fixtures_dir().display());
}

#[test]
fn cargo_binary_is_the_cargo_that_built_the_tests() {
    let cargo = cargo_binary();
    let output = std::process::Command::new(&cargo)
        .arg("--version")
        .output()
        .expect("runs");
    assert!(output.status.success(), "{}", cargo.display());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
}
