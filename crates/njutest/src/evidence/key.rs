// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The behaviour key of one test target: everything that could change what that target does, and nothing else.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rust_mutants::cargo::{BuildSelection, Metadata};

use super::digest::Fields;
use super::tree::Scan;

/// The domain hashed first for a behaviour key.
pub const KEY_DOMAIN: &str = "njutest-mutation-evidence-key-v5";

/// The domain separating a configured build's continuation journal from the run-wide identity shared by every build in the same verification.
const CONTINUATION_DOMAIN: &str = "njutest-build-continuation-v1";

/// APIs whose result depends on what is in a directory rather than on what a file says.
pub const DIRECTORY_READERS: [&str; 7] = [
    "read_dir",
    "walkdir",
    "globset",
    "include_dir",
    "current_dir",
    "glob::",
    "ignore::",
];

/// What every key of one run shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Common {
    /// The toolchain, as it names itself.
    pub toolchain: String,
    /// The target triple.
    pub platform: String,
    /// The environment the run selected, as names and values.
    pub environment: Vec<(String, String)>,
    /// The contract, by name.
    pub contract: String,
    /// The arguments the test binaries were given.
    pub test_args: Vec<String>,
    /// The canonical, typed Cargo selection for this particular configured build.
    ///
    /// Toolchain, resolved platform, and environment inputs remain separate fields in this key; this value does not claim to identify a binary.
    pub build: BuildSelection,
    /// How long one command may take, in milliseconds.
    pub timeout_ms: u64,
    /// How many times an active mutation guard may be entered before the execution is stopped.
    /// Zero disables the bound.
    pub steps: u64,
    /// The version of the runner and of the engine.
    pub versions: Vec<String>,
    /// The digest of the fuzz corpora.
    pub corpus: String,
    /// The digest of the running njutest, because two builds of it may mean two different things by the same answer.
    pub engine: String,
}

/// The domain hashed first for what a carried answer is keyed on beyond what the engine keys it on.
const RUNNER_DOMAIN: &str = "njutest-carry-runner-v1";

/// What njutest decides an answer under beyond what the engine's key holds.
///
/// That is the platform, the environment, the contract, the versions and the corpora; the toolchain, the arguments, the build, the bounds and njutest's own digest are fields of the engine's key already.
#[must_use]
pub fn runner(common: &Common) -> String {
    let environment: BTreeSet<&(String, String)> = common.environment.iter().collect();
    let mut fields = Fields::new(RUNNER_DOMAIN);
    fields
        .field("platform", &common.platform)
        .list(
            "environment",
            environment
                .iter()
                .map(|(name, value)| format!("{name}\u{0}{value}")),
        )
        .field("contract", &common.contract)
        .list("versions", &common.versions)
        .field("corpus", &common.corpus);
    fields.finish()
}

/// Binds a run-wide identity to one Cargo build selection.
///
/// Checkpoints and other continuation state use this narrower identity.
/// Two builds in one run intentionally share the run identity and must never share this one unless their typed inputs are identical.
#[must_use]
pub fn continuation_identity(run: &str, build: &BuildSelection) -> String {
    let mut fields = Fields::new(CONTINUATION_DOMAIN);
    fields
        .field("run", run)
        .field("build", build.digest().as_str());
    fields.finish()
}

/// The packages one target links, and what each of them is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// `name@version` for each package in the closure, sorted.
    pub packages: Vec<String>,
    /// The digest of each package's own sources, for a package whose sources this run read.
    pub sources: BTreeMap<String, String>,
    /// The digest of the resolved dependencies, which is what says a registry package's bytes are the bytes.
    pub dependencies: String,
    /// Whether any package in the closure reads a directory rather than a file, in which case the key is over the whole tree.
    pub reads_directories: bool,
    /// The digest of the whole tree, which a directory-reading package keys on.
    pub tree: String,
}

/// The behaviour key of one target: what it links, folded with what every key of this run shares.
#[must_use]
pub fn behaviour(linked: &Linked, common: &Common) -> String {
    key(KEY_DOMAIN, linked, common)
}

fn key(domain: &str, linked: &Linked, common: &Common) -> String {
    let environment: BTreeSet<&(String, String)> = common.environment.iter().collect();
    let mut fields = Fields::new(domain);
    fields
        .list("packages", &linked.packages)
        .list(
            "sources",
            linked
                .sources
                .iter()
                .map(|(name, value)| format!("{name}\u{0}{value}")),
        )
        .field("dependencies", &linked.dependencies)
        .field(
            "reads-directories",
            if linked.reads_directories {
                "yes"
            } else {
                "no"
            },
        )
        .field(
            "tree",
            if linked.reads_directories {
                &linked.tree
            } else {
                ""
            },
        )
        .field("toolchain", &common.toolchain)
        .field("platform", &common.platform)
        .field("engine", &common.engine)
        .list(
            "environment",
            environment
                .iter()
                .map(|(name, value)| format!("{name}={value}")),
        )
        .field("contract", &common.contract)
        .list("test-args", &common.test_args);
    fields
        .field("build", common.build.digest().as_str())
        .field("timeout", &common.timeout_ms.to_string())
        .field("steps", &common.steps.to_string())
        .list("versions", &common.versions)
        .field("corpus", &common.corpus);
    fields.finish()
}

/// The graph and the tree a key is read from.
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    /// The resolved dependency graph.
    pub metadata: &'a Metadata,
    /// The tree this run read.
    pub scan: &'a Scan,
    /// The workspace root.
    pub root: &'a Path,
    /// The digest of what the lock file resolved.
    pub dependencies: &'a str,
}

/// What one package's test binary links, read from the resolved graph and the tree this run scanned.
/// # Errors
/// Refuses a package directory whose platform spelling cannot be retained in the UTF-8 evidence key.
pub fn linked_by(
    reading: &Reading<'_>,
    package_id: &str,
) -> Result<Linked, crate::evidence::tree::ScanError> {
    let Reading {
        metadata,
        scan,
        root,
        dependencies,
    } = *reading;
    let closure = metadata.closure(package_id);
    let by_id: BTreeMap<&str, &rust_mutants::cargo::Package> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();
    let mut packages = Vec::new();
    let mut sources = Vec::new();
    let mut reads_directories = false;
    for id in &closure {
        let Some(package) = by_id.get(id.as_str()) else {
            packages.push(id.clone());
            continue;
        };
        let name = format!("{}@{}", package.name, package.version);
        packages.push(name.clone());
        let Some(prefix) = inside(root, package.manifest_dir())? else {
            continue;
        };
        sources.push((name, package_digest(root, scan, &prefix)));
        reads_directories |= reads_directories_under(root, scan, &prefix);
    }
    let unique_packages = packages
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let sources_by_display = sources.into_iter().collect();
    Ok(Linked {
        packages: unique_packages,
        sources: sources_by_display,
        dependencies: dependencies.to_owned(),
        reads_directories,
        tree: scan.tree.clone(),
    })
}

/// What is in a package that could change what its code does.
fn package_digest(root: &Path, scan: &Scan, prefix: &str) -> String {
    if names_its_own_data(root, scan, prefix) {
        return scan.under(prefix);
    }
    scan.under_matching(prefix, is_source)
}

/// Whether a path is one of a package's own sources or its manifest.
fn is_source(path: &str) -> bool {
    Path::new(path).extension() == Some(std::ffi::OsStr::new("rs")) || path.ends_with("Cargo.toml")
}

/// Whether a package reads files it does not compile: one whose sources name `include_str!` or `include_bytes!`, or that has a build script whose `rerun-if-changed` this release does not read.
fn names_its_own_data(root: &Path, scan: &Scan, prefix: &str) -> bool {
    let build_script = if prefix.is_empty() {
        "build.rs".to_owned()
    } else {
        format!("{}/build.rs", prefix.trim_end_matches('/'))
    };
    if scan.entries.contains_key(&build_script) {
        return true;
    }
    scan.paths_under(prefix, is_source).keys().any(|path| {
        std::fs::read_to_string(root.join(path))
            .is_ok_and(|text| text.contains("include_str!") || text.contains("include_bytes!"))
    })
}

/// `directory` as a slash-separated path relative to `root`, or nothing when it is not inside it — a path dependency outside the tree is not something this run measured.
fn inside(
    root: &Path,
    directory: &Path,
) -> Result<Option<String>, crate::evidence::tree::ScanError> {
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(_unavailable_physical_spelling) => root.to_path_buf(),
    };
    let directory = match directory.canonicalize() {
        Ok(directory) => directory,
        Err(_unavailable_physical_spelling) => directory.to_path_buf(),
    };
    let Ok(relative) = directory.strip_prefix(&root) else {
        return Ok(None);
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        let text = component.as_os_str().to_str().ok_or_else(|| {
            crate::evidence::tree::ScanError::PathNotUtf8 {
                path: relative.to_path_buf(),
            }
        })?;
        parts.push(text.to_owned());
    }
    Ok(Some(parts.join("/")))
}

/// Whether any Rust file under `prefix` names an API whose result depends on what is in a directory rather than on what a file says.
#[must_use]
pub fn reads_directories_under(root: &Path, scan: &Scan, prefix: &str) -> bool {
    let inside = format!("{}/", prefix.trim_end_matches('/'));
    scan.entries
        .keys()
        .filter(|path| prefix.is_empty() || path.starts_with(&inside))
        .filter(|path| Path::new(path.as_str()).extension() == Some(std::ffi::OsStr::new("rs")))
        .any(|path| {
            std::fs::read_to_string(root.join(path))
                .is_ok_and(|text| DIRECTORY_READERS.iter().any(|reader| text.contains(reader)))
        })
}
