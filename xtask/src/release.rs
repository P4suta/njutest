// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Version consistency: the workspace version, the release-please manifest,
//! and every member manifest inheriting it must agree, so a release tag names
//! one version everywhere.

/// The `[workspace.package].version` of the root manifest, if it has one.
#[must_use]
pub fn workspace_version(root_manifest: &str) -> Option<String> {
    let table = root_manifest.parse::<toml::Table>().ok()?;
    table
        .get("workspace")?
        .get("package")?
        .get("version")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn manifest_version(release_manifest: &str) -> Option<String> {
    // `{ ".": "0.1.0" }` — small enough to read without a JSON crate.
    let start = release_manifest.find("\".\"")?;
    let rest = release_manifest.get(start.checked_add(3)?..)?;
    let colon = rest.find(':')?;
    let value = rest.get(colon.checked_add(1)?..)?.trim_start();
    let value = value.strip_prefix('"')?;
    let end = value.find('"')?;
    value.get(..end).map(ToOwned::to_owned)
}

/// Every inconsistency between the root manifest, the release-please
/// manifest, and the member manifests `(label, text)`.
#[must_use]
pub fn check(root_manifest: &str, release_manifest: &str, members: &[(&str, &str)]) -> Vec<String> {
    let mut problems = Vec::new();
    let workspace = workspace_version(root_manifest);
    let release = manifest_version(release_manifest);
    match (&workspace, &release) {
        (Some(workspace), Some(release)) if workspace != release => problems.push(format!(
            "Cargo.toml [workspace.package].version is {workspace} but .release-please-manifest.json says {release}"
        )),
        (None, _) => problems.push("Cargo.toml has no [workspace.package].version".to_owned()),
        (_, None) => problems.push(".release-please-manifest.json has no \".\" entry".to_owned()),
        _ => {}
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
