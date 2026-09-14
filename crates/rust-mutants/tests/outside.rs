// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every place a workspace reads code from that a copy of it would not hold.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::{Path, PathBuf};

use rust_mutants::cargo::manifest::Patch;
use rust_mutants::cargo::{Metadata, reaching_outside};

/// A metadata document for one member at `root/crates/a` with the given path dependencies.
fn document(root: &Path, dependencies: &[(&str, &str)]) -> Metadata {
    let manifest = root.join("crates/a/Cargo.toml");
    let deps: Vec<String> = dependencies
        .iter()
        .map(|(name, path)| format!(r#"{{"name":"{name}","kind":null,"path":"{path}"}}"#))
        .collect();
    let json = format!(
        r#"{{"packages":[{{"name":"a","version":"0.1.0","id":"a 0.1.0","manifest_path":"{manifest}","targets":[],"dependencies":[{deps}]}}],"workspace_members":["a 0.1.0"],"workspace_root":"{root}","target_directory":"{target}","version":1,"resolve":null}}"#,
        manifest = njutest_devkit::paths::in_json(&manifest),
        root = njutest_devkit::paths::in_json(root),
        target = njutest_devkit::paths::in_json(&root.join("target")),
        deps = deps.join(","),
    );
    Metadata::parse(json.as_bytes()).expect("the document parses")
}

#[test]
fn a_path_dependency_inside_the_tree_reaches_nowhere() {
    let root = Path::new("/w");
    let metadata = document(
        root,
        &[("b", "../b"), ("c", "./vendor/c"), ("d", "../../d")],
    );
    assert!(
        reaching_outside(&metadata, root, &[]).is_empty(),
        "a sibling member, a vendored directory, and a path that climbs back into the tree are \
         all inside it"
    );
}

#[test]
fn a_path_dependency_outside_the_tree_is_named_with_the_manifest_that_declares_it() {
    let root = Path::new("/w");
    let metadata = document(root, &[("outside", "../../../elsewhere")]);
    let found = reaching_outside(&metadata, root, &[]);
    assert_eq!(found.len(), 1, "{found:?}");
    let one = found.first().expect("one");
    assert_eq!(one.name, "outside");
    assert_eq!(one.manifest, PathBuf::from("/w/crates/a/Cargo.toml"));
    assert_eq!(one.path, PathBuf::from("/elsewhere"));
}

#[test]
fn a_patch_that_points_outside_the_tree_is_named_too() {
    let root = Path::new("/w");
    let metadata = document(root, &[]);
    let patches = vec![
        Patch {
            source: "crates-io".to_owned(),
            name: "serde".to_owned(),
            path: PathBuf::from("../serde"),
        },
        Patch {
            source: "crates-io".to_owned(),
            name: "regex".to_owned(),
            path: PathBuf::from("vendor/regex"),
        },
    ];
    let found = reaching_outside(&metadata, root, &patches);
    assert_eq!(found.len(), 1, "the vendored one is inside: {found:?}");
    let one = found.first().expect("one");
    assert_eq!(one.name, "serde");
    assert_eq!(
        one.manifest,
        PathBuf::from("/w/Cargo.toml"),
        "a patch is declared by the root manifest, whatever member reads it"
    );
}

#[test]
fn a_package_outside_the_tree_is_not_asked_what_it_depends_on() {
    let root = Path::new("/w");
    let mut metadata = document(root, &[("outside", "../../../elsewhere")]);
    for package in &mut metadata.packages {
        package.manifest_path = PathBuf::from("/registry/a-0.1.0/Cargo.toml");
    }
    assert!(
        reaching_outside(&metadata, root, &[]).is_empty(),
        "a dependency of a dependency is not the tree's business: what it reads is already in \
         the registry cache, which a copy of the tree does not need to hold"
    );
}
