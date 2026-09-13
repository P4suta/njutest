// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every place a workspace reads code from that a copy of it would not hold.
//!
//! A run measures a copy of the tree. Anything the build reads from outside
//! the tree is not in the copy, so the build inside the copy fails — with
//! cargo's words about a missing manifest, which say nothing about what a run
//! could have done instead. Finding them first is what lets the refusal name
//! the dependency, the manifest that declares it, and the flag that allows it.

use std::path::{Path, PathBuf};

use super::manifest::Patch;
use super::metadata::Metadata;

/// One thing a workspace reads from outside itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Outside {
    /// The dependency or patched crate.
    pub name: String,
    /// The manifest that declares it, relative to the root where it is inside it.
    pub manifest: PathBuf,
    /// Where it reads it from.
    pub path: PathBuf,
}

/// Every dependency and patch of `metadata` that reads from outside `root`, in name order.
///
/// A path is outside when it is not `root` and does not descend from it, both
/// resolved as far as the filesystem allows: a symbolic link that leaves the
/// tree leaves it, whatever the spelling says.
#[must_use]
pub fn reaching_outside(metadata: &Metadata, root: &Path, patches: &[Patch]) -> Vec<Outside> {
    let root = resolved(root);
    let mut found = Vec::new();
    for package in &metadata.packages {
        let manifest_dir = package.manifest_dir();
        if !within(&root, &resolved(manifest_dir)) {
            continue;
        }
        for dependency in &package.dependencies {
            let Some(path) = &dependency.path else {
                continue;
            };
            let full = resolved(&manifest_dir.join(path));
            if !within(&root, &full) {
                found.push(Outside {
                    name: dependency.name.clone(),
                    manifest: package.manifest_path.clone(),
                    path: full,
                });
            }
        }
    }
    for patch in patches {
        let full = resolved(&root.join(&patch.path));
        if !within(&root, &full) {
            found.push(Outside {
                name: patch.name.clone(),
                manifest: root.join(super::manifest::FILE_NAME),
                path: full,
            });
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The path with every `.` and `..` it can resolve resolved, and the rest as written.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_error| cleaned(path))
}

/// The path with `.` removed and `..` folded, without asking the filesystem.
fn cleaned(path: &Path) -> PathBuf {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if parts.len() > 1 {
                    let _climbed = parts.pop();
                }
            }
            other => parts.push(other.as_os_str().to_owned()),
        }
    }
    parts.iter().collect()
}

/// Whether `path` is `root` or descends from it.
fn within(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}
