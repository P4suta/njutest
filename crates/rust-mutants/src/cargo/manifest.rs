// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a manifest says that `cargo metadata` does not.

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

/// Whether each target of the manifest at `path` is built with the libtest harness.
#[must_use]
pub fn harnesses(path: &Path) -> std::collections::BTreeMap<(String, String), bool> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return std::collections::BTreeMap::new();
    };
    read_harnesses(&text)
}

/// Whether each target `text` declares is built with the libtest harness.
#[must_use]
pub fn read_harnesses(text: &str) -> std::collections::BTreeMap<(String, String), bool> {
    let mut found = std::collections::BTreeMap::new();
    let Ok(document) = text.parse::<toml::Table>() else {
        return found;
    };
    if let Some(toml::Value::Table(one)) = document.get("lib")
        && let Some(harness) = one.get("harness").and_then(toml::Value::as_bool)
    {
        let name = one
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let _replaced = found.insert(("lib".to_owned(), name), harness);
    }
    for kind in ["bin", "test", "bench", "example"] {
        let Some(toml::Value::Array(entries)) = document.get(kind) else {
            continue;
        };
        for entry in entries {
            let (Some(name), Some(harness)) = (
                entry.get("name").and_then(toml::Value::as_str),
                entry.get("harness").and_then(toml::Value::as_bool),
            ) else {
                continue;
            };
            let _replaced = found.insert((kind.to_owned(), name.to_owned()), harness);
        }
    }
    found
}

/// Every lint the manifest at `path` forbids, with the workspace lints it inherits.
#[must_use]
pub fn forbidden(path: &Path, workspace: Option<&Path>) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let root = workspace.and_then(|path| std::fs::read_to_string(path).ok());
    read_forbidden(&text, root.as_deref())
}

/// Every lint `text` forbids, taking the workspace's own table when it asks to inherit it.
#[must_use]
pub fn read_forbidden(text: &str, workspace: Option<&str>) -> Vec<String> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(toml::Value::Table(lints)) = document.get("lints") else {
        return Vec::new();
    };
    let mut found = forbidden_in(lints);
    if lints.get("workspace").and_then(toml::Value::as_bool) == Some(true)
        && let Some(root) = workspace
        && let Ok(root) = root.parse::<toml::Table>()
        && let Some(toml::Value::Table(inherited)) =
            root.get("workspace").and_then(|table| table.get("lints"))
    {
        found.extend(forbidden_in(inherited));
    }
    found.sort();
    found.dedup();
    found
}

/// Every lint a `[lints]` table sets to `forbid`, spelled as the tool spells it.
fn forbidden_in(lints: &toml::Table) -> Vec<String> {
    let mut found = Vec::new();
    for (tool, entries) in lints {
        let toml::Value::Table(entries) = entries else {
            continue;
        };
        for (name, value) in entries {
            let level = match value {
                toml::Value::String(level) => Some(level.as_str()),
                toml::Value::Table(table) => table.get("level").and_then(toml::Value::as_str),
                _ => None,
            };
            if level == Some("forbid") {
                found.push(if tool == "rust" {
                    name.clone()
                } else {
                    format!("{tool}::{name}")
                });
            }
        }
    }
    found
}
