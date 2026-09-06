// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a manifest says that `cargo metadata` does not.
//!
//! `cargo metadata` reports the graph it resolved, and a `[patch]` table is
//! not in it: the patch has already been applied, so the document names the
//! replacement without saying it was one. A run that copies a tree has to know
//! about a patch that points outside the tree, because inside the copy it
//! points at nothing.

use std::path::{Path, PathBuf};

/// The file every cargo project is named by.
pub const FILE_NAME: &str = "Cargo.toml";

/// One entry of a `[patch]` table that replaces a dependency with a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// The registry or source it patches, as the table names it.
    pub source: String,
    /// The crate the entry is about.
    pub name: String,
    /// The directory it reads that crate from.
    pub path: PathBuf,
}

/// Every `[patch]` entry of the manifest at `root` that names a directory.
///
/// A manifest that cannot be read or parsed has no patches to report: the
/// build will say so in its own words, and guessing at half a document is
/// worse than saying nothing.
#[must_use]
pub fn patches(root: &Path) -> Vec<Patch> {
    let Ok(text) = std::fs::read_to_string(root.join(FILE_NAME)) else {
        return Vec::new();
    };
    read_patches(&text)
}

/// Every `[patch]` entry of `text` that names a directory.
#[must_use]
pub fn read_patches(text: &str) -> Vec<Patch> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(toml::Value::Table(table)) = document.get("patch") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (source, entries) in table {
        let toml::Value::Table(entries) = entries else {
            continue;
        };
        for (name, entry) in entries {
            let Some(path) = entry.get("path").and_then(toml::Value::as_str) else {
                continue;
            };
            found.push(Patch {
                source: source.clone(),
                name: name.clone(),
                path: PathBuf::from(path),
            });
        }
    }
    found.sort_by(|one, other| (&one.source, &one.name).cmp(&(&other.source, &other.name)));
    found
}
