// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The sealed build: the instrumented snapshot compiled for `wasm32-wasip1`, and what each of its test modules holds (ADR 0046).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::cargo::{CargoError, Compiled, Package, Unit};
use crate::execute::TestTarget;

/// The target a sealed build compiles for.
pub const TARGET: &str = "wasm32-wasip1";

/// The builds that make the sealed modules, in order: every test harness but an example's, which keeps going past one that refuses, then the examples.
pub const BUILDS: [crate::cargo::CompileKind; 2] = [
    crate::cargo::CompileKind::SealedTests,
    crate::cargo::CompileKind::SealedExamples,
];

/// Why a native test target has no sealed module, so that nothing only it reaches can be judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Unsealed {
    /// The toolchain holds no standard library for the sealed target.
    TargetMissing,
    /// The sealed build produced no module for the target: it does not build for the sealed target.
    NotBuilt,
}

impl Unsealed {
    /// What to do so that what this names seals.
    #[must_use]
    pub const fn remedy(self) -> &'static str {
        match self {
            Self::TargetMissing => "rustup target add wasm32-wasip1",
            Self::NotBuilt => {
                "make the target's tests build for wasm32-wasip1, which the sealed build's own \
                 error names, or put what cannot build behind cfg(not(target_family = \"wasm\"))"
            }
        }
    }
}

/// Whether the toolchain whose sysroot is `sysroot` holds the sealed target's standard library.
///
/// # Errors
/// A directory that is there and could not be listed, which says nothing about the target.
pub fn installed(sysroot: &Path) -> std::io::Result<bool> {
    let libdir = sysroot.join("lib").join("rustlib").join(TARGET).join("lib");
    let entries = match std::fs::read_dir(&libdir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = PathBuf::from(entry?.file_name());
        let standard = path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|stem| stem.starts_with("libstd-"));
        let archive = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rlib"));
        if standard && archive {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One test module of the sealed build: the sealed build of one native test target.
#[derive(Debug, Clone)]
pub struct Module {
    /// The target as the sealed build built it, whose executable is the module.
    pub target: TestTarget,
    /// Every source file compiled into it, absolute.
    pub sources: BTreeSet<PathBuf>,
}

/// What the sealed build produced for each native test target, and why it produced nothing for the rest.
#[derive(Debug, Clone, Default)]
pub struct SealedBuild {
    /// Each native target's sealed module, by target identity.
    pub modules: BTreeMap<String, Module>,
    /// Each native target with no sealed module, by target identity, and why.
    pub unsealed: BTreeMap<String, Unsealed>,
}

impl SealedBuild {
    /// A tree with nothing sealed, each of `native` for `why`.
    #[must_use]
    pub fn none(native: &[TestTarget], why: Unsealed) -> Self {
        Self {
            modules: BTreeMap::new(),
            unsealed: native
                .iter()
                .map(|target| (target.id().to_owned(), why))
                .collect(),
        }
    }

    /// What the sealed builds, each of [`BUILDS`] as `compiled` into `target_dir`, answer for each of `native`.
    ///
    /// # Errors
    /// What reading the sealed build's targets refuses: a manifest that cannot be read.
    pub fn of(
        native: &[TestTarget],
        compiled: &[Compiled],
        (packages, target_dir): (&[Package], &Path),
    ) -> Result<Self, CargoError> {
        let mut modules = BTreeMap::new();
        for build in compiled {
            for target in crate::execute::targets_of(&build.messages, packages, target_dir)? {
                let sources = sources_of(&target, &build.units, packages);
                modules.insert(target.id().to_owned(), Module { target, sources });
            }
        }
        let unsealed = native
            .iter()
            .filter(|target| !modules.contains_key(target.id()))
            .map(|target| (target.id().to_owned(), Unsealed::NotBuilt))
            .collect();
        modules.retain(|id, _| native.iter().any(|target| target.id() == id));
        Ok(Self { modules, unsealed })
    }

    /// Whether the sealed build compiled `source` into the module of `target`.
    #[must_use]
    pub fn compiled(&self, target: &str, source: &Path) -> bool {
        self.modules
            .get(target)
            .is_some_and(|module| module.sources.contains(source))
    }
}

/// Every source file the compiler read for the test unit `target` is the module of.
fn sources_of(target: &TestTarget, units: &[Unit], packages: &[Package]) -> BTreeSet<PathBuf> {
    units
        .iter()
        .filter(|unit| unit.test && unit.target.name == target.name())
        .filter(|unit| {
            packages
                .iter()
                .any(|package| package.id == unit.package_id && package.name == target.package())
        })
        .flat_map(|unit| unit.sources.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests;
