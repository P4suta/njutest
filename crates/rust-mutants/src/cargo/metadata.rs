// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo metadata --format-version 1`, as much of it as the engine reads.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::locate::command_failed;
use super::{CargoError, CargoErrorKind, Driver};
use crate::runner::run;
use crate::trace::ExecRecord;

/// How much `cargo metadata` output is kept. A workspace whose metadata is
/// larger than this is not one the engine is going to instrument anyway.
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
    /// The kinds: `lib`, `bin`, `test`, `bench`, `example`, `custom-build`,
    /// `proc-macro`, or a library crate type.
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

impl Metadata {
    /// Parses a metadata document.
    ///
    /// # Errors
    ///
    /// [`CargoErrorKind::MetadataUnparsable`].
    pub fn parse(json: &[u8]) -> Result<Self, CargoError> {
        serde_json::from_slice(json).map_err(|source| {
            CargoError::new(
                CargoErrorKind::MetadataUnparsable,
                "cargo metadata did not print its document",
            )
            .with_source(source)
        })
    }

    /// Runs `cargo metadata --format-version 1` in the driver's directory and
    /// parses it.
    ///
    /// # Errors
    ///
    /// [`CargoErrorKind::CommandFailed`] with cargo's own words when the
    /// command fails, and [`CargoErrorKind::MetadataUnparsable`] otherwise.
    pub fn load(driver: &Driver<'_>, options: MetadataOptions) -> Result<Self, CargoError> {
        let mut args = vec!["metadata", "--format-version", "1"];
        if options.locked {
            args.push("--locked");
        }
        if options.offline {
            args.push("--offline");
        }
        let mut spec = driver.toolchain.command(driver.dir, args);
        spec.structured_stdout = Some(METADATA_OUTPUT_LIMIT);
        let result = run(&spec, driver.cancel);
        driver.trace.exec(ExecRecord::of(&spec, &result));
        if !result.ok() {
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
