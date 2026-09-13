// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Conventions of the fixture projects under `fixtures/`.

use std::path::Path;

use walkdir::WalkDir;

/// The rule, for the failure message.
pub const RULE: &str = "A fixture is an independent cargo project: its Cargo.toml carries a \
    [workspace] table so cargo does not look upwards, its Cargo.lock is committed, its only \
    dependencies are paths inside itself (fixtures build offline against no registry), every \
    .rs and Cargo.toml starts with the SPDX header, and its README.md states what a run of it \
    establishes in a ```fates block. See fixtures/README.md.";

/// The fence that opens the block of a README stating what a run of the fixture establishes.
pub const FATES_FENCE: &str = "```fates";

const SPDX_HEADER: [&str; 2] = [
    "SPDX-FileCopyrightText: 2026 njutest contributors",
    "SPDX-License-Identifier: MIT OR Apache-2.0",
];

/// Every convention `dir` breaks, one line each, sorted.
#[must_use]
pub fn check_fixture(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let manifest_path = dir.join("Cargo.toml");
    match std::fs::read_to_string(&manifest_path) {
        Ok(text) => problems.extend(check_manifest(&text)),
        Err(_) => problems.push("Cargo.toml is missing".to_owned()),
    }
    if !dir.join("Cargo.lock").is_file() {
        problems.push(
            "Cargo.lock is missing (commit it: fixtures build with --locked --offline)".to_owned(),
        );
    }
    problems.extend(check_readme(dir));
    for entry in WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with("target/") {
            continue;
        }
        let is_source =
            path.extension().is_some_and(|ext| ext == "rs") || entry.file_name() == "Cargo.toml";
        if is_source {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if !has_spdx_header(&text) {
                problems.push(format!("{relative}: missing the SPDX header"));
            }
        }
    }
    problems.sort();
    problems
}

/// The README, and the ledger of fates a fixture with mutable code has to keep.
fn check_readme(dir: &Path) -> Vec<String> {
    let Ok(readme) = std::fs::read_to_string(dir.join("README.md")) else {
        return vec!["README.md is missing (it is where a fixture says what it is for)".to_owned()];
    };
    if readme.contains(FATES_FENCE) {
        return Vec::new();
    }
    vec![format!(
        "README.md has no {FATES_FENCE} block; a fixture whose fates nothing states is one a \
         change can quietly re-decide"
    )]
}

fn has_spdx_header(text: &str) -> bool {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let second = lines.next().unwrap_or_default();
    first.contains(SPDX_HEADER[0]) && second.contains(SPDX_HEADER[1])
}

fn check_manifest(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let Ok(table) = text.parse::<toml::Table>() else {
        return vec!["Cargo.toml does not parse".to_owned()];
    };
    if !table.contains_key("workspace") {
        problems.push(
            "Cargo.toml needs an empty [workspace] table to stay independent of the root workspace"
                .to_owned(),
        );
    }
    check_dependencies(&table, &mut problems);
    problems
}

/// Every dependency table a manifest can hold: the three at the top, the three under each `[target.<cfg>]`, and the workspace's own.
fn check_dependencies(table: &toml::Table, problems: &mut Vec<String>) {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut holders: Vec<&toml::Table> = vec![table];
    if let Some(toml::Value::Table(workspace)) = table.get("workspace") {
        holders.push(workspace);
    }
    if let Some(toml::Value::Table(targets)) = table.get("target") {
        holders.extend(targets.values().filter_map(|value| match value {
            toml::Value::Table(target) => Some(target),
            _ => None,
        }));
    }
    let tables = holders.into_iter().flat_map(|holder| {
        KINDS
            .into_iter()
            .filter_map(move |kind| match holder.get(kind) {
                Some(toml::Value::Table(deps)) => Some(deps),
                _ => None,
            })
    });
    for deps in tables {
        for (name, value) in deps {
            if !is_local_path_dependency(value) {
                problems.push(format!(
                    "Cargo.toml: dependency {name:?} is not a path inside the fixture; \
                     fixtures build offline against no registry"
                ));
            }
        }
    }
}

/// Whether a dependency is a path inside the fixture: a table with a `path` that neither escapes nor is absolute, and no source that would need a network (`git`, a registry, or a bare version requirement).
fn is_local_path_dependency(value: &toml::Value) -> bool {
    let toml::Value::Table(spec) = value else {
        return false;
    };
    if ["git", "registry", "registry-index"]
        .iter()
        .any(|key| spec.contains_key(*key))
    {
        return false;
    }
    let Some(toml::Value::String(path)) = spec.get("path") else {
        return false;
    };
    if path.starts_with('/') || path.starts_with('\\') || path.contains(':') {
        return false;
    }
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    parts.iter().all(|component| *component != "..") || is_sibling_fixture(&parts)
}

/// Whether the path is `../fixture-…` and nothing more: the one shape allowed to climb.
///
/// A fixture that exists to have a dependency outside itself needs one, and
/// the thing outside has to be a fixture too, so that the suite still builds
/// from what this repository holds and still builds offline.
fn is_sibling_fixture(parts: &[&str]) -> bool {
    matches!(parts, [first, second]
        if *first == ".." && second.starts_with("fixture-"))
}
