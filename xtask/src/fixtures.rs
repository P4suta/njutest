// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Conventions of the fixture projects under `fixtures/`.
//!
//! Each is an independent cargo workspace with a committed lock file and no
//! dependencies, so the integration suites can drive it offline and a
//! fixture that fails on purpose never fails this workspace.

use std::path::Path;

use walkdir::WalkDir;

/// The rule, for the failure message.
pub const RULE: &str = "A fixture is an independent cargo project: its Cargo.toml carries an empty \
    [workspace] table so cargo does not look upwards, its Cargo.lock is committed, it declares no \
    dependencies of any kind, and every .rs and Cargo.toml starts with the SPDX header. See \
    fixtures/README.md.";

const SPDX_HEADER: [&str; 2] = [
    "SPDX-FileCopyrightText: 2026 mjutest contributors",
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
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(toml::Value::Table(deps)) = table.get(key)
            && !deps.is_empty()
        {
            problems.push(format!(
                "Cargo.toml declares [{key}]; fixtures have no dependencies"
            ));
        }
    }
    problems
}
