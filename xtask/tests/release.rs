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

#[test]
fn a_renamed_crate_cannot_leave_the_release_train_pointing_at_a_name_that_is_gone() {
    let members = [
        "njutest",
        "njutest-macros",
        "rust-mutants",
        "rust-mutants-cli",
    ];
    let renamed = r#"
[[package]]
name = "njutest-cli"
changelog_include = ["njutest", "njutest-macros"]
git_tag_enable = true

[[package]]
name = "njutest"
"#;
    assert_eq!(
        xtask::release::release_train(renamed, &members),
        ["release-plz.toml names njutest-cli, which is no longer a workspace member"]
    );

    let corrected = r#"
[[package]]
name = "njutest"
changelog_include = ["njutest-macros"]
git_tag_enable = true
"#;
    assert!(xtask::release::release_train(corrected, &members).is_empty());
}

#[test]
fn the_tag_is_cut_by_exactly_one_package() {
    let members = ["njutest", "rust-mutants"];
    let none = "[[package]]\nname = \"njutest\"\n";
    assert_eq!(
        xtask::release::release_train(none, &members),
        ["exactly one package cuts the tag, because every crate shares one version; [] do"]
    );
    let two = "[[package]]\nname = \"njutest\"\ngit_tag_enable = true\n\n\
               [[package]]\nname = \"rust-mutants\"\ngit_tag_enable = true\n";
    assert_eq!(
        xtask::release::release_train(two, &members),
        [
            "exactly one package cuts the tag, because every crate shares one version; [\"njutest\", \"rust-mutants\"] do"
        ]
    );
}

#[test]
fn the_release_train_in_this_tree_names_only_packages_it_has() {
    let root = njutest_devkit::paths::workspace_root();
    let text = std::fs::read_to_string(root.join("release-plz.toml"))
        .unwrap_or_else(|error| panic!("release-plz.toml: {error}"));
    let members = njutest_devkit::census::members(&root);
    let names: Vec<&str> = members.iter().map(|one| one.name.as_str()).collect();
    let problems = xtask::release::release_train(&text, &names);
    assert!(problems.is_empty(), "{problems:?}");
}
