// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every place a workspace reads code from that a copy of it would not hold.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::{Path, PathBuf};

use njutest_devkit::cargo_double::{Document, Package, PathDependency};
use rust_mutants::cargo::manifest::Patch;
use rust_mutants::cargo::{Metadata, reaching_outside};

/// A metadata document for one member at `root/crates/a` reading from the given directories.
///
/// Every path is absolute, which is the only kind cargo reports; the document this replaced gave relative ones, so each conclusion below was drawn from an input that cannot arrive.
/// `toolchain_metadata_double` is where that is held against cargo itself.
fn document(root: &Path, dependencies: &[(&str, &str)]) -> Metadata {
    let mut package = Package::at("a", &root.join("crates/a"));
    for (name, at) in dependencies {
        package = package.reading(PathDependency::on(name, Path::new(at)));
    }
    let json = Document::of(root).holding(package).json();
    Metadata::parse(json.as_bytes()).expect("the document parses")
}

#[test]
fn a_path_dependency_inside_the_tree_reaches_nowhere() {
    let root = Path::new("/w");
    let metadata = document(
        root,
        &[("b", "/w/b"), ("c", "/w/crates/a/vendor/c"), ("d", "/w/d")],
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
    let metadata = document(root, &[("outside", "/elsewhere")]);
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
    let mut metadata = document(root, &[("outside", "/elsewhere")]);
    for package in &mut metadata.packages {
        package.manifest_path = PathBuf::from("/registry/a-0.1.0/Cargo.toml");
    }
    assert!(
        reaching_outside(&metadata, root, &[]).is_empty(),
        "a dependency of a dependency is not the tree's business: what it reads is already in \
         the registry cache, which a copy of the tree does not need to hold"
    );
}

#[test]
fn a_manifest_that_is_there_and_will_not_parse_is_a_refusal_rather_than_nothing() {
    let held = tempfile::tempdir().expect("a temporary directory");
    let manifest = held.path().join("Cargo.toml");
    std::fs::write(
        &manifest,
        b"[patch.crates-io\nserde = { path = \"../serde\" }\n",
    )
    .expect("the manifest is written");
    let refused = rust_mutants::cargo::manifest::patches(held.path());
    let error = match refused {
        Ok(found) => panic!(
            "a manifest that does not parse said the tree patches nothing, which is the \
             same answer as a tree that patches nothing: {found:?}"
        ),
        Err(error) => error,
    };
    assert_eq!(error.code().code, "RM1020", "{error}");
    let absent = rust_mutants::cargo::manifest::patches(&held.path().join("nowhere"));
    assert_eq!(
        absent.map_err(|error| error.to_string()),
        Ok(Vec::new()),
        "while a manifest that is not there is a tree with no patches, which it is"
    );
}
