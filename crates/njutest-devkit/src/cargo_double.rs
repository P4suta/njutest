// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A `cargo metadata` document a test hands to a reader, spelled the way cargo spells one: five different notions of a package identity were written by hand across this tree and one of them was cargo's.

#![expect(
    clippy::panic,
    reason = "support for tests reports a setup failure by panicking: a test whose double \
              cannot be built has nothing left to assert"
)]

use std::path::{Path, PathBuf};

/// The identity cargo gives a package whose manifest sits in `directory`.
///
/// `path+file:///abs#version` where the directory is named after the package,
/// and `path+file:///abs#name@version` where it is not.
/// The spelling changed in cargo 1.77 and four of the five doubles in this tree still used the one before it.
///
/// # Panics
/// When `directory` is not valid UTF-8, which no fixture path is.
#[must_use]
pub fn package_id(directory: &Path, name: &str, version: &str) -> String {
    let at = crate::paths::utf8(directory);
    let named_after_it = directory
        .file_name()
        .is_some_and(|last| last == std::ffi::OsStr::new(name));
    if named_after_it {
        format!("path+file://{at}#{version}")
    } else {
        format!("path+file://{at}#{name}@{version}")
    }
}

/// The identity cargo gives a package it took from a registry.
#[must_use]
pub fn registry_id(registry: &str, name: &str, version: &str) -> String {
    format!("registry+{registry}#{name}@{version}")
}

/// One compilation target of a package.
#[derive(Debug, Clone)]
pub struct Target {
    /// What cargo builds it as: `lib`, `bin`, `test`, and the rest.
    pub kind: String,
    /// The target's name.
    pub name: String,
    /// The file cargo compiles.
    pub source: PathBuf,
}

impl Target {
    /// A library target named after its package, with the sources a fixture keeps.
    #[must_use]
    pub fn library(name: &str, source: &Path) -> Self {
        Self {
            kind: "lib".to_owned(),
            name: name.to_owned(),
            source: source.to_path_buf(),
        }
    }

    /// This target as cargo reports one.
    #[must_use]
    fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": [self.kind],
            "crate_types": [self.kind],
            "name": self.name,
            "src_path": crate::paths::utf8(&self.source),
            "edition": "2024",
            "test": true,
            "doctest": true,
            "harness": true,
        })
    }
}

/// One path dependency, which cargo always reports by an absolute path.
#[derive(Debug, Clone)]
pub struct PathDependency {
    /// The dependency's package name.
    pub name: String,
    /// The directory holding its manifest, absolute as cargo reports it.
    pub directory: PathBuf,
}

impl PathDependency {
    /// A dependency on the package whose manifest sits in `directory`.
    ///
    /// # Panics
    /// When `directory` is relative, which cargo never reports.
    #[must_use]
    pub fn on(name: &str, directory: &Path) -> Self {
        assert!(
            directory.is_absolute(),
            "cargo reports a path dependency by an absolute path, so a double giving {} a \
             relative one is testing an input that cannot arrive",
            crate::paths::utf8(directory)
        );
        Self {
            name: name.to_owned(),
            directory: directory.to_path_buf(),
        }
    }

    /// This dependency as cargo reports one.
    #[must_use]
    fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "kind": serde_json::Value::Null,
            "path": crate::paths::utf8(&self.directory),
        })
    }
}

/// One package of a document.
#[derive(Debug, Clone)]
pub struct Package {
    /// The package name.
    pub name: String,
    /// The version, as the manifest spells it.
    pub version: String,
    /// The directory holding its manifest.
    pub directory: PathBuf,
    /// Everything cargo would compile for it.
    pub targets: Vec<Target>,
    /// Its path dependencies.
    pub dependencies: Vec<PathDependency>,
}

impl Package {
    /// A package at `directory` with nothing in it yet.
    #[must_use]
    pub fn at(name: &str, directory: &Path) -> Self {
        Self {
            name: name.to_owned(),
            version: "0.1.0".to_owned(),
            directory: directory.to_path_buf(),
            targets: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// The same package with `target` added.
    #[must_use]
    pub fn building(mut self, target: Target) -> Self {
        self.targets.push(target);
        self
    }

    /// The same package with `dependency` added.
    #[must_use]
    pub fn reading(mut self, dependency: PathDependency) -> Self {
        self.dependencies.push(dependency);
        self
    }

    /// The identity cargo gives this package.
    #[must_use]
    pub fn id(&self) -> String {
        package_id(&self.directory, &self.name, &self.version)
    }

    /// The manifest cargo reads it from.
    #[must_use]
    pub fn manifest(&self) -> PathBuf {
        self.directory.join("Cargo.toml")
    }

    /// This package as cargo reports one.
    #[must_use]
    fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "version": self.version,
            "id": self.id(),
            "manifest_path": crate::paths::utf8(&self.manifest()),
            "targets": self.targets.iter().map(Target::value).collect::<Vec<_>>(),
            "dependencies": self
                .dependencies
                .iter()
                .map(PathDependency::value)
                .collect::<Vec<_>>(),
        })
    }
}

/// A whole `cargo metadata` document.
#[derive(Debug, Clone)]
pub struct Document {
    /// The workspace root.
    pub root: PathBuf,
    /// The target directory, which cargo puts under the root unless told otherwise.
    pub target_directory: PathBuf,
    /// Every package the document reports.
    pub packages: Vec<Package>,
}

impl Document {
    /// A document for the workspace rooted at `root`, holding nothing.
    #[must_use]
    pub fn of(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            target_directory: root.join("target"),
            packages: Vec::new(),
        }
    }

    /// The same document with `package` in it, as a workspace member.
    #[must_use]
    pub fn holding(mut self, package: Package) -> Self {
        self.packages.push(package);
        self
    }

    /// The document, as cargo would print it.
    ///
    /// # Panics
    /// When the value cannot be written as JSON, which a document of strings cannot fail to be.
    #[must_use]
    pub fn json(&self) -> String {
        let value = serde_json::json!({
            "version": 1,
            "workspace_root": crate::paths::utf8(&self.root),
            "target_directory": crate::paths::utf8(&self.target_directory),
            "workspace_members": self
                .packages
                .iter()
                .map(Package::id)
                .collect::<Vec<_>>(),
            "workspace_default_members": self
                .packages
                .iter()
                .map(Package::id)
                .collect::<Vec<_>>(),
            "packages": self.packages.iter().map(Package::value).collect::<Vec<_>>(),
            "resolve": serde_json::Value::Null,
        });
        match serde_json::to_string(&value) {
            Ok(text) => text,
            Err(error) => panic!("a metadata double is not JSON: {error}"),
        }
    }
}
