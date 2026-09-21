// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Version consistency.

use xtask::release::{check, workspace_version};

const ROOT: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n";

#[test]
fn a_consistent_workspace_has_no_problems() {
    let member = "[package]\nname = \"a\"\nversion.workspace = true\n";
    assert_eq!(workspace_version(ROOT).as_deref(), Some("0.1.0"));
    assert!(check(ROOT, &[("crates/a/Cargo.toml", member)]).is_empty());
}

#[test]
fn a_member_that_pins_its_own_version_is_reported() {
    let member = "[package]\nname = \"a\"\nversion = \"0.1.0\"\n";
    let problems = check(ROOT, &[("crates/a/Cargo.toml", member)]);
    assert_eq!(
        problems,
        ["crates/a/Cargo.toml: [package] must inherit version.workspace = true"],
        "release-plz bumps the workspace version and nothing else, so a member \
         carrying its own is one the next tag does not name"
    );
}

#[test]
fn a_workspace_with_no_version_is_reported() {
    assert_eq!(
        check("[workspace]\n", &[]),
        ["Cargo.toml has no [workspace.package].version"]
    );
}
