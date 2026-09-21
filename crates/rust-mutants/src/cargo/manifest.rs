// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a manifest says that `cargo metadata` does not.

use std::path::{Path, PathBuf};

use super::{CargoError, CargoErrorKind};

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

/// The text of the manifest at `path`, where absent and unreadable are different answers.
///
/// Each reader below returned an empty answer for both, so a manifest that could not be read looked exactly like one that declares nothing: a `[patch]` pointing outside the tree would be neither refused nor copied, a target built without the libtest harness would be read as one that has it,
/// and a crate that forbids the lints the generated module allows would be instrumented anyway.
///
/// # Errors
/// The file is there and could not be read.
fn text_of(path: &Path) -> Result<Option<String>, CargoError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CargoError::new(
            CargoErrorKind::ManifestUnreadable,
            format!("{} could not be read", path.display()),
        )
        .with_source(source)),
    }
}

/// A manifest's TOML, where a document that does not parse is a refusal rather than nothing.
///
/// # Errors
/// The text is not a TOML table.
fn table_of(path: &Path, text: &str) -> Result<toml::Table, CargoError> {
    text.parse::<toml::Table>().map_err(|source| {
        CargoError::new(
            CargoErrorKind::ManifestUnreadable,
            format!("{} is not a TOML document: {source}", path.display()),
        )
    })
}

/// Every `[patch]` entry of the manifest at `root` that names a directory.
///
/// # Errors
/// The manifest is there and could not be read or parsed.
pub fn patches(root: &Path) -> Result<Vec<Patch>, CargoError> {
    let path = root.join(FILE_NAME);
    let Some(text) = text_of(&path)? else {
        return Ok(Vec::new());
    };
    Ok(patches_in(&table_of(&path, &text)?))
}

/// Every `[patch]` entry of `text` that names a directory.
#[must_use]
pub fn read_patches(text: &str) -> Vec<Patch> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    patches_in(&document)
}

/// Every `[patch]` entry of a parsed manifest that names a directory.
fn patches_in(document: &toml::Table) -> Vec<Patch> {
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
///
/// # Errors
/// The manifest is there and could not be read or parsed.
pub fn harnesses(
    path: &Path,
) -> Result<std::collections::BTreeMap<(String, String), bool>, CargoError> {
    let Some(text) = text_of(path)? else {
        return Ok(std::collections::BTreeMap::new());
    };
    Ok(harnesses_in(&table_of(path, &text)?))
}

/// Whether each target `text` declares is built with the libtest harness.
#[must_use]
pub fn read_harnesses(text: &str) -> std::collections::BTreeMap<(String, String), bool> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return std::collections::BTreeMap::new();
    };
    harnesses_in(&document)
}

/// Whether each target a parsed manifest declares is built with the libtest harness.
fn harnesses_in(document: &toml::Table) -> std::collections::BTreeMap<(String, String), bool> {
    let mut found = std::collections::BTreeMap::new();
    if let Some(toml::Value::Table(one)) = document.get("lib")
        && let Some(harness) = one.get("harness").and_then(toml::Value::as_bool)
    {
        let name = one
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        found.entry(("lib".to_owned(), name)).or_insert(harness);
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
            found
                .entry((kind.to_owned(), name.to_owned()))
                .or_insert(harness);
        }
    }
    found
}

/// Every lint the manifest at `path` forbids, with the workspace lints it inherits.
///
/// # Errors
/// Either manifest is there and could not be read or parsed.
pub fn forbidden(path: &Path, workspace: Option<&Path>) -> Result<Vec<String>, CargoError> {
    let Some(text) = text_of(path)? else {
        return Ok(Vec::new());
    };
    let document = table_of(path, &text)?;
    let root = match workspace {
        Some(at) => match text_of(at)? {
            Some(text) => Some(table_of(at, &text)?),
            None => None,
        },
        None => None,
    };
    Ok(forbidden_of(&document, root.as_ref()))
}

/// Every lint `text` forbids, taking the workspace's own table when it asks to inherit it.
#[must_use]
pub fn read_forbidden(text: &str, workspace: Option<&str>) -> Vec<String> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let root = match workspace.map(str::parse::<toml::Table>) {
        None => None,
        Some(Ok(root)) => Some(root),
        Some(Err(_workspace_manifest_is_not_a_toml_table)) => return Vec::new(),
    };
    forbidden_of(&document, root.as_ref())
}

/// Every lint a parsed manifest forbids, taking the workspace's own table when it asks to inherit it.
fn forbidden_of(document: &toml::Table, workspace: Option<&toml::Table>) -> Vec<String> {
    let Some(toml::Value::Table(lints)) = document.get("lints") else {
        return Vec::new();
    };
    let mut found = forbidden_in(lints);
    if lints.get("workspace").and_then(toml::Value::as_bool) == Some(true)
        && let Some(root) = workspace
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
