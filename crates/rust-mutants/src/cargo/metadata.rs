// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo metadata --format-version 1`, as much of it as the engine reads.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::locate::command_failed;
use super::{CargoError, CargoErrorKind, Driver};
use crate::runner::run;
use crate::trace::ExecRecord;

/// How much `cargo metadata` output is kept.
/// A workspace whose metadata is larger than this is not one the engine is going to instrument anyway.
const METADATA_OUTPUT_LIMIT: usize = 256 << 20;

/// Configures [`Metadata::load`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MetadataOptions {
    /// Pass `--locked`: refuse to change `Cargo.lock`.
    pub locked: bool,
    /// Pass `--offline`: never touch the network.
    pub offline: bool,
}

/// The metadata document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Metadata {
    /// The format version, `1`.
    pub version: u32,
    /// The absolute workspace root.
    pub workspace_root: PathBuf,
    /// The absolute target directory.
    pub target_directory: PathBuf,
    /// The ids of the workspace members.
    pub workspace_members: Vec<String>,
    /// The ids of the default members.
    #[serde(default)]
    pub workspace_default_members: Vec<String>,
    /// Every package in the graph, members and dependencies alike.
    pub packages: Vec<Package>,
    /// The resolved dependency graph, when cargo produced one.
    #[serde(default)]
    pub resolve: Option<Resolve>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// The resolved dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Resolve {
    /// One node per package in the graph.
    #[serde(default)]
    pub nodes: Vec<Node>,
    /// The root package, for a single-package workspace.
    #[serde(default)]
    pub root: Option<String>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// One package's edges in the resolved graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Node {
    /// The package this node is about.
    pub id: String,
    /// What it depends on.
    #[serde(default)]
    pub deps: Vec<NodeDep>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// One edge of the resolved graph.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NodeDep {
    /// The package depended on.
    pub pkg: String,
    /// How it is depended on.
    /// A dependency may be several kinds at once.
    #[serde(default)]
    pub dep_kinds: Vec<DepKind>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// One way one package depends on another.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DepKind {
    /// `null` for a normal dependency, `"dev"` or `"build"` otherwise.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

impl DepKind {
    /// The name of a normal dependency, which cargo writes as the absence of a name.
    pub const NORMAL: &'static str = "normal";

    /// The kind, with a normal dependency named rather than absent.
    #[must_use]
    pub fn name(&self) -> &str {
        self.kind.as_deref().unwrap_or(Self::NORMAL)
    }
}

/// One package.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    /// The package id, as cargo spells it.
    pub id: String,
    /// The package name.
    pub name: String,
    /// The package version.
    pub version: String,
    /// The absolute path of its `Cargo.toml`.
    pub manifest_path: PathBuf,
    /// The edition.
    #[serde(default)]
    pub edition: String,
    /// Its targets.
    #[serde(default)]
    pub targets: Vec<Target>,
    /// Who the manifest names as authors.
    #[serde(default)]
    pub authors: Vec<String>,
    /// What the manifest says the package is.
    #[serde(default)]
    pub description: Option<String>,
    /// The homepage the manifest names.
    #[serde(default)]
    pub homepage: Option<String>,
    /// The repository the manifest names.
    #[serde(default)]
    pub repository: Option<String>,
    /// The licence expression the manifest names.
    #[serde(default)]
    pub license: Option<String>,
    /// The licence file the manifest names.
    #[serde(default)]
    pub license_file: Option<PathBuf>,
    /// The rust version the manifest requires.
    #[serde(default)]
    pub rust_version: Option<String>,
    /// The readme the manifest names.
    #[serde(default)]
    pub readme: Option<PathBuf>,
    /// What its manifest says it depends on, before anything is resolved.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// The native library its manifest says it links, which is code no Rust source shows.
    #[serde(default)]
    pub links: Option<String>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// One dependency, as the manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Dependency {
    /// The dependency's name.
    pub name: String,
    /// `null` for a normal dependency, `dev` or `build` otherwise.
    #[serde(default)]
    pub kind: Option<String>,
    /// The directory it is read from, for a path dependency.
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

impl Dependency {
    /// What kind of edge it is, in the word cargo uses.
    #[must_use]
    pub fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or(DepKind::NORMAL)
    }
}

impl Package {
    /// The directory holding the manifest.
    #[must_use]
    pub fn manifest_dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(&self.manifest_path)
    }
}

/// One target of a package.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Target {
    /// The target name.
    pub name: String,
    /// The kinds: `lib`, `bin`, `test`, `bench`, `example`, `custom-build`, `proc-macro`, or a library crate type.
    pub kind: Vec<String>,
    /// The crate types.
    #[serde(default)]
    pub crate_types: Vec<String>,
    /// The absolute path of the crate root.
    pub src_path: PathBuf,
    /// The edition.
    #[serde(default)]
    pub edition: String,
    /// Whether the target is tested by default.
    #[serde(default = "yes")]
    pub test: bool,
    /// Whether the target's documentation is tested.
    #[serde(default)]
    pub doctest: bool,
    /// Whether the target uses the libtest harness.
    #[serde(default = "yes")]
    pub harness: bool,
    #[serde(flatten)]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

const fn yes() -> bool {
    true
}

impl Target {
    fn has_kind(&self, kind: &str) -> bool {
        self.kind.iter().any(|k| k == kind)
    }

    /// Whether the target is a procedural macro.
    #[must_use]
    pub fn is_proc_macro(&self) -> bool {
        self.has_kind("proc-macro") || self.crate_types.iter().any(|c| c == "proc-macro")
    }

    /// Whether the target is a build script.
    #[must_use]
    pub fn is_custom_build(&self) -> bool {
        self.has_kind("custom-build")
    }

    /// Whether the target is a library of any crate type.
    #[must_use]
    pub fn is_lib(&self) -> bool {
        self.kind.iter().any(|k| {
            matches!(
                k.as_str(),
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib"
            )
        })
    }

    /// Whether the target is a binary.
    #[must_use]
    pub fn is_bin(&self) -> bool {
        self.has_kind("bin")
    }

    /// Whether the target is an integration test.
    #[must_use]
    pub fn is_test(&self) -> bool {
        self.has_kind("test")
    }

    /// Whether the target is a benchmark.
    #[must_use]
    pub fn is_bench(&self) -> bool {
        self.has_kind("bench")
    }

    /// Whether the target is an example.
    #[must_use]
    pub fn is_example(&self) -> bool {
        self.has_kind("example")
    }
}

/// What one `cargo metadata` is asked, with the promises a run made about the network and the lock file kept.
#[must_use]
pub fn metadata_arguments(options: MetadataOptions, no_deps: bool) -> Vec<&'static str> {
    let mut args = vec!["metadata", "--format-version", "1"];
    if no_deps {
        args.push("--no-deps");
    }
    if options.locked {
        args.push("--locked");
    }
    if options.offline {
        args.push("--offline");
    }
    args
}

impl Metadata {
    /// Parses a metadata document.
    ///
    /// # Errors
    /// [`CargoErrorKind::MetadataUnparsable`].
    pub fn parse(json: &[u8]) -> Result<Self, CargoError> {
        crate::strictjson::decode_slice(json).map_err(|source| {
            CargoError::new(
                CargoErrorKind::MetadataUnparsable,
                "cargo metadata did not print its document",
            )
            .with_source(source)
        })
    }

    /// Runs `cargo metadata --format-version 1 --no-deps` in the driver's directory and parses it.
    ///
    /// # Errors
    /// The failure of the command, and a document that is not one.
    pub fn load_no_deps(driver: &Driver<'_>, options: MetadataOptions) -> Result<Self, CargoError> {
        Self::run(driver, options, true)
    }

    /// Runs `cargo metadata --format-version 1` in the driver's directory and parses it.
    ///
    /// # Errors
    /// [`CargoErrorKind::CommandFailed`] with cargo's own words when the command fails, and [`CargoErrorKind::MetadataUnparsable`] otherwise.
    pub fn load(driver: &Driver<'_>, options: MetadataOptions) -> Result<Self, CargoError> {
        Self::run(driver, options, false)
    }

    /// One `cargo metadata`, resolved or not.
    fn run(
        driver: &Driver<'_>,
        options: MetadataOptions,
        no_deps: bool,
    ) -> Result<Self, CargoError> {
        let mut spec = driver
            .toolchain
            .command(driver.dir, metadata_arguments(options, no_deps));
        spec.structured_stdout = Some(METADATA_OUTPUT_LIMIT);
        let result = run(&spec, driver.cancel);
        driver.trace.exec_result(ExecRecord::of(&spec, &result));
        if !result.succeeded() {
            return Err(command_failed(&spec, &result));
        }
        if result.stdout_truncated {
            return Err(CargoError::new(
                CargoErrorKind::MetadataUnparsable,
                "cargo metadata printed more than the engine keeps",
            ));
        }
        Self::parse(&result.stdout)
    }

    /// Every package whose code goes into `id`'s test binary: `id` itself, everything it depends on through normal and build edges transitively, and its own development dependencies.
    #[must_use]
    pub fn closure(&self, id: &str) -> Vec<String> {
        let Some(resolve) = &self.resolve else {
            return self.packages.iter().map(|one| one.id.clone()).collect();
        };
        let by_id: std::collections::BTreeMap<&str, &Node> = resolve
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();
        let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut pending = vec![(id.to_owned(), true)];
        while let Some((current, is_root)) = pending.pop() {
            if !found.insert(current.clone()) {
                continue;
            }
            let Some(node) = by_id.get(current.as_str()) else {
                continue;
            };
            for edge in &node.deps {
                let linked = edge.dep_kinds.is_empty()
                    || edge.dep_kinds.iter().any(|kind| {
                        let name = kind.name();
                        name == DepKind::NORMAL || name == "build" || (is_root && name == "dev")
                    });
                if linked {
                    pending.push((edge.pkg.clone(), false));
                }
            }
        }
        found.into_iter().collect()
    }

    /// The workspace members, in document order.
    pub fn members(&self) -> impl Iterator<Item = &Package> {
        self.packages
            .iter()
            .filter(|package| self.workspace_members.contains(&package.id))
    }

    /// The package with `id`.
    #[must_use]
    pub fn package(&self, id: &str) -> Option<&Package> {
        self.packages.iter().find(|package| package.id == id)
    }
}

#[cfg(test)]
mod tests {
    use njutest_devkit::result::{ResultState::Returned, result_state};

    #[test]
    fn a_new_metadata_field_is_captured_instead_of_disappearing() {
        let parsed = crate::strictjson::decode_str::<super::Metadata>(
            r#"{"version":1,"workspace_root":"/demo","target_directory":"/demo/target","workspace_members":[],"packages":[],"future_cargo_field":true}"#,
        );
        assert_eq!(result_state(&parsed), Returned, "metadata: {parsed:?}");
        let Ok(metadata) = parsed else { return };
        assert_eq!(
            metadata.external_fields.get("future_cargo_field"),
            Some(&serde_json::Value::Bool(true))
        );
    }
}
