// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Version consistency: `[workspace.package].version` and every member manifest inheriting it must agree, so a release tag names one version everywhere.

/// The `[workspace.package].version` of the root manifest, if it has one.
#[must_use]
pub fn workspace_version(root_manifest: &str) -> Option<String> {
    let Ok(table) = root_manifest.parse::<toml::Table>() else {
        return None;
    };
    table
        .get("workspace")?
        .get("package")?
        .get("version")?
        .as_str()
        .map(ToOwned::to_owned)
}

/// Every inconsistency between the root manifest and the member manifests `(label, text)`.
///
/// release-plz bumps `[workspace.package].version` and nothing else, so the root manifest is the only place a version is written; what this holds is that every member takes it from there rather than carrying its own.
#[must_use]
pub fn check(root_manifest: &str, members: &[(&str, &str)]) -> Vec<String> {
    let mut problems = Vec::new();
    if workspace_version(root_manifest).is_none() {
        problems.push("Cargo.toml has no [workspace.package].version".to_owned());
    }
    for (label, text) in members {
        let Ok(table) = text.parse::<toml::Table>() else {
            problems.push(format!("{label}: does not parse"));
            continue;
        };
        let inherits = table
            .get("package")
            .and_then(|package| package.get("version"))
            .and_then(|version| version.get("workspace"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        if !inherits {
            problems.push(format!(
                "{label}: [package] must inherit version.workspace = true"
            ));
        }
    }
    problems
}
