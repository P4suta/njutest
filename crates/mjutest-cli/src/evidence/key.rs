// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The behaviour key of one test target: everything that could change what that target does, and nothing else.
//!
//! A key is an allowlist over what the run already digested for its own
//! identity. Two runs may reuse a verdict about a mutant only where the key of
//! the target that established it is the same, so the key must cover every
//! input to that target's behaviour and must not cover anything else —
//! diagnostics, parallelism, and how long the run took are outside every key
//! (ADR 0007).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rust_mutants::cargo::Metadata;

use super::digest::Fields;
use super::tree::Scan;

/// The domain hashed first for a behaviour key.
pub const KEY_DOMAIN: &str = "mjutest-mutation-evidence-key-v2";

/// APIs whose result depends on what is in a directory rather than on what a file says. A package that uses one keys the whole tree: Rust offers no portable, unprivileged observation of what a test actually read, so the selection is static and widens rather than trusts.
pub const DIRECTORY_READERS: [&str; 7] = [
    "read_dir",
    "walkdir",
    "globset",
    "include_dir",
    "current_dir",
    "glob",
    "ignore",
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
    /// The features the build selected.
    pub features: Vec<String>,
    /// How long one command may take, in milliseconds.
    pub timeout_ms: u64,
    /// The version of the runner and of the engine.
    pub versions: Vec<String>,
    /// The digest of the fuzz corpora.
    pub corpus: String,
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
        .list(
            "environment",
            environment
                .iter()
                .map(|(name, value)| format!("{name}={value}")),
        )
        .field("contract", &common.contract)
        .list("test-args", &common.test_args)
        .list("features", &common.features)
        .field("timeout", &common.timeout_ms.to_string())
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
#[must_use]
pub fn linked_by(reading: &Reading<'_>, package_id: &str) -> Linked {
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
    let mut packages = BTreeSet::new();
    let mut sources = BTreeMap::new();
    let mut reads_directories = false;
    for id in &closure {
        let Some(package) = by_id.get(id.as_str()) else {
            let _first = packages.insert(id.clone());
            continue;
        };
        let name = format!("{}@{}", package.name, package.version);
        let _first = packages.insert(name.clone());
        let Some(prefix) = inside(root, package.manifest_dir()) else {
            continue;
        };
        sources.insert(name, package_digest(root, scan, &prefix));
        reads_directories |= reads_directories_under(root, scan, &prefix);
    }
    Linked {
        packages: packages.into_iter().collect(),
        sources,
        dependencies: dependencies.to_owned(),
        reads_directories,
        tree: scan.tree.clone(),
    }
}

/// What is in a package that could change what its code does.
///
/// The key is an allowlist: a package's own Rust files and its manifest. A
/// README beside them is not something a test can read without saying so, and
/// a package that does say so — one whose sources name `include_str!`,
/// `include_bytes!`, or that carries a build script whose `rerun-if-changed`
/// this release does not read — is keyed on everything beside it instead.
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
fn inside(root: &Path, directory: &Path) -> Option<String> {
    let root = root
        .canonicalize()
        .unwrap_or_else(|_error| root.to_path_buf());
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_error| directory.to_path_buf());
    let relative = directory.strip_prefix(&root).ok()?;
    Some(
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join("/"),
    )
}

/// Whether any Rust file under `prefix` names an API whose result depends on what is in a directory rather than on what a file says.
///
/// The search is over the source text, so a mention in a comment counts. That
/// widens the key, which is the direction a key may be wrong in: a key that
/// covers too much makes a run do work it need not, and a key that covers too
/// little makes it claim what it did not establish.
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
