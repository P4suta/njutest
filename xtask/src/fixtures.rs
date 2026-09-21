// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Conventions of the fixture projects under `fixtures/`.

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// The rule, for the failure message.
pub const RULE: &str = "A fixture is an independent cargo project: its Cargo.toml carries a \
    [workspace] table so cargo does not look upwards, its Cargo.lock is committed, its only \
    dependencies are paths inside itself or a sibling fixture it climbs to (fixtures build \
    offline against no registry), every .rs and Cargo.toml starts with the SPDX header, and \
    its README.md states what a run of it establishes in a ```fates block, and in a ```seams \
    block as well where it interposes on a seam. A directory under fixtures/ that holds no \
    Cargo.toml is a group, and holds fixtures and nothing else. See fixtures/README.md.";

/// The fence that opens the block of a README stating what a run of the fixture establishes.
pub const FATES_FENCE: &str = "```fates";

/// The fence that opens the block stating what a run established about a fixture's seams.
pub const SEAMS_FENCE: &str = "```seams";

const SPDX_HEADER: [&str; 2] = [
    "SPDX-FileCopyrightText: 2026 njutest contributors",
    "SPDX-License-Identifier: MIT OR Apache-2.0",
];

/// Why a fixture's files could not be checked.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// Walking the fixture did not reach every entry.
    #[error("walking {root}: {source}")]
    Walk {
        /// The fixture being checked.
        root: PathBuf,
        /// What stopped the walk.
        #[source]
        source: walkdir::Error,
    },
    /// A source file found by the walk could not be read.
    #[error("reading {}: {source}", path.display())]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A symbolic link would make the checked tree depend on a target outside the walk.
    #[error("{path} is a symbolic link; fixture checks never follow hidden or escaping trees")]
    Symlink {
        /// The link the fixture contains.
        path: PathBuf,
    },
    /// A fixture path could not be named exactly in a diagnostic or policy comparison.
    #[error("{} is not a UTF-8 fixture path", path.display())]
    NonUtf8Path {
        /// The path whose operating-system spelling was not UTF-8.
        path: PathBuf,
    },
    /// A directory under a group is not a fixture, so `fixtures/` would become a tree to search rather than a place to find one.
    #[error("{} is under the group {} and is not a fixture", path.display(), group.display())]
    NotAFixture {
        /// What the group holds.
        path: PathBuf,
        /// The group holding it.
        group: PathBuf,
    },
    /// The optional fixture configuration exists but is not TOML.
    #[error("reading {} as TOML: {source}", path.display())]
    Config {
        /// The configuration being parsed.
        path: PathBuf,
        /// Why TOML rejected it.
        #[source]
        source: toml::de::Error,
    },
}

/// Every fixture under `dir`, as a `/`-joined name relative to it, in sorted order.
///
/// A directory holding a `Cargo.toml` is a fixture.
/// One holding no `Cargo.toml` is a group, and every child of it is a fixture; a group that holds anything else, or a group inside a group, is a refusal, because `fixtures/` is a place to find a fixture rather than a tree to search.
///
/// # Errors
/// A directory that could not be read, a symbolic link, a name that is not UTF-8, or a group that holds something other than a fixture.
pub fn discover(dir: &Path) -> Result<Vec<String>, CheckError> {
    let mut found = Vec::new();
    for (name, path) in children(dir)? {
        if is_file(&path.join("Cargo.toml")) {
            found.push(name);
            continue;
        }
        for (inner, nested) in children(&path)? {
            if !is_file(&nested.join("Cargo.toml")) {
                return Err(CheckError::NotAFixture {
                    path: nested,
                    group: path.clone(),
                });
            }
            found.push(format!("{name}/{inner}"));
        }
    }
    found.sort();
    Ok(found)
}

/// Whether `path` is a regular file, asked of the filesystem rather than of a method that answers `false` to every question it could not ask.
fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|entry| entry.is_file())
}

/// Every subdirectory of `dir`, by name, refusing a link or a name this gate cannot spell.
fn children(dir: &Path) -> Result<Vec<(String, PathBuf)>, CheckError> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|source| CheckError::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CheckError::Read {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|source| CheckError::Read {
            path: path.clone(),
            source,
        })?;
        if kind.is_symlink() {
            return Err(CheckError::Symlink { path });
        }
        if !kind.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_non_utf8| CheckError::NonUtf8Path { path: path.clone() })?;
        found.push((name, path));
    }
    found.sort();
    Ok(found)
}

/// Every convention `dir` breaks, one line each, sorted.
///
/// # Errors
/// A directory entry or source file could not be read, so no claim is made about the part of the fixture that was hidden.
pub fn check_fixture(dir: &Path) -> Result<Vec<String>, CheckError> {
    let mut problems = Vec::new();
    let manifest_path = dir.join("Cargo.toml");
    match std::fs::read_to_string(&manifest_path) {
        Ok(text) => problems.extend(check_manifest(&text)),
        Err(_) => problems.push("Cargo.toml is missing".to_owned()),
    }
    check_lockfile(dir, &mut problems)?;
    problems.extend(check_readme(dir)?);
    for entry in WalkDir::new(dir).sort_by_file_name() {
        let entry = entry.map_err(|source| CheckError::Walk {
            root: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if entry.path_is_symlink() {
            return Err(CheckError::Symlink {
                path: path.to_path_buf(),
            });
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = relative_name(path, dir)?;
        if relative.starts_with("target/") {
            continue;
        }
        let is_source =
            path.extension().is_some_and(|ext| ext == "rs") || entry.file_name() == "Cargo.toml";
        if is_source {
            let text = std::fs::read_to_string(path).map_err(|source| CheckError::Read {
                path: path.to_path_buf(),
                source,
            })?;
            if !has_spdx_header(&text) {
                problems.push(format!("{relative}: missing the SPDX header"));
            }
        }
    }
    problems.sort();
    Ok(problems)
}

fn check_lockfile(dir: &Path, problems: &mut Vec<String>) -> Result<(), CheckError> {
    let path = dir.join("Cargo.lock");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CheckError::Symlink { path }),
        Ok(_) => {
            problems.push(
                "Cargo.lock is missing (commit it: fixtures build with --locked --offline)"
                    .to_owned(),
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            problems.push(
                "Cargo.lock is missing (commit it: fixtures build with --locked --offline)"
                    .to_owned(),
            );
            Ok(())
        }
        Err(source) => Err(CheckError::Read { path, source }),
    }
}

fn relative_name(path: &Path, dir: &Path) -> Result<String, CheckError> {
    let relative_path = match path.strip_prefix(dir) {
        Ok(relative) => relative,
        Err(_outside) => path,
    };
    let Some(relative) = relative_path.to_str() else {
        return Err(CheckError::NonUtf8Path {
            path: path.to_path_buf(),
        });
    };
    Ok(relative.replace('\\', "/"))
}

/// The README, and the ledgers of fates a fixture has to keep.
fn check_readme(dir: &Path) -> Result<Vec<String>, CheckError> {
    let Ok(readme) = std::fs::read_to_string(dir.join("README.md")) else {
        return Ok(vec![
            "README.md is missing (it is where a fixture says what it is for)".to_owned(),
        ]);
    };
    let mut problems = Vec::new();
    if !readme.contains(FATES_FENCE) {
        problems.push(format!(
            "README.md has no {FATES_FENCE} block; a fixture whose fates nothing states is one \
             a change can quietly re-decide"
        ));
    }
    if interposes(dir)? && !readme.contains(SEAMS_FENCE) {
        problems.push(format!(
            "README.md has no {SEAMS_FENCE} block, and .njutest.toml puts an interposer in \
             front of a seam; what a run establishes about that seam is a ledger for the same \
             reason the mutation fates are one, and a fixture that stated only half of what it \
             decides would let the other half move without anybody reading a diff"
        ));
    }
    Ok(problems)
}

/// Whether the fixture's configuration puts an interposer in front of anything.
fn interposes(dir: &Path) -> Result<bool, CheckError> {
    let path = dir.join(".njutest.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(CheckError::Read { path, source }),
    };
    let table = text
        .parse::<toml::Table>()
        .map_err(|source| CheckError::Config { path, source })?;
    let Some(toml::Value::Table(resources)) = table.get("resources") else {
        return Ok(false);
    };
    Ok(resources.values().any(|value| match value {
        toml::Value::Table(resource) => matches!(
            resource.get("interpose"),
            Some(toml::Value::String(named)) if !named.is_empty()
        ),
        _ => false,
    }))
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

/// Whether the path climbs out of the fixture and lands on one fixture and nothing else.
///
/// A dependency reaching a fixture beside its own is the one shape allowed to leave the directory, and how far it climbs is how deep the fixture sits under `fixtures/`.
fn is_sibling_fixture(parts: &[&str]) -> bool {
    let climbed = parts.iter().take_while(|part| **part == "..").count();
    climbed > 0
        && parts.len().checked_sub(climbed) == Some(1)
        && parts
            .get(climbed)
            .is_some_and(|last| last.starts_with("fixture-"))
}

#[cfg(all(test, unix))]
mod tests {

    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::Path;

    use super::{CheckError, relative_name};

    #[test]
    fn a_non_utf8_fixture_path_is_a_typed_refusal() {
        let path = Path::new("fixtures").join(OsString::from_vec(vec![b'b', 0xff, b'd']));
        let error = relative_name(&path, Path::new("fixtures"))
            .expect_err("the path cannot be named exactly");
        let CheckError::NonUtf8Path { path: refused } = error else {
            panic!("the wrong fixture error was returned: {error}");
        };
        assert_eq!(refused, path);
    }
}
