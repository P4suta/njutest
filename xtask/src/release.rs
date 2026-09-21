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

/// Every workspace member declared by a plain directory rather than a glob.
///
/// The globbed ones are walked from the filesystem; these are the roots beside `crates/`, and a release configuration may legitimately name one.
#[must_use]
pub fn plain_members(root_manifest: &str) -> Vec<String> {
    let Ok(table) = root_manifest.parse::<toml::Table>() else {
        return Vec::new();
    };
    table
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .map_or_else(Vec::new, |members| {
            members
                .iter()
                .filter_map(toml::Value::as_str)
                .filter(|entry| !entry.contains('*'))
                .map(ToOwned::to_owned)
                .collect()
        })
}

/// The package name each manifest declares.
#[must_use]
pub fn member_names(manifests: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (_, text) in manifests {
        let Ok(table) = text.parse::<toml::Table>() else {
            continue;
        };
        if let Some(name) = table
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
        {
            found.push(name.to_owned());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Every disagreement between `release-plz.toml` and the packages the workspace actually has.
///
/// A renamed crate leaves its old name here silently: release-plz skips a `[[package]]` it cannot find, so the settings on it — which is where `git_tag_enable` lives — stop applying, and a release train that cuts no tag looks exactly like one that had nothing to release.
#[must_use]
pub fn release_train(release_plz: &str, members: &[&str]) -> Vec<String> {
    let Ok(table) = release_plz.parse::<toml::Table>() else {
        return vec!["release-plz.toml: does not parse".to_owned()];
    };
    let mut problems = Vec::new();
    let mut tagging = Vec::new();
    let packages = table
        .get("package")
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for package in packages {
        let Some(name) = package.get("name").and_then(toml::Value::as_str) else {
            problems.push("release-plz.toml: a [[package]] has no name".to_owned());
            continue;
        };
        if !members.contains(&name) {
            problems.push(format!(
                "release-plz.toml names {name}, which is no longer a workspace member"
            ));
        }
        for included in package
            .get("changelog_include")
            .and_then(toml::Value::as_array)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter_map(toml::Value::as_str)
        {
            if !members.contains(&included) {
                problems.push(format!(
                    "release-plz.toml has {name} include {included} in its changelog, which is no longer a workspace member"
                ));
            }
        }
        if package
            .get("git_tag_enable")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
        {
            tagging.push(name.to_owned());
        }
    }
    if tagging.len() != 1 {
        problems.push(format!(
            "exactly one package cuts the tag, because every crate shares one version; {tagging:?} do"
        ));
    }
    problems
}
