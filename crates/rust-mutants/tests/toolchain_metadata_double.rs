// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The document a test hands the metadata reader, against the one cargo prints.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::Path;
use std::process::Command;

use njutest_devkit::cargo_double::{Document, Package, PathDependency, Target};
use njutest_devkit::fixture::Fixture;
use rust_mutants::cargo::Metadata;
use serde_json::Value;

/// What cargo itself says about the workspace rooted at `root`.
fn real(root: &Path) -> (Metadata, Value) {
    let output = Command::new(njutest_devkit::paths::cargo_binary())
        .args(["metadata", "--locked", "--offline", "--format-version", "1"])
        .current_dir(root)
        .envs(njutest_devkit::paths::environment_for_a_run())
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo metadata: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    (
        Metadata::parse(&output.stdout).expect("the document this engine reads every run"),
        rust_mutants::strictjson::from_slice(&output.stdout).expect("cargo prints JSON"),
    )
}

/// The same workspace as a double built from what cargo said about it.
fn doubled(root: &Path, said: &Metadata) -> (Metadata, Value) {
    let mut document = Document::of(root);
    for package in &said.packages {
        let directory = package.manifest_dir();
        let mut held = Package::at(&package.name, directory);
        held.version.clone_from(&package.version);
        for target in &package.targets {
            let kind = target
                .kind
                .first()
                .map_or_else(|| "lib".to_owned(), Clone::clone);
            held = held.building(Target {
                kind,
                name: target.name.clone(),
                source: target.src_path.clone(),
            });
        }
        for dependency in &package.dependencies {
            if let Some(path) = &dependency.path {
                held = held.reading(PathDependency::on(&dependency.name, path));
            }
        }
        document = document.holding(held);
    }
    document.target_directory.clone_from(&said.target_directory);
    document
        .workspace_default_members
        .clone_from(&said.workspace_default_members);
    let value = document.json();
    (
        Metadata::parse(value.as_bytes()).expect("a double this engine reads"),
        rust_mutants::strictjson::from_slice(value.as_bytes()).expect("the double prints JSON"),
    )
}

fn matching<'a>(expected: &Value, actual: &'a [Value]) -> Option<&'a Value> {
    if let Some(id) = expected.get("id") {
        return actual
            .iter()
            .find(|candidate| candidate.get("id") == Some(id));
    }
    if let Some(name) = expected.get("name") {
        return actual.iter().find(|candidate| {
            candidate.get("name") == Some(name)
                && expected
                    .get("path")
                    .is_none_or(|path| candidate.get("path") == Some(path))
        });
    }
    actual.iter().find(|candidate| *candidate == expected)
}

fn assert_shape_subset(expected: &Value, actual: &Value, at: &str) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for (key, value) in expected {
                let next = format!("{at}.{key}");
                let actual = actual
                    .get(key)
                    .unwrap_or_else(|| panic!("{next}: cargo has no such key"));
                assert_shape_subset(value, actual, &next);
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            for value in expected {
                let matched = matching(value, actual)
                    .unwrap_or_else(|| panic!("{at}: cargo has no matching {value}"));
                assert_shape_subset(value, matched, at);
            }
        }
        (Value::Null, Value::Null)
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_)) => {}
        _ => panic!("{at}: double has {expected}, cargo has {actual}"),
    }
}

#[test]
fn a_typed_metadata_document_serializes_to_cargos_field_shapes() {
    for name in ["fixture-simple", "fixture-macros"] {
        let fixture = Fixture::copy(name);
        let (said, real_value) = real(fixture.root());
        let serialized = serde_json::to_value(&said).expect("typed cargo metadata serializes");
        assert_shape_subset(&real_value, &serialized, name);
    }
}

#[test]
fn a_double_of_a_metadata_document_parses_to_what_cargo_parses_to() {
    for name in ["fixture-simple", "fixture-macros"] {
        let fixture = Fixture::copy(name);
        let (said, real_value) = real(fixture.root());
        let (double, double_value) = doubled(fixture.root(), &said);
        assert_shape_subset(&double_value, &real_value, name);
        let serialized = serde_json::to_value(&double).expect("typed double serializes");
        assert_shape_subset(&double_value, &serialized, name);
        assert_eq!(
            double.workspace_root, said.workspace_root,
            "{name}: the reader is handed a different tree"
        );
        assert_eq!(double.version, said.version, "{name}");
        let mut wanted: Vec<&String> = said.workspace_members.iter().collect();
        let mut held: Vec<&String> = double.workspace_members.iter().collect();
        wanted.sort();
        held.sort();
        assert_eq!(
            held, wanted,
            "{name}: five spellings of a package identity were written by hand in this tree \
             and one of them was cargo's, which is what a double built from the type rather \
             than from memory is for"
        );
        for (one, other) in double.packages.iter().zip(&said.packages) {
            assert_eq!(one.id, other.id, "{name}: {}", other.name);
            assert_eq!(one.manifest_path, other.manifest_path, "{name}");
            let theirs: Vec<(&String, Option<&std::path::PathBuf>)> = other
                .dependencies
                .iter()
                .filter(|dependency| dependency.path.is_some())
                .map(|dependency| (&dependency.name, dependency.path.as_ref()))
                .collect();
            let ours: Vec<(&String, Option<&std::path::PathBuf>)> = one
                .dependencies
                .iter()
                .map(|dependency| (&dependency.name, dependency.path.as_ref()))
                .collect();
            assert_eq!(
                ours, theirs,
                "{name}: cargo reports a path dependency by an absolute path and the doubles \
                 gave relative ones, so every conclusion about one was drawn from an input \
                 that cannot arrive"
            );
        }
    }
}

#[test]
fn cargo_reports_a_path_dependency_by_an_absolute_path() {
    let fixture = Fixture::copy("fixture-macros");
    let (said, _) = real(fixture.root());
    let paths: Vec<&std::path::PathBuf> = said
        .packages
        .iter()
        .flat_map(|package| package.dependencies.iter())
        .filter_map(|dependency| dependency.path.as_ref())
        .collect();
    assert!(
        !paths.is_empty(),
        "this fixture is the one with a path dependency; without one the law below says nothing"
    );
    for path in paths {
        assert!(
            path.is_absolute(),
            "a double giving a relative one exercises `manifest_dir.join(path)` down a branch \
             real input never takes: {}",
            path.display()
        );
    }
}
