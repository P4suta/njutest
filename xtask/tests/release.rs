// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Version consistency.

use xtask::release::{check, workspace_version};

const ROOT: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n";
const MANIFEST: &str = "{\n  \".\": \"0.1.0\"\n}\n";

#[test]
fn a_consistent_workspace_has_no_problems() {
    let member = "[package]\nname = \"a\"\nversion.workspace = true\n";
    assert_eq!(workspace_version(ROOT).as_deref(), Some("0.1.0"));
    assert!(check(ROOT, MANIFEST, &[("crates/a/Cargo.toml", member)]).is_empty());
}

#[test]
fn a_manifest_that_disagrees_with_the_workspace_is_reported() {
    let problems = check(ROOT, "{ \".\": \"0.2.0\" }", &[]);
    assert_eq!(
        problems,
        [
            "Cargo.toml [workspace.package].version is 0.1.0 but .release-please-manifest.json says 0.2.0"
        ]
    );
}

#[test]
fn a_member_that_pins_its_own_version_is_reported() {
    let member = "[package]\nname = \"a\"\nversion = \"0.1.0\"\n";
    let problems = check(ROOT, MANIFEST, &[("crates/a/Cargo.toml", member)]);
    assert_eq!(
        problems,
        ["crates/a/Cargo.toml: [package] must inherit version.workspace = true"]
    );
}

#[test]
fn missing_versions_are_reported() {
    assert_eq!(
        check("[workspace]\n", MANIFEST, &[]),
        ["Cargo.toml has no [workspace.package].version"]
    );
    assert_eq!(
        check(ROOT, "{}", &[]),
        [".release-please-manifest.json has no \".\" entry"]
    );
}
