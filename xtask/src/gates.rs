// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gates applied to this repository: each one reads the tree, hands it to the pure checker of its module, and renders the answer.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Arguments;
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

use crate::{
    deps, devgates, engineaudit, fixtures, lints as lint_scan, proofaudit, release, reportdiff,
    shapes,
};

/// The root of this workspace, resolved from the xtask manifest at compile time so the gates do not depend on the working directory.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

/// A gate's failure, rendered for a person.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct GateFailure(pub String);

fn append(output: &mut String, arguments: Arguments<'_>) {
    output.push_str(&arguments.to_string());
}

fn line(output: &mut String, arguments: Arguments<'_>) {
    append(output, arguments);
    output.push('\n');
}

/// Every Rust file the repository commits, tests included.
///
/// # Errors
/// Any directory entry cannot be read; an incomplete source set proves no gate.
pub fn all_sources(root: &Path) -> Result<Vec<PathBuf>, GateFailure> {
    validate_closed_source_inventory(root)?;
    let mut files = Vec::new();
    for base in SOURCE_ROOTS {
        for entry in WalkDir::new(root.join(base)).sort_by_file_name() {
            let entry = walked(entry)?;
            let path = entry.path();
            let relative = relative_slash(root, path)?;
            if entry.file_type().is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
                && !relative.split('/').any(|part| part == "target")
            {
                files.push(path.to_path_buf());
            }
        }
    }
    Ok(files)
}

const SOURCE_ROOTS: [&str; 4] = ["compiler-surfaces", "crates", "xtask", "fuzz"];

fn validate_closed_source_inventory(root: &Path) -> Result<(), GateFailure> {
    let entries = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() != 1 {
                return true;
            }
            entry
                .file_name()
                .as_encoded_bytes()
                .first()
                .is_none_or(|byte| *byte != b'.')
                && entry.file_name() != std::ffi::OsStr::new("target")
                && !SOURCE_ROOTS
                    .iter()
                    .any(|source| entry.file_name() == std::ffi::OsStr::new(source))
        });
    for entry in entries {
        let entry = walked(entry)?;
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "rs")
        {
            continue;
        }
        let relative = relative_slash(root, entry.path())?;
        if !relative.starts_with("fixtures/") {
            return Err(GateFailure(format!(
                "walking the repository: {relative} is Rust outside the four scanned source roots; only fixtures/ is an explicit unproved input corpus"
            )));
        }
    }
    Ok(())
}

const ALLOWED_PATH_REDIRECTS: [(&str, &str); 8] = [
    (
        "compiler-surfaces/src/bin/njutest.rs",
        "../../../crates/njutest/src/lib.rs",
    ),
    (
        "compiler-surfaces/src/bin/njutest.rs",
        "../../../crates/njutest/src/bin/cargo-njutest/main.rs",
    ),
    (
        "compiler-surfaces/src/bin/njutest.rs",
        "../../../crates/njutest/src/main.rs",
    ),
    (
        "compiler-surfaces/src/bin/rust_mutants_cli.rs",
        "../../../crates/rust-mutants-cli/src/lib.rs",
    ),
    (
        "compiler-surfaces/src/bin/rust_mutants_cli.rs",
        "../../../crates/rust-mutants-cli/src/bin/cargo-rust-mutants/main.rs",
    ),
    (
        "compiler-surfaces/src/bin/rust_mutants_cli.rs",
        "../../../crates/rust-mutants-cli/src/main.rs",
    ),
    (
        "compiler-surfaces/src/bin/xtask.rs",
        "../../../xtask/src/lib.rs",
    ),
    (
        "compiler-surfaces/src/bin/xtask.rs",
        "../../../xtask/src/main.rs",
    ),
];

fn walked(
    entry: Result<walkdir::DirEntry, walkdir::Error>,
) -> Result<walkdir::DirEntry, GateFailure> {
    let entry = entry.map_err(|error| GateFailure(format!("walking the repository: {error}")))?;
    if entry.path_is_symlink() {
        return Err(GateFailure(format!(
            "walking the repository: {} is a symbolic link; a gate does not follow a name that \
             can hide or escape the tree it proves",
            entry.path().display()
        )));
    }
    Ok(entry)
}

#[derive(Debug)]
struct ProcMacroPackage {
    name: String,
    source_root: PathBuf,
    target: PathBuf,
}

fn source_universe(
    root: &Path,
    files: &[PathBuf],
    sources: &[(String, String, String)],
) -> Result<Vec<lint_scan::Finding>, GateFailure> {
    let mut labels = BTreeSet::new();
    let mut canonical_labels = BTreeMap::new();
    for path in files {
        let label = relative_slash(root, path)?;
        let canonical = std::fs::canonicalize(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        labels.insert(label.clone());
        canonical_labels.insert(canonical, label);
    }

    let proc_macros = cargo_source_universe(root, &canonical_labels)?;
    let mut found = Vec::new();
    for (_scope, file, source) in sources {
        for redirect in lint_scan::source_redirects(source)
            .map_err(|error| GateFailure(format!("{file}: {error}")))?
        {
            let line = match &redirect {
                lint_scan::SourceRedirect::Include { line, .. }
                | lint_scan::SourceRedirect::Path { line, .. }
                | lint_scan::SourceRedirect::Opaque { line } => *line,
            };
            let accepted = match redirect {
                lint_scan::SourceRedirect::Include {
                    target: Some(target),
                    ..
                } => {
                    support_include(&target)
                        && resolve_redirect(file, &target)
                            .is_some_and(|path| labels.contains(&path))
                }
                lint_scan::SourceRedirect::Path {
                    target: Some(target),
                    ..
                } => {
                    ALLOWED_PATH_REDIRECTS.contains(&(file.as_str(), target.as_str()))
                        && resolve_redirect(file, &target)
                            .is_some_and(|path| labels.contains(&path))
                }
                lint_scan::SourceRedirect::Include { target: None, .. }
                | lint_scan::SourceRedirect::Path { target: None, .. }
                | lint_scan::SourceRedirect::Opaque { .. } => false,
            };
            if !accepted {
                found.push(lint_scan::Finding {
                    kind: lint_scan::Kind::OpaqueMacroSyntax,
                    file: file.clone(),
                    line,
                });
            }
        }
    }

    validate_proc_macros(&proc_macros, &canonical_labels, sources, &mut found)?;
    Ok(found)
}

fn support_include(target: &str) -> bool {
    let path = Path::new(target);
    !path.is_absolute()
        && path.extension().is_some_and(|extension| extension == "rs")
        && matches!(
            path.components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(support), Component::Normal(_)] if *support == std::ffi::OsStr::new("support")
        )
}

fn resolve_redirect(file: &str, target: &str) -> Option<String> {
    let target = Path::new(target);
    if target.is_absolute() {
        return None;
    }
    let parent = Path::new(file).parent()?;
    let mut normalized = PathBuf::new();
    for component in parent.join(target).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    normalized
        .to_str()
        .map(|path| path.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn cargo_source_universe(
    root: &Path,
    sources: &BTreeMap<PathBuf, String>,
) -> Result<Vec<ProcMacroPackage>, GateFailure> {
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|error| GateFailure(format!("{}: {error}", root.display())))?;
    preflight_cargo_manifests(&canonical_root)?;
    validate_dependency_proc_macro_inventory(&canonical_root)?;
    let mut pending = VecDeque::from([root.join("Cargo.toml"), root.join("fuzz/Cargo.toml")]);
    let mut requested = BTreeSet::new();
    let mut packages = BTreeSet::new();
    let mut proc_macros = Vec::new();

    while let Some(manifest) = pending.pop_front() {
        let manifest = std::fs::canonicalize(&manifest)
            .map_err(|error| GateFailure(format!("{}: {error}", manifest.display())))?;
        if !requested.insert(manifest.clone()) {
            continue;
        }
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&manifest)
            .no_deps()
            .exec()
            .map_err(|error| {
                GateFailure(format!(
                    "cargo metadata for {}: {error}",
                    manifest.display()
                ))
            })?;
        let mut newly_seen = Vec::new();
        for package in metadata.workspace_packages() {
            let package_manifest = std::fs::canonicalize(package.manifest_path.as_std_path())
                .map_err(|error| {
                    GateFailure(format!("{}: {error}", package.manifest_path.as_str()))
                })?;
            if !packages.insert(package_manifest.clone()) {
                continue;
            }
            proc_macros.extend(cargo_package_sources(&canonical_root, sources, package)?);
            newly_seen.push(package);
        }
        for package in newly_seen {
            for dependency in &package.dependencies {
                let Some(path) = &dependency.path else {
                    continue;
                };
                let dependency_directory =
                    std::fs::canonicalize(path.as_std_path()).map_err(|error| {
                        GateFailure(format!("local dependency {}: {error}", path.as_str()))
                    })?;
                if !inside_source_roots(&canonical_root, &dependency_directory) {
                    return Err(GateFailure(format!(
                        "lints: local dependency {} is outside the four scanned source roots",
                        path.as_str()
                    )));
                }
                let dependency_manifest = dependency_directory.join("Cargo.toml");
                let dependency_manifest =
                    std::fs::canonicalize(&dependency_manifest).map_err(|error| {
                        GateFailure(format!("{}: {error}", dependency_manifest.display()))
                    })?;
                if !packages.contains(&dependency_manifest) {
                    pending.push_back(dependency_manifest);
                }
            }
        }
    }
    proc_macros.sort_by(|left, right| left.name.cmp(&right.name));
    proc_macros.dedup_by(|left, right| left.name == right.name && left.target == right.target);
    Ok(proc_macros)
}

const PROC_MACRO_INVENTORY: &str = "xtask/proc_macro_inventory.txt";

fn validate_dependency_proc_macro_inventory(root: &Path) -> Result<(), GateFailure> {
    let inventory_path = root.join(PROC_MACRO_INVENTORY);
    let expected = expected_proc_macro_dependencies(&inventory_path)?;

    for (graph, manifest, lockfile) in [
        ("root", root.join("Cargo.toml"), root.join("Cargo.lock")),
        (
            "fuzz",
            root.join("fuzz/Cargo.toml"),
            root.join("fuzz/Cargo.lock"),
        ),
    ] {
        let locked = lock_packages(&lockfile)?;
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&manifest)
            .features(cargo_metadata::CargoOpt::AllFeatures)
            .other_options(vec!["--locked".to_owned()])
            .exec()
            .map_err(|error| {
                GateFailure(format!(
                    "cargo metadata --locked for {}: {error}",
                    manifest.display()
                ))
            })?;
        let observed = metadata
            .packages
            .iter()
            .filter(|package| {
                package
                    .targets
                    .iter()
                    .any(cargo_metadata::Target::is_proc_macro)
            })
            .map(proc_macro_dependency_key)
            .collect::<BTreeSet<_>>();
        let absent_from_lock = observed.difference(&locked).cloned().collect::<Vec<_>>();
        if !absent_from_lock.is_empty() {
            return Err(GateFailure(format!(
                "lints: {graph} metadata reports procedural macros absent from {}: {absent_from_lock:?}",
                lockfile.display()
            )));
        }
        let declared = expected.get(graph).ok_or_else(|| {
            GateFailure(format!(
                "lints: {PROC_MACRO_INVENTORY} has no {graph} graph"
            ))
        })?;
        if observed != *declared {
            let added = observed.difference(declared).cloned().collect::<Vec<_>>();
            let removed = declared.difference(&observed).cloned().collect::<Vec<_>>();
            return Err(GateFailure(format!(
                "lints: {graph} locked dependency procedural-macro inventory drifted; added {added:?}, removed {removed:?}"
            )));
        }
    }
    Ok(())
}

fn expected_proc_macro_dependencies(
    path: &Path,
) -> Result<BTreeMap<String, BTreeSet<String>>, GateFailure> {
    let inventory = std::fs::read_to_string(path)
        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
    let mut expected = BTreeMap::from([
        ("root".to_owned(), BTreeSet::new()),
        ("fuzz".to_owned(), BTreeSet::new()),
    ]);
    for (line_index, line) in inventory.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let [graph, package, version, source] = fields.as_slice() else {
            return Err(GateFailure(format!(
                "{}:{}: expected graph, package, version, and source",
                path.display(),
                line_index.saturating_add(1)
            )));
        };
        let Some(packages) = expected.get_mut(*graph) else {
            return Err(GateFailure(format!(
                "{}:{}: unknown Cargo graph {graph:?}",
                path.display(),
                line_index.saturating_add(1)
            )));
        };
        let entry = format!("{package} {version} {source}");
        if !packages.insert(entry.clone()) {
            return Err(GateFailure(format!(
                "{}:{}: duplicate procedural-macro package {entry}",
                path.display(),
                line_index.saturating_add(1)
            )));
        }
    }
    Ok(expected)
}

fn proc_macro_dependency_key(package: &cargo_metadata::Package) -> String {
    let source = package
        .source
        .as_ref()
        .map_or_else(|| "path".to_owned(), ToString::to_string);
    format!("{} {} {source}", package.name, package.version)
}

fn lock_packages(path: &Path) -> Result<BTreeSet<String>, GateFailure> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
    let lock = toml::from_str::<toml::Value>(&source)
        .map_err(|error| GateFailure(format!("parsing {}: {error}", path.display())))?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| GateFailure(format!("{} has no package array", path.display())))?;
    let mut found = BTreeSet::new();
    for package in packages {
        let table = package
            .as_table()
            .ok_or_else(|| GateFailure(format!("{} has a non-table package", path.display())))?;
        let name = table
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                GateFailure(format!("{} has a package without a name", path.display()))
            })?;
        let version = table
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                GateFailure(format!(
                    "{} has a package without a version",
                    path.display()
                ))
            })?;
        let source = table
            .get("source")
            .map_or(Some("path"), toml::Value::as_str)
            .ok_or_else(|| {
                GateFailure(format!("{} has a non-text package source", path.display()))
            })?;
        if source.starts_with("registry+") {
            let checksum = table
                .get("checksum")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| {
                    GateFailure(format!(
                        "{} has registry package {name} {version} without a checksum",
                        path.display()
                    ))
                })?;
            if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(GateFailure(format!(
                    "{} has invalid checksum for registry package {name} {version}",
                    path.display()
                )));
            }
        }
        found.insert(format!("{name} {version} {source}"));
    }
    Ok(found)
}

fn cargo_package_sources(
    root: &Path,
    sources: &BTreeMap<PathBuf, String>,
    package: &cargo_metadata::Package,
) -> Result<Vec<ProcMacroPackage>, GateFailure> {
    let package_manifest = std::fs::canonicalize(package.manifest_path.as_std_path())
        .map_err(|error| GateFailure(format!("{}: {error}", package.manifest_path.as_str())))?;
    let directory = package_manifest.parent().ok_or_else(|| {
        GateFailure(format!(
            "{} has no package directory",
            package.manifest_path
        ))
    })?;
    if !inside_source_roots(root, directory) {
        return Err(GateFailure(format!(
            "lints: local package {} at {} is outside the four scanned source roots",
            package.name, package.manifest_path
        )));
    }
    let mut proc_macros = Vec::new();
    for target in &package.targets {
        let source = std::fs::canonicalize(target.src_path.as_std_path()).map_err(|error| {
            GateFailure(format!("{} target {}: {error}", package.name, target.name))
        })?;
        if !sources.contains_key(&source) {
            return Err(GateFailure(format!(
                "lints: Cargo target {}:{} at {} is not an exact .rs member of the scanned source universe",
                package.name, target.name, target.src_path
            )));
        }
        if target.is_proc_macro() {
            proc_macros.push(ProcMacroPackage {
                name: package.name.to_string(),
                source_root: directory.join("src"),
                target: source,
            });
        }
    }
    Ok(proc_macros)
}

fn preflight_cargo_manifests(root: &Path) -> Result<(), GateFailure> {
    preflight_cargo_manifest(root, &root.join("Cargo.toml"))?;
    for base in SOURCE_ROOTS {
        for entry in WalkDir::new(root.join(base)).sort_by_file_name() {
            let entry = walked(entry)?;
            if !entry.file_type().is_file()
                || entry.file_name() != std::ffi::OsStr::new("Cargo.toml")
                || entry
                    .path()
                    .components()
                    .any(|component| matches!(component, Component::Normal(part) if part == std::ffi::OsStr::new("target")))
            {
                continue;
            }
            preflight_cargo_manifest(root, entry.path())?;
        }
    }
    Ok(())
}

fn preflight_cargo_manifest(root: &Path, path: &Path) -> Result<(), GateFailure> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
    let manifest = toml::from_str::<toml::Value>(&source).map_err(|error| {
        GateFailure(format!("parsing {} before Cargo: {error}", path.display()))
    })?;
    validate_manifest_paths(root, path, &manifest)
}

fn validate_manifest_paths(
    root: &Path,
    manifest_path: &Path,
    manifest: &toml::Value,
) -> Result<(), GateFailure> {
    let Some(top) = manifest.as_table() else {
        return Ok(());
    };
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = top.get(key).and_then(toml::Value::as_table) {
            validate_dependency_paths(root, manifest_path, table)?;
        }
    }
    if let Some(workspace) = top.get("workspace").and_then(toml::Value::as_table) {
        if let Some(dependencies) = workspace
            .get("dependencies")
            .and_then(toml::Value::as_table)
        {
            validate_dependency_paths(root, manifest_path, dependencies)?;
        }
        for key in ["members", "default-members"] {
            if let Some(members) = workspace.get(key).and_then(toml::Value::as_array) {
                for member in members.iter().filter_map(toml::Value::as_str) {
                    validate_declared_path(root, manifest_path, member, false)?;
                }
            }
        }
    }
    if let Some(package) = top.get("package").and_then(toml::Value::as_table)
        && let Some(workspace) = package.get("workspace").and_then(toml::Value::as_str)
    {
        validate_declared_path(root, manifest_path, workspace, true)?;
    }
    if let Some(targets) = top.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(table) = target.get(key).and_then(toml::Value::as_table) {
                    validate_dependency_paths(root, manifest_path, table)?;
                }
            }
        }
    }
    if let Some(patches) = top.get("patch").and_then(toml::Value::as_table) {
        for patch in patches.values().filter_map(toml::Value::as_table) {
            validate_dependency_paths(root, manifest_path, patch)?;
        }
    }
    if let Some(replacements) = top.get("replace").and_then(toml::Value::as_table) {
        validate_dependency_paths(root, manifest_path, replacements)?;
    }
    Ok(())
}

fn validate_dependency_paths(
    root: &Path,
    manifest_path: &Path,
    dependencies: &toml::value::Table,
) -> Result<(), GateFailure> {
    for dependency in dependencies.values().filter_map(toml::Value::as_table) {
        if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
            validate_declared_path(root, manifest_path, path, true)?;
        }
    }
    Ok(())
}

fn validate_declared_path(
    root: &Path,
    manifest_path: &Path,
    declared: &str,
    canonical: bool,
) -> Result<(), GateFailure> {
    let directory = manifest_path.parent().ok_or_else(|| {
        GateFailure(format!(
            "{} has no parent directory",
            manifest_path.display()
        ))
    })?;
    let candidate = repository_path(root, directory, declared).ok_or_else(|| {
        GateFailure(format!(
            "lints: {} declares path {declared:?} outside the repository",
            manifest_path.display()
        ))
    })?;
    if !inside_source_roots(root, &candidate) {
        return Err(GateFailure(format!(
            "lints: {} declares path {declared:?} outside the four scanned source roots",
            manifest_path.display()
        )));
    }
    if canonical {
        let resolved = std::fs::canonicalize(&candidate)
            .map_err(|error| GateFailure(format!("{}: {error}", candidate.display())))?;
        if !inside_source_roots(root, &resolved) {
            return Err(GateFailure(format!(
                "lints: {} declares path {declared:?} that resolves outside the four scanned source roots",
                manifest_path.display()
            )));
        }
    }
    Ok(())
}

fn repository_path(root: &Path, directory: &Path, declared: &str) -> Option<PathBuf> {
    let declared = Path::new(declared);
    if declared.is_absolute() {
        return None;
    }
    let relative_directory = match directory.strip_prefix(root) {
        Ok(relative) => relative,
        Err(_outside_repository) => return None,
    };
    let mut normalized = PathBuf::new();
    for component in relative_directory.join(declared).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(root.join(normalized))
}

fn inside_source_roots(root: &Path, path: &Path) -> bool {
    SOURCE_ROOTS.iter().any(|base| path.starts_with(root.join(base)))
        && !path.components().any(|component| {
            matches!(component, Component::Normal(part) if part == std::ffi::OsStr::new("target"))
        })
}

fn validate_proc_macros(
    packages: &[ProcMacroPackage],
    labels: &BTreeMap<PathBuf, String>,
    sources: &[(String, String, String)],
    found: &mut Vec<lint_scan::Finding>,
) -> Result<(), GateFailure> {
    if !matches!(packages, [package] if package.name == "njutest-macros") {
        return Err(GateFailure(format!(
            "lints: the workspace procedural-macro package inventory drifted: {:?}",
            packages
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>()
        )));
    }
    let package = packages
        .first()
        .ok_or_else(|| GateFailure("lints: njutest-macros is absent".to_owned()))?;
    let target_label = labels.get(&package.target).ok_or_else(|| {
        GateFailure(format!(
            "lints: {} procedural-macro target escaped the source labels",
            package.name
        ))
    })?;
    if !sources
        .iter()
        .any(|(_scope, file, _source)| file == target_label)
    {
        return Err(GateFailure(format!("lints: {target_label} was not read")));
    }
    let source_root = std::fs::canonicalize(&package.source_root)
        .map_err(|error| GateFailure(format!("{}: {error}", package.source_root.display())))?;
    let mut exports = Vec::new();
    for (_scope, file, source) in sources {
        let absolute = labels
            .iter()
            .find_map(|(path, label)| (label == file).then_some(path));
        if !absolute.is_some_and(|path| path.starts_with(&source_root)) {
            continue;
        }
        exports.extend(
            lint_scan::proc_macro_exports(source)
                .map_err(|error| GateFailure(format!("{file}: {error}")))?,
        );
        found.extend(
            lint_scan::opaque_proc_macro_synthesis(file, source)
                .map_err(|error| GateFailure(format!("{file}: {error}")))?,
        );
    }
    exports.sort();
    exports.dedup();
    let observed = exports
        .iter()
        .map(|export| (export.kind, export.name.as_str()))
        .collect::<Vec<_>>();
    let expected = [("derive", "AllVariants")];
    if observed != expected {
        return Err(GateFailure(format!(
            "lints: njutest-macros exports {observed:?}, expected exactly {expected:?}"
        )));
    }
    Ok(())
}

/// Whether a waiving file and a declaring file are compiled as one crate, which is what decides whether an arm could have been left out.
///
/// An integration test is its own crate, so an enum that says it may grow forces a place to be left for the growth there even though the same match inside the declaring crate would not need one.
fn shares_a_crate(waiving: &str, declaring: &str) -> bool {
    compiled_as(waiving) == compiled_as(declaring)
}

/// What a file is compiled into: a package's library, or the one-file crate a test or a benchmark is.
fn compiled_as(path: &str) -> String {
    path.split_once("/src/")
        .map_or_else(|| path.to_owned(), |(package, _rest)| package.to_owned())
}

/// A second opinion on every catch-all the ledger still waives, taken from the shape of its body.
///
/// This never refuses anything.
/// The ledger is a reviewed list and the review is a person's; what a machine can add is a reading that was not derived from theirs, so the two can disagree.
/// An audit that shares the implementation it audits agrees with it for free (ADR 0023), which is why this reads only the syntax and says so in every line it prints.
///
/// # Errors
/// A file the ledger names that cannot be read.
pub fn waivers(root: &Path) -> Result<String, GateFailure> {
    let files = all_sources(root)?;
    let (ours, open) = sets(root, &files)?;
    let path = root.join("xtask/wildcard_allowlist.txt");
    let ledger = std::fs::read_to_string(&path)
        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
    let mut said = String::new();
    let mut counted: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut entries: Vec<&'static str> = Vec::new();
    for entry in ledger
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (file, item, over, expected_arms) = parted(entry)?;
        let source = std::fs::read_to_string(root.join(file))
            .map_err(|error| GateFailure(format!("{file}: {error}")))?;
        let mut shaped = shapes::shapes(&source, &ours)
            .map_err(|error| GateFailure(format!("{file}: {error}")))?;
        let lines: Vec<usize> = lint_scan::wildcards_over(&source, &ours)
            .map_err(|error| GateFailure(format!("{file}: {error}")))?
            .into_iter()
            .filter(|one| one.item == item && one.over == over)
            .map(|one| one.line)
            .collect();
        if lines.len() != expected_arms {
            return Err(GateFailure(format!(
                "lints: {entry:?} declares {expected_arms} wildcard arm(s), but the source has {}",
                lines.len()
            )));
        }
        let waived = lines.first().and_then(|number| shaped.remove(number));
        let forced = waived
            .as_ref()
            .and_then(|one| open.get(&one.over))
            .is_some_and(|declared| !shares_a_crate(file, declared));
        let (word, hint) = match (&waived, forced) {
            (Some(_), true) => (
                "required",
                "not a shape at all: the enum says it may grow and is declared in another \
                 crate, so the compiler will not let this arm be left out. Nobody can take \
                 this line out of the ledger by editing the code it points at."
                    .to_owned(),
            ),
            (Some(one), false) => (one.shape.word(), one.shape.hint().to_owned()),
            (None, _) => (
                "gone",
                "nothing to read: no arm in that item absorbs that set now, and this is \
                 saying so rather than guessing."
                    .to_owned(),
            ),
        };
        entries.push(word);
        line(&mut said, format_args!("{entry}\t{word}\t{hint}"));
    }
    for word in &entries {
        counted.insert(word, entries.iter().filter(|one| *one == word).count());
    }
    let tally = counted
        .iter()
        .map(|(word, how_many)| format!("{how_many} {word}"))
        .collect::<Vec<_>>()
        .join(", ");
    line(
        &mut said,
        format_args!(
            "waivers: {tally}, read from the syntax and from nothing else. This is a \
         second reading, not a verdict, and it refuses nothing: where it disagrees \
         with the ledger, the disagreement is the thing worth looking at."
        ),
    );
    line(
        &mut said,
        format_args!(
            "waivers: a line counted `required` is one the compiler demands and the gate \
         asked a waiver for anyway, which is the gate to fix rather than the ledger."
        ),
    );
    Ok(said)
}

/// Refuses lossy Rust shapes and comments that are not documentation, anywhere in the repository's own code.
///
/// # Errors
/// Every finding, one per line, or a file that could not be read or parsed.
pub fn lints(root: &Path) -> Result<String, GateFailure> {
    let planted = lint_sentinels()?;
    let scanned = lints_scanned(root)?;
    Ok(format!(
        "{scanned}. Before any of it was read, {planted} planted shapes were found, each as the \
         kind it was planted as"
    ))
}

/// The lint scan of `root` alone, for a synthetic tree whose gate has already been shown its planted shapes.
///
/// # Errors
/// Every finding, one per line, or a file that could not be read or parsed.
pub fn lints_scanned(root: &Path) -> Result<String, GateFailure> {
    let (files, found) = lint_findings(root)?;
    if found.is_empty() {
        let kinds = lint_scan::Kind::ALL
            .iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(format!(
            "lints: {files} files carry none of {} prohibited Rust shapes ({kinds}). {} catch-all \
             waiver(s) are \
             still standing; `cargo xtask waivers` reads each of them a second time",
            lint_scan::Kind::ALL.len(),
            waived_lines(root)?
        ));
    }
    let mut report = String::new();
    for finding in &found {
        line(&mut report, format_args!("{finding}"));
    }
    Err(GateFailure(report.trim_end().to_owned()))
}

/// How many planted shapes every lint kind was found in, before any real file is read.
///
/// # Errors
/// The first kind a planted shape of it was not found as, which means the scan is blind to that shape and its silence about the tree says nothing.
pub fn lint_sentinels() -> Result<usize, GateFailure> {
    let mut found = 0_usize;
    for kind in lint_scan::Kind::ALL {
        let shapes = lint_sighted(*kind, kind.planted())?;
        found = found.checked_add(shapes).ok_or_else(|| {
            GateFailure("lints: more planted shapes than a count can hold".to_owned())
        })?;
    }
    Ok(found)
}

/// How many shapes of `planted` the lint scan found as `kind`, which is all of them or an error.
///
/// # Errors
/// The first shape the scan did not find as `kind`, or planted text that does not parse.
pub fn lint_sighted(kind: lint_scan::Kind, planted: &str) -> Result<usize, GateFailure> {
    let kinds = |found: Vec<lint_scan::Finding>| -> Vec<lint_scan::Kind> {
        found.into_iter().map(|finding| finding.kind).collect()
    };
    sighted(
        &Planted {
            gate: "lints",
            file: format!("xtask/sentinels/lints/{}.planted", kind.label()),
            label: kind.label(),
            text: planted,
        },
        &kind,
        |path, text| {
            lint_scan::scan_source(path, text)
                .map(kinds)
                .map_err(|error| GateFailure(error.to_string()))
        },
        |root| lint_findings(root).map(|(_files, found)| kinds(found)),
    )
}

/// One planted file: which gate it is for, where it lives, and what it holds.
struct Planted<'a> {
    gate: &'a str,
    file: String,
    label: &'a str,
    text: &'a str,
}

/// How many shapes of a planted file a scan found as `kind`, which is all of them or an error.
///
/// A one-file shape is read by `per_file` exactly as the gate reads a file of the tree; a tree shape is laid over a synthetic repository and read by `whole`, which is the gate's own scan.
fn sighted<K: PartialEq>(
    planted: &Planted<'_>,
    kind: &K,
    per_file: impl Fn(&str, &str) -> Result<Vec<K>, GateFailure>,
    whole: impl Fn(&Path) -> Result<Vec<K>, GateFailure>,
) -> Result<usize, GateFailure> {
    let Planted {
        gate,
        ref file,
        label,
        text,
    } = *planted;
    let shapes = crate::sentinel::shapes(text)
        .map_err(|error| GateFailure(format!("{gate}: {file}: {error}")))?;
    for shape in &shapes {
        let name = shape.name();
        let found = match shape {
            crate::sentinel::Shape::Source { path, text, .. } => {
                per_file(path, text).map_err(|GateFailure(error)| {
                    GateFailure(format!(
                        "{gate}: planted shape `{name}` of {label}: {error}"
                    ))
                })?
            }
            crate::sentinel::Shape::Tree { files, .. } => {
                let root = tempfile::tempdir().map_err(|error| {
                    GateFailure(format!("{gate}: a directory to plant {label} in: {error}"))
                })?;
                crate::sentinel::plant(root.path(), files).map_err(|error| {
                    GateFailure(format!(
                        "{gate}: planting shape `{name}` of {label}: {error}"
                    ))
                })?;
                whole(root.path())?
            }
        };
        if !found.contains(kind) {
            return Err(GateFailure(format!(
                "{gate}: the {label} check is blind. Its planted shape `{name}` ({file}) was not \
                 found as {label}, so a tree that carries none says nothing about whether this \
                 repository does. Nothing this gate would have said is believed until the \
                 planted shape is found again."
            )));
        }
    }
    Ok(shapes.len())
}

/// How many files the scan read under `root`, and every finding in them.
///
/// # Errors
/// A file that could not be read or parsed.
fn lint_findings(root: &Path) -> Result<(usize, Vec<lint_scan::Finding>), GateFailure> {
    let files = all_sources(root)?;
    let mut found = Vec::new();
    let mut sources = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        found.extend(
            lint_scan::scan_source(&label, &source)
                .map_err(|error| GateFailure(format!("{label}: {error}")))?,
        );
        sources.push((compiled_as(&label), label, source));
    }
    let crates: Vec<(String, String, String)> = sources
        .iter()
        .map(|(_compiled, file, source)| (lint_scan::crate_of(file), file.clone(), source.clone()))
        .collect();
    found.extend(source_universe(root, &files, &sources)?);
    found.extend(
        lint_scan::open_and_closed_across(
            sources
                .iter()
                .map(|(scope, file, source)| (scope.as_str(), file.as_str(), source.as_str())),
        )
        .map_err(|error| GateFailure(format!("cross-file Rust declarations: {error}")))?,
    );
    found.extend(
        lint_scan::manual_variant_lists_across(
            crates
                .iter()
                .map(|(scope, file, source)| (scope.as_str(), file.as_str(), source.as_str())),
        )
        .map_err(|error| GateFailure(format!("cross-file variant lists: {error}")))?,
    );
    found.extend(loose_layouts(root, &files)?);
    found.extend(wildcards(root, &files)?);
    found.sort();
    found.dedup();
    Ok((files.len(), found))
}

/// Every exported constant that more than one module joins onto a path for itself.
///
/// This is the shape the report layout had: a `pub const` spelling a structure,
/// joined in six places and in the tests, so the configuration could not own it and moving it meant moving all of them.
/// One module joining its own constant is not that — it is a name it happens to have written down — and a document type or a URL is not a path at all, which is why this counts the Every catch-all over a set this repository closes, over the whole tree at once.
///
/// Two passes, because whether an arm may catch everything depends on who owns the enum, and that is a fact about the workspace rather than about the file being read.
/// A foreign enum keeps its catch-all: the values of `syn::Expr` are not ours to list, so an arm that stands for the rest is the handling.
fn wildcards(root: &Path, files: &[PathBuf]) -> Result<Vec<lint_scan::Finding>, GateFailure> {
    let (ours, open) = sets(root, files)?;
    let mut standing: Vec<Waived> = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        let mut grouped: BTreeMap<(String, String), Vec<lint_scan::Wildcard>> = BTreeMap::new();
        for one in lint_scan::wildcards_over(&source, &ours)
            .map_err(|error| GateFailure(format!("{label}: {error}")))?
        {
            if open
                .get(&one.over)
                .is_some_and(|declared| !shares_a_crate(&label, declared))
            {
                continue;
            }
            grouped
                .entry((one.item.clone(), one.over.clone()))
                .or_default()
                .push(one);
        }
        for arms in grouped.into_values() {
            let Some(first) = arms.first() else {
                continue;
            };
            standing.push(Waived {
                name: first.key(&label, arms.len()),
                file: label.clone(),
                line: first.line,
            });
        }
    }
    standing.sort();
    ratcheted(root, &standing)
}

/// The enums this repository declares, and which of those say they may grow, by the file that declared them.
///
/// Two answers from one read of the tree, because a caller that needs the second always needs the first and two walks would be two chances to disagree about what an enum of ours is.
///
/// # Errors
/// A file that cannot be read.
fn sets(
    root: &Path,
    files: &[PathBuf],
) -> Result<(Vec<String>, BTreeMap<String, String>), GateFailure> {
    let mut ours: Vec<String> = Vec::new();
    let mut open: BTreeMap<String, String> = BTreeMap::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        ours.extend(
            lint_scan::declared_enums(&source)
                .map_err(|error| GateFailure(format!("{label}: {error}")))?,
        );
        for name in lint_scan::open_enums(&source)
            .map_err(|error| GateFailure(format!("{label}: {error}")))?
        {
            open.insert(name, label.clone());
        }
    }
    ours.sort();
    ours.dedup();
    Ok((ours, open))
}

/// The file, the item and the set a ledger name is made of.
///
/// A name carries no line, which is the point of it, so the second reading finds the arms for itself rather than being handed a coordinate that may by now be pointing at something else.
fn parted(entry: &str) -> Result<(&str, &str, &str, usize), GateFailure> {
    let Some((place, rest)) = entry.split_once(" over ") else {
        return Err(GateFailure(format!(
            "lints: malformed wildcard ledger entry {entry:?}"
        )));
    };
    let Some((over, how_many)) = rest.split_once(", ") else {
        return Err(GateFailure(format!(
            "lints: malformed wildcard ledger entry {entry:?}"
        )));
    };
    let Some(how_many) = how_many.strip_suffix(" arm(s)") else {
        return Err(GateFailure(format!(
            "lints: malformed wildcard ledger count in {entry:?}"
        )));
    };
    let how_many = how_many.parse::<usize>().map_err(|error| {
        GateFailure(format!(
            "lints: malformed wildcard ledger count in {entry:?}: {error}"
        ))
    })?;
    let (file, item) = match place.split_once("::") {
        Some(parts) => parts,
        None => (place, ""),
    };
    Ok((file, item, over, how_many))
}

/// How many waivers the catch-all ledger still carries, held to the ceiling beside it.
///
/// In the pass line because a number somebody sees every run is a number they notice moving, and a file of forty-four that nobody could shorten was a file nobody opened.
/// Held to `xtask/waiver_ceiling.txt` because noticing is not holding: the ledger's header has always said it may shrink and never grow, and until now the count was printed and compared against nothing, so a waiver could be granted by the same hand that wrote the code wanting one.
///
/// # Errors
/// Either file cannot be read, or the ledger has grown past the ceiling.
fn waived_lines(root: &Path) -> Result<usize, GateFailure> {
    let how_many = counted(root, "xtask/wildcard_allowlist.txt")?.len();
    let ceiling = counted(root, "xtask/waiver_ceiling.txt")?;
    let [written] = ceiling.as_slice() else {
        return Err(GateFailure(
            "lints: xtask/waiver_ceiling.txt holds one number and nothing else.".to_owned(),
        ));
    };
    let Ok(most) = written.parse::<usize>() else {
        return Err(GateFailure(format!(
            "lints: xtask/waiver_ceiling.txt holds {written}, which is not a number."
        )));
    };
    if how_many > most {
        return Err(GateFailure(format!(
            "lints: the catch-all ledger carries {how_many} waiver(s) and \
             xtask/waiver_ceiling.txt allows {most}. That file may shrink and never \
             grow, so a new waiver is a number going up in a file of its own — which \
             is the review the ledger exists to ask for, and the thing to argue for \
             in the change rather than notice in a graph later."
        )));
    }
    Ok(how_many)
}

/// The one number a ceiling file holds.
///
/// # Errors
/// The file cannot be read, or holds anything but one number.
fn ceiling(root: &Path, relative: &str) -> Result<usize, GateFailure> {
    let held = counted(root, relative)?;
    let [written] = held.as_slice() else {
        return Err(GateFailure(format!(
            "{relative} holds one number and nothing else."
        )));
    };
    written.parse::<usize>().map_err(|_not_a_number| {
        GateFailure(format!(
            "{relative} holds {written}, which is not a number."
        ))
    })
}

/// The lines of a ledger that are not its header.
///
/// # Errors
/// The file cannot be read.
fn counted(root: &Path, relative: &str) -> Result<Vec<String>, GateFailure> {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect())
}

/// Which of these the ledger still waives, and which nobody has reviewed.
///
/// The ledger may shrink and never grow.
/// A line that is still there is reported as nothing; a catch-all that is not on it is refused, so writing one is not a thing anybody decides while writing — it is a line somebody else reads.
/// A group of catch-all arms the ledger either waives or has never been shown.
///
/// `name` is what the ledger holds and `line` is only where to look: a coordinate cannot say what is being waived, and a ledger that keyed on one waived whatever happened to be standing there when it was next read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Waived {
    name: String,
    file: String,
    line: usize,
}

fn ratcheted(root: &Path, standing: &[Waived]) -> Result<Vec<lint_scan::Finding>, GateFailure> {
    let path = root.join("xtask/wildcard_allowlist.txt");
    let ledger = std::fs::read_to_string(&path)
        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
    let allowed: Vec<&str> = ledger
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let mut found = Vec::new();
    for one in standing {
        if allowed.iter().any(|line| *line == one.name) {
            continue;
        }
        found.push(lint_scan::Finding {
            kind: lint_scan::Kind::WildcardOverOurOwn,
            file: one.file.clone(),
            line: one.line,
        });
    }
    let stale: Vec<&&str> = allowed
        .iter()
        .filter(|line| !standing.iter().any(|one| one.name == ***line))
        .collect();
    if !stale.is_empty() {
        let how_many = stale.len();
        let named = stale
            .iter()
            .map(|line| format!("\n  {line}"))
            .collect::<Vec<_>>()
            .concat();
        return Err(GateFailure(format!(
            "lints: xtask/wildcard_allowlist.txt names {how_many} line(s) that no longer \
             catch everything left of a set this repository closes. Take them out: a \
             ledger that keeps a waiver nobody needs is one nobody reads.{named}"
        )));
    }
    Ok(found)
}

/// joiners rather than reading the spelling.
fn loose_layouts(root: &Path, files: &[PathBuf]) -> Result<Vec<lint_scan::Finding>, GateFailure> {
    let mut layouts = Vec::new();
    let mut configured: Vec<String> = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let module = path
            .file_stem()
            .ok_or_else(|| GateFailure(format!("{} has no file stem", path.display())))?
            .to_str()
            .ok_or_else(|| GateFailure(format!("{} has a non-UTF-8 file stem", path.display())))?
            .to_owned();
        for (name, _line) in lint_scan::exported_strings(&source) {
            layouts.push((module.clone(), name));
        }
    }
    for path in production_sources(root)? {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        configured.extend(lint_scan::configured_directories(&source));
    }
    configured.sort();
    configured.dedup();
    let mut found = Vec::new();
    for path in files {
        if path.file_name().is_some_and(|name| name == "config.rs") {
            continue;
        }
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        for line in lint_scan::spelled(&source, &configured) {
            found.push(lint_scan::Finding {
                kind: lint_scan::Kind::LooseLayout,
                file: label.clone(),
                line,
            });
        }
    }
    for path in tests_under(root)? {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, &path)?;
        for (module, name) in &layouts {
            if !lint_scan::joins(&source, name)
                || lint_scan::imported_from(&source, name).as_ref() != Some(module)
            {
                continue;
            }
            let line = source
                .lines()
                .position(|line| lint_scan::joins(line, name))
                .map_or(1, |at| at.saturating_add(1));
            found.push(lint_scan::Finding {
                kind: lint_scan::Kind::LooseLayout,
                file: label.clone(),
                line,
            });
        }
    }
    Ok(found)
}

/// Every test source of the workspace, which is where a layout being joined freezes it.
fn tests_under(root: &Path) -> Result<Vec<PathBuf>, GateFailure> {
    let mut found = Vec::new();
    for base in SOURCE_ROOTS {
        for entry in WalkDir::new(root.join(base)).sort_by_file_name() {
            let entry = walked(entry)?;
            let path = entry.path();
            let relative = relative_slash(root, path)?;
            if entry.file_type().is_file()
                && path.extension().is_some_and(|one| one == "rs")
                && relative.contains("/tests/")
            {
                found.push(path.to_path_buf());
            }
        }
    }
    Ok(found)
}

/// The production source files the seam ratchet scans.
///
/// # Errors
/// Any directory entry cannot be read; an incomplete source set proves no gate.
pub fn production_sources(root: &Path) -> Result<Vec<PathBuf>, GateFailure> {
    let mut files = Vec::new();
    for base in SOURCE_ROOTS {
        for entry in WalkDir::new(root.join(base)).sort_by_file_name() {
            let entry = walked(entry)?;
            let path = entry.path();
            if entry.file_type().is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                let relative = relative_slash(root, path)?;
                if is_production(&relative) {
                    files.push(path.to_path_buf());
                }
            }
        }
    }
    Ok(files)
}

fn is_production(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.first() == Some(&"crates") && parts.get(1) == Some(&"njutest-devkit") {
        return false;
    }
    let inside_src = parts.iter().position(|part| *part == "src");
    let Some(src_index) = inside_src else {
        return false;
    };
    let below_src = parts.get(src_index.saturating_add(1)..).unwrap_or(&[]);
    !below_src
        .iter()
        .any(|part| *part == "testkit" || *part == "tests")
        && !parts
            .iter()
            .any(|part| *part == "tests" || *part == "benches" || *part == "examples")
}

fn relative_slash(root: &Path, path: &Path) -> Result<String, GateFailure> {
    let relative = path.strip_prefix(root).map_err(|error| {
        GateFailure(format!(
            "{} is outside {}: {error}",
            path.display(),
            root.display()
        ))
    })?;
    let relative = relative
        .to_str()
        .ok_or_else(|| GateFailure(format!("{} is not UTF-8", relative.display())))?;
    Ok(relative.replace('\\', "/"))
}

/// The seam ratchet against `xtask/seam_allowlist.txt`.
///
/// # Errors
/// Returns a disagreement between the scan and the ledger, or an unreadable file.
pub fn devgates(root: &Path) -> Result<String, GateFailure> {
    let planted = seam_sentinels()?;
    let (files, found) = seam_findings(root)?;
    let ledger_path = root.join("xtask/seam_allowlist.txt");
    let ledger_text = std::fs::read_to_string(&ledger_path)
        .map_err(|error| GateFailure(format!("{}: {error}", ledger_path.display())))?;
    let ledger =
        devgates::parse_ledger(&ledger_text).map_err(|error| GateFailure(error.to_string()))?;
    devgates::compare(&found, &ledger)
        .map_err(|disagreement| GateFailure(disagreement.to_string()))?;
    let most = ceiling(root, "xtask/seam_ceiling.txt")?;
    if ledger.len() > most {
        return Err(GateFailure(format!(
            "devgates: the seam ledger carries {} seam(s) and xtask/seam_ceiling.txt \
             allows {most}. That file may shrink and never grow, so a new seam is a \
             number going up in a file of its own — which is the review the ledger \
             exists to ask for. The scan and the ledger agreeing is set equality: it \
             says nothing about the set having grown, which is what this holds.",
            ledger.len()
        )));
    }
    Ok(format!(
        "devgates: {planted} planted seams found first, each as the kind it was planted as; \
         then {files} production files scanned, {} seam(s) recorded in the ledger, \
         {most} allowed",
        ledger.len()
    ))
}

/// How many production files the seam scan read under `root`, and every seam in them.
///
/// # Errors
/// A file that could not be read or parsed.
fn seam_findings(root: &Path) -> Result<(usize, Vec<devgates::Seam>), GateFailure> {
    let mut found = Vec::new();
    let files = production_sources(root)?;
    for path in &files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        let seams = devgates::scan_source(&label, &source)
            .map_err(|error| GateFailure(format!("{label}: {error}")))?;
        found.extend(seams);
    }
    found.sort();
    found.dedup();
    Ok((files.len(), found))
}

/// How many planted shapes every seam kind was found in, before any real file is read.
///
/// # Errors
/// The first kind a planted shape of it was not found as.
pub fn seam_sentinels() -> Result<usize, GateFailure> {
    let mut found = 0_usize;
    for kind in devgates::SeamKind::ALL {
        let shapes = seam_sighted(kind, kind.planted())?;
        found = found.checked_add(shapes).ok_or_else(|| {
            GateFailure("devgates: more planted shapes than a count can hold".to_owned())
        })?;
    }
    Ok(found)
}

/// How many shapes of `planted` the seam scan found as `kind`, which is all of them or an error.
///
/// # Errors
/// The first shape the scan did not find as `kind`, or planted text that does not parse.
pub fn seam_sighted(kind: devgates::SeamKind, planted: &str) -> Result<usize, GateFailure> {
    let kinds = |found: Vec<devgates::Seam>| -> Vec<devgates::SeamKind> {
        found.into_iter().map(|seam| seam.kind).collect()
    };
    sighted(
        &Planted {
            gate: "devgates",
            file: format!("xtask/sentinels/seams/{}.planted", kind.label()),
            label: kind.label(),
            text: planted,
        },
        &kind,
        |path, text| {
            devgates::scan_source(path, text)
                .map(kinds)
                .map_err(|error| GateFailure(error.to_string()))
        },
        |root| seam_findings(root).map(|(_files, found)| kinds(found)),
    )
}

/// Dependency direction between the workspace crates.
///
/// # Errors
/// Returns every edge the direction rule refuses, or a `cargo metadata` failure.
pub fn deps(root: &Path) -> Result<String, GateFailure> {
    let census = census(root)?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateFailure(format!("cargo metadata: {error}")))?;
    let members: Vec<String> = metadata
        .workspace_packages()
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    let mut edges = Vec::new();
    let mut direct = Vec::new();
    for package in metadata.workspace_packages() {
        for dependency in &package.dependencies {
            direct.push((package.name.as_str(), dependency.name.as_str()));
            if members.contains(&dependency.name) {
                let kind = match dependency.kind {
                    cargo_metadata::DependencyKind::Development => deps::EdgeKind::Dev,
                    _ => deps::EdgeKind::Normal,
                };
                edges.push(deps::Edge {
                    from: package.name.to_string(),
                    to: dependency.name.clone(),
                    kind,
                });
            }
        }
    }
    let violations = deps::check(&edges);
    let mut prohibited = deps::prohibited_direct_dependencies(direct);
    let fuzz = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("fuzz/Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateFailure(format!("cargo metadata for fuzz/Cargo.toml: {error}")))?;
    prohibited.extend(deps::prohibited_direct_dependencies(
        fuzz.workspace_packages().iter().flat_map(|package| {
            package
                .dependencies
                .iter()
                .map(|dependency| (package.name.as_str(), dependency.name.as_str()))
        }),
    ));
    prohibited.sort();
    prohibited.dedup();
    if violations.is_empty() && prohibited.is_empty() {
        return Ok(format!(
            "deps: {} internal edges, all in the allowed direction; {census}",
            edges.len()
        ));
    }
    let mut message = String::from("deps: the dependency direction rule refuses:\n");
    for violation in violations {
        line(&mut message, format_args!("  {violation}"));
    }
    for violation in prohibited {
        line(&mut message, format_args!("  {violation}"));
    }
    append(&mut message, format_args!("{}", deps::RULE));
    Err(GateFailure(message))
}

/// Every cargo manifest in the tree, classified, so a fourth kind cannot appear unnoticed.
///
/// A manifest belongs to the root workspace, to the fuzz workspace, or to a fixture, and each class has a command that reaches it: `cargo nextest run --workspace`, `cargo xtask fuzz-clippy`, and the fate suite.
/// `fuzz/` was the one workspace nothing compiled while testing anything, and it rotted; `compiler-surfaces` arrived as a fourth root and the seam ratchet did not see it for a campaign.
/// What a command reaches is a fact about this tree, and a fact about this tree is something a gate can hold.
///
/// # Errors
/// A manifest that belongs to none of the three, or metadata that could not be read.
fn census(root: &Path) -> Result<String, GateFailure> {
    let members: BTreeSet<PathBuf> = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateFailure(format!("cargo metadata: {error}")))?
        .workspace_packages()
        .iter()
        .map(|package| PathBuf::from(package.manifest_path.as_std_path()))
        .collect();
    let fuzz = root.join("fuzz");
    let fixtures = root.join("fixtures");
    let mut counted = [0_usize; 3];
    let mut loose = Vec::new();
    let walk = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || entry.file_name().as_encoded_bytes().first() != Some(&b'.')
                    && entry.file_name() != std::ffi::OsStr::new("target")
        });
    for entry in walk {
        let entry = walked(entry)?;
        let path = entry.path();
        if !entry.file_type().is_file() || entry.file_name() != std::ffi::OsStr::new("Cargo.toml") {
            continue;
        }
        let relative = relative_slash(root, path)?;
        let at = if path == root.join("Cargo.toml") || members.contains(path) {
            0
        } else if path.starts_with(&fuzz) {
            1
        } else if path.starts_with(&fixtures) {
            2
        } else {
            loose.push(relative);
            continue;
        };
        if let Some(count) = counted.get_mut(at) {
            *count = count.saturating_add(1);
        }
    }
    if !loose.is_empty() {
        return Err(GateFailure(format!(
            "deps: {} cargo manifest(s) belong to no class this repository has a command \
             for. A manifest is a member of the root workspace, the fuzz workspace, or a \
             fixture, and each of those is reached by `cargo nextest run --workspace`, \
             `cargo xtask fuzz-clippy`, and the fate suite. One in none of them is \
             compiled by nothing that tests anything, which is how fuzz/ rotted: {loose:?}",
            loose.len()
        )));
    }
    Ok(format!(
        "{} root-workspace manifest(s), {} under fuzz/ and {} under fixtures/, each \
         reached by a command",
        counted.first().copied().unwrap_or_default(),
        counted.get(1).copied().unwrap_or_default(),
        counted.get(2).copied().unwrap_or_default()
    ))
}

/// Conventions of the fixture projects.
///
/// # Errors
/// Returns every convention a fixture breaks.
pub fn fixtures(root: &Path) -> Result<String, GateFailure> {
    let dir = root.join("fixtures");
    let names = fixtures::discover(&dir)
        .map_err(|error| GateFailure(format!("{}: {error}", dir.display())))?;
    let mut problems = Vec::new();
    for name in &names {
        for problem in fixtures::check_fixture(&dir.join(name))
            .map_err(|error| GateFailure(format!("fixtures/{name}: {error}")))?
        {
            problems.push(format!("fixtures/{name}: {problem}"));
        }
    }
    if problems.is_empty() {
        return Ok(format!(
            "fixtures: {} fixture projects follow the conventions",
            names.len()
        ));
    }
    problems.sort();
    Err(GateFailure(format!(
        "fixtures: {}\n{}",
        problems.join("\n"),
        fixtures::RULE
    )))
}

/// Version consistency between the workspace and the release manifest.
///
/// # Errors
/// Returns every inconsistency.
pub fn release_check(root: &Path) -> Result<String, GateFailure> {
    let read = |relative: &str| {
        std::fs::read_to_string(root.join(relative))
            .map_err(|error| GateFailure(format!("{relative}: {error}")))
    };
    let workspace = read("Cargo.toml")?;
    let mut members = Vec::new();
    for entry in WalkDir::new(root.join("crates"))
        .max_depth(2)
        .sort_by_file_name()
    {
        let entry = walked(entry)?;
        if entry.file_name() == "Cargo.toml" {
            let label = relative_slash(root, entry.path())?;
            members.push((label.clone(), read(&label)?));
        }
    }
    let member_refs: Vec<(&str, &str)> = members
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let mut problems = release::check(&workspace, &member_refs);
    let train = read("release-plz.toml")?;
    let mut every = members.clone();
    for directory in release::plain_members(&workspace) {
        let label = format!("{directory}/Cargo.toml");
        every.push((label.clone(), read(&label)?));
    }
    let names = release::member_names(&every);
    let member_names: Vec<&str> = names.iter().map(String::as_str).collect();
    problems.extend(release::release_train(&train, &member_names));
    if problems.is_empty() {
        let version = release::workspace_version(&workspace).unwrap_or_default();
        return Ok(format!(
            "release-check: version {version} is consistent across {} member manifests, and the release train names {} of them",
            members.len(),
            member_names.len()
        ));
    }
    Err(GateFailure(format!(
        "release-check:\n  {}",
        problems.join("\n  ")
    )))
}

/// Every milestone-shaped name in the book resolves to exactly one row in the roadmap.
///
/// # Errors
/// The roadmap registry is malformed, a page cannot be read, or a page names a milestone the registry does not declare.
pub fn milestones(root: &Path) -> Result<String, GateFailure> {
    let roadmap_path = root.join("docs/roadmap.md");
    let roadmap = std::fs::read_to_string(&roadmap_path)
        .map_err(|error| GateFailure(format!("{}: {error}", roadmap_path.display())))?;
    let registered = crate::milestones::registry(&roadmap)
        .map_err(|error| GateFailure(format!("milestones: docs/roadmap.md: {error}")))?;
    let mut unresolved = Vec::new();
    let mut pages = 0usize;
    for entry in WalkDir::new(root.join("docs")) {
        let entry = walked(entry)?;
        if !entry.file_type().is_file()
            || entry.path().extension() != Some(std::ffi::OsStr::new("md"))
        {
            continue;
        }
        pages = pages.saturating_add(1);
        let text = std::fs::read_to_string(entry.path())
            .map_err(|error| GateFailure(format!("{}: {error}", entry.path().display())))?;
        for reference in crate::milestones::references(&text) {
            if !registered.contains(&reference) {
                unresolved.push(format!(
                    "{} names {reference}",
                    relative_slash(root, entry.path())?
                ));
            }
        }
    }
    unresolved.sort();
    unresolved.dedup();
    if !unresolved.is_empty() {
        return Err(GateFailure(format!(
            "milestones: these references have no row in docs/roadmap.md:\n  {}",
            unresolved.join("\n  ")
        )));
    }
    Ok(format!(
        "milestones: {pages} pages name only the {} registered milestones",
        registered.len()
    ))
}

/// Every workspace crate declares what Rust visibility means for it, and every incidental surface is named by the private compiler harness.
///
/// # Errors
/// Cargo metadata is unreadable, a declaration is absent or contradictory, or the compiler harness and the incidental declarations are not the same set.
pub fn surfaces(root: &Path) -> Result<String, GateFailure> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateFailure(format!("cargo metadata: {error}")))?;
    let mut packages = Vec::new();
    let mut harness_targets = Vec::new();
    for package in metadata.workspace_packages() {
        let njutest = package.metadata.get("njutest");
        let declared = njutest
            .and_then(|value| value.get("surface"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        if package.name.as_str() == "compiler-surfaces" {
            harness_targets.extend(surface_harnesses(package)?);
        }
        let library = package.targets.iter().any(|target| {
            target.is_lib()
                || target.is_proc_macro()
                || target.is_rlib()
                || target.is_dylib()
                || target.is_cdylib()
                || target.is_staticlib()
        });
        let library_path = package
            .targets
            .iter()
            .find(|target| {
                target.is_lib()
                    || target.is_proc_macro()
                    || target.is_rlib()
                    || target.is_dylib()
                    || target.is_cdylib()
                    || target.is_staticlib()
            })
            .map(|target| target.src_path.as_std_path())
            .map(std::fs::canonicalize)
            .transpose()
            .map_err(|error| GateFailure(format!("{} library target: {error}", package.name)))?;
        let binary = package.targets.iter().any(cargo_metadata::Target::is_bin);
        packages.push(crate::surface::Package {
            name: package.name.to_string(),
            declared,
            publishable: package
                .publish
                .as_ref()
                .is_none_or(|registries| !registries.is_empty()),
            library,
            library_path,
            binary,
        });
    }
    let problems = crate::surface::check(&packages, &harness_targets);
    if !problems.is_empty() {
        return Err(GateFailure(format!(
            "surfaces: crate visibility has no mechanically checked meaning:\n  {}",
            problems.join("\n  ")
        )));
    }
    Ok(format!(
        "surfaces: {} crates declare their visibility; {} incidental surfaces are compiled privately",
        packages.len(),
        harness_targets.len()
    ))
}

fn surface_harnesses(
    package: &cargo_metadata::Package,
) -> Result<Vec<crate::surface::Harness>, GateFailure> {
    package
        .targets
        .iter()
        .filter(|target| target.is_bin())
        .map(|target| {
            let source = std::fs::read_to_string(target.src_path.as_std_path())
                .map_err(|error| GateFailure(format!("{}: {error}", target.src_path)))?;
            let product = crate::surface::harness_product(&source, target.src_path.as_std_path())
                .map_err(|error| {
                    GateFailure(format!("{}: invalid Rust: {error}", target.src_path))
                })?
                .map(|path| {
                    std::fs::canonicalize(&path)
                        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))
                })
                .transpose()?;
            Ok(crate::surface::Harness {
                name: target.name.clone(),
                product,
                public_root: crate::surface::has_public_root(&source),
            })
        })
        .collect()
}

/// Every gate, in order, stopping at the first failure.
///
/// # Errors
/// Returns the first gate's failure.
pub fn all(root: &Path) -> Result<String, GateFailure> {
    let mut report = String::new();
    for gate in [
        devgates,
        lints,
        deps,
        fixtures,
        release_check,
        milestones,
        surfaces,
        reached,
        waivers,
    ] {
        line(&mut report, format_args!("{}", gate(root)?));
    }
    Ok(report.trim_end().to_owned())
}

/// Whether a completed run's verdicts are the ones its own recording supports.
///
/// # Errors
/// A run directory whose report could not be read, is not JSON, or is not the assurance report.
pub fn proofaudit(
    run: &Path,
    trace: Option<&Path>,
) -> Result<proofaudit::Audit, proofaudit::AuditError> {
    let path = run.join(proofaudit::REPORT_FILE);
    let label = path.display().to_string();
    let text =
        std::fs::read_to_string(&path).map_err(|source| proofaudit::AuditError::Unreadable {
            path: label.clone(),
            source,
        })?;
    let recorded = trace
        .map(|directory| directory.join("trace.jsonl"))
        .map(|path| {
            let label = path.display().to_string();
            std::fs::read_to_string(&path)
                .map(|text| (label.clone(), text))
                .map_err(|source| proofaudit::AuditError::Unreadable {
                    path: label,
                    source,
                })
        })
        .transpose()?;
    let engines = trace
        .map(engine_recordings)
        .transpose()?
        .unwrap_or_default();
    proofaudit::audit_with(
        &label,
        &text,
        proofaudit::Recorded {
            runner: recorded
                .as_ref()
                .map(|(recording_path, text)| (recording_path.as_str(), text.as_str())),
            engines: &engines,
        },
        Some(run),
    )
}

/// Every configured build's engine recording under a runner recording, in namespace order, each as its path and its text.
fn engine_recordings(trace: &Path) -> Result<Vec<(String, String)>, proofaudit::AuditError> {
    let builds = trace.join("builds");
    let unreadable = |path: &Path, source: std::io::Error| proofaudit::AuditError::Unreadable {
        path: path.display().to_string(),
        source,
    };
    let entries = match std::fs::read_dir(&builds) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(unreadable(&builds, source)),
    };
    let mut namespaces = Vec::new();
    for entry in entries {
        namespaces.push(entry.map_err(|source| unreadable(&builds, source))?.path());
    }
    namespaces.sort();
    namespaces
        .into_iter()
        .map(|namespace| {
            let path = namespace.join("engine").join("trace.jsonl");
            std::fs::read_to_string(&path)
                .map(|text| (path.display().to_string(), text))
                .map_err(|source| unreadable(&path, source))
        })
        .collect()
}

/// How many planted defects every layer of the proof audit found, after the clean specimen drew nothing from any of them.
///
/// # Errors
/// A clean specimen some layer finds a violation in, which means that layer fires on anything, or the first layer that did not find a defect planted for it.
pub fn proofaudit_sentinels() -> Result<usize, GateFailure> {
    let clean = proofaudit::sentinel::clean();
    let audit = proofaudit_specimen(&clean)?;
    if audit.violations() > 0 {
        return Err(GateFailure(format!(
            "proofaudit: the clean specimen draws {} violation(s), so a layer that fires on it \
             fires on anything and its violations about a real run say nothing. Nothing this \
             gate would have said is believed until the clean specimen is silent again.\n{audit}",
            audit.violations()
        )));
    }
    let mut found = 0_usize;
    for layer in proofaudit::Layer::ALL {
        let planted = proofaudit_sighted(layer, &layer.planted())?;
        found = found.checked_add(planted).ok_or_else(|| {
            GateFailure("proofaudit: more planted defects than a count can hold".to_owned())
        })?;
    }
    Ok(found)
}

/// How many of `planted` the proof audit found as a violation of `layer`, which is all of them or an error.
///
/// # Errors
/// The first perturbation `layer` found nothing in, an empty `planted`, or a specimen that could not be laid out or read.
pub fn proofaudit_sighted(
    layer: proofaudit::Layer,
    planted: &[proofaudit::sentinel::Perturbation],
) -> Result<usize, GateFailure> {
    if planted.is_empty() {
        return Err(GateFailure(format!(
            "proofaudit: the {} layer is blind. Nothing is planted for it, so its silence \
             about a real run says nothing. Nothing this gate would have said is believed \
             until a defect is planted for it and found.",
            layer.label()
        )));
    }
    for perturbation in planted {
        let audit = proofaudit_specimen(perturbation)?;
        if !audit.violated(layer) {
            return Err(GateFailure(format!(
                "proofaudit: the {label} layer is blind. Its planted defect `{name}` \
                 (`Layer::planted` in xtask/src/proofaudit/sentinel.rs) drew no {label} \
                 violation, so a run it is silent about says nothing about whether the run is \
                 sound. Nothing this gate would have said is believed until the planted defect \
                 is found again.\n{audit}",
                label = layer.label(),
                name = perturbation.name,
            )));
        }
    }
    Ok(planted.len())
}

/// The proof audit of one specimen, laid out on disk and read as the gate reads a run.
fn proofaudit_specimen(
    specimen: &proofaudit::sentinel::Perturbation,
) -> Result<proofaudit::Audit, GateFailure> {
    let name = specimen.name;
    let laid = specimen
        .lay()
        .map_err(|error| GateFailure(format!("proofaudit: specimen `{name}`: {error}")))?;
    proofaudit(laid.run(), laid.trace())
        .map_err(|error| GateFailure(format!("proofaudit: specimen `{name}`: {error}")))
}

/// What one engine run is audited against: its own directory, and everything a layer needs beyond it.
#[derive(Debug, Clone, Copy)]
pub struct EngineRun<'a> {
    /// The directory the run left its report in.
    pub run: &'a Path,
    /// The directory the run left its recording in.
    pub trace: Option<&'a Path>,
    /// The reports of the other parts of this catalog.
    pub shards: &'a [PathBuf],
    /// The configuration file whose accepted survivors the run is held to.
    pub ledger: Option<&'a Path>,
    /// Whether the census of the walk's own decisions is re-derived.
    pub sites: bool,
}

/// Re-decides one completed engine run from its own report, recording, and ledger.
///
/// # Errors
/// The report that is not there, is not JSON, or is not a run report.
pub fn engine_audit(asked: &EngineRun<'_>) -> Result<engineaudit::Audit, engineaudit::AuditError> {
    let path = asked.run.join(engineaudit::REPORT_FILE);
    let label = path.display().to_string();
    let text =
        std::fs::read_to_string(&path).map_err(|source| engineaudit::AuditError::Unreadable {
            path: label.clone(),
            source,
        })?;
    let recorded = asked
        .trace
        .map(|directory| read_engine_document(&directory.join("trace.jsonl")))
        .transpose()?;
    let parts: Vec<(String, String)> = asked
        .shards
        .iter()
        .map(|part| {
            let metadata = std::fs::symlink_metadata(part).map_err(|source| {
                engineaudit::AuditError::Unreadable {
                    path: part.display().to_string(),
                    source,
                }
            })?;
            if metadata.file_type().is_symlink() {
                return Err(engineaudit::AuditError::Unreadable {
                    path: part.display().to_string(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "an engine shard path must not be a symbolic link",
                    ),
                });
            }
            let path = if metadata.is_dir() {
                part.join(engineaudit::REPORT_FILE)
            } else {
                part.clone()
            };
            read_engine_document(&path)
        })
        .collect::<Result<_, _>>()?;
    let ledger = asked.ledger.map(read_engine_document).transpose()?;
    let reached = read_optional_engine_document(&asked.run.join("reached-v1.json"))?;
    let touched = read_optional_engine_document(&asked.run.join("touched-v1.json"))?;
    let catalog = read_optional_engine_document(&asked.run.join("catalog-v1.json"))?;
    let probe_logs = read_probe_logs(&asked.run.join("probe"))?;
    engineaudit::audit(
        &label,
        &text,
        &engineaudit::Evidence {
            recorded: recorded
                .as_ref()
                .map(|(path, text)| engineaudit::Source { path, text }),
            shards: parts
                .iter()
                .map(|(path, text)| engineaudit::Source { path, text })
                .collect(),
            ledger: ledger
                .as_ref()
                .map(|(path, text)| engineaudit::Source { path, text }),
            sites: asked.sites,
            reached: reached
                .as_ref()
                .map(|(path, text)| engineaudit::Source { path, text }),
            touched: touched
                .as_ref()
                .map(|(path, text)| engineaudit::Source { path, text }),
            catalog: catalog
                .as_ref()
                .map(|(path, text)| engineaudit::Source { path, text }),
            probe_logs,
        },
    )
}

/// How many planted defects every layer of the engine audit found, after the clean specimen drew nothing from any of them.
///
/// # Errors
/// A clean specimen some layer finds a violation in, which means that layer fires on anything, or the first layer that did not find a defect planted for it.
pub fn engine_audit_sentinels() -> Result<usize, GateFailure> {
    let clean = engineaudit::sentinel::clean();
    let audit = engine_audit_specimen(&clean)?;
    if audit.violations() > 0 {
        return Err(GateFailure(format!(
            "engine-audit: the clean specimen draws {} violation(s), so a layer that fires on it \
             fires on anything and its violations about a real run say nothing. Nothing this \
             gate would have said is believed until the clean specimen is silent again.\n{audit}",
            audit.violations()
        )));
    }
    let mut found = 0_usize;
    for layer in engineaudit::Layer::ALL {
        let planted = engine_audit_sighted(layer, &layer.planted())?;
        found = found.checked_add(planted).ok_or_else(|| {
            GateFailure("engine-audit: more planted defects than a count can hold".to_owned())
        })?;
    }
    Ok(found)
}

/// How many of `planted` the audit found as a violation of `layer`, which is all of them or an error.
///
/// # Errors
/// The first perturbation `layer` found nothing in, an empty `planted`, or a specimen that could not be laid out or read.
pub fn engine_audit_sighted(
    layer: engineaudit::Layer,
    planted: &[engineaudit::sentinel::Perturbation],
) -> Result<usize, GateFailure> {
    if planted.is_empty() {
        return Err(GateFailure(format!(
            "engine-audit: the {} layer is blind. Nothing is planted for it, so its silence \
             about a real run says nothing. Nothing this gate would have said is believed \
             until a defect is planted for it and found.",
            layer.label()
        )));
    }
    for perturbation in planted {
        let audit = engine_audit_specimen(perturbation)?;
        if !audit.violated(layer) {
            return Err(GateFailure(format!(
                "engine-audit: the {label} layer is blind. Its planted defect `{name}` \
                 (`Layer::planted` in xtask/src/engineaudit/sentinel.rs) drew no {label} \
                 violation, so a run it is silent about says nothing about whether the run is \
                 sound. Nothing this gate would have said is believed until the planted defect \
                 is found again.\n{audit}",
                label = layer.label(),
                name = perturbation.name,
            )));
        }
    }
    Ok(planted.len())
}

/// The audit of one specimen, laid out on disk and read with every layer asked.
fn engine_audit_specimen(
    specimen: &engineaudit::sentinel::Perturbation,
) -> Result<engineaudit::Audit, GateFailure> {
    let name = specimen.name;
    let laid = specimen
        .lay()
        .map_err(|error| GateFailure(format!("engine-audit: specimen `{name}`: {error}")))?;
    engine_audit(&EngineRun {
        run: laid.run(),
        trace: Some(laid.trace()),
        shards: laid.shards(),
        ledger: laid.ledger(),
        sites: true,
    })
    .map_err(|error| GateFailure(format!("engine-audit: specimen `{name}`: {error}")))
}

fn read_engine_document(path: &Path) -> Result<(String, String), engineaudit::AuditError> {
    let label = path.display().to_string();
    std::fs::read_to_string(path)
        .map(|text| (label.clone(), text))
        .map_err(|source| engineaudit::AuditError::Unreadable {
            path: label,
            source,
        })
}

fn read_optional_engine_document(
    path: &Path,
) -> Result<Option<(String, String)>, engineaudit::AuditError> {
    match read_engine_document(path) {
        Ok(document) => Ok(Some(document)),
        Err(engineaudit::AuditError::Unreadable {
            path: label,
            source,
        }) if source.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(path) {
                Err(absent) if absent.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Ok(_) => Err(engineaudit::AuditError::Unreadable {
                    path: label,
                    source,
                }),
                Err(metadata) => Err(engineaudit::AuditError::Unreadable {
                    path: label,
                    source: metadata,
                }),
            }
        }
        Err(error) => Err(error),
    }
}

fn read_probe_logs(path: &Path) -> Result<Vec<String>, engineaudit::AuditError> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(path) {
                Err(absent) if absent.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Vec::new());
                }
                Ok(_) => {
                    return Err(engineaudit::AuditError::Unreadable {
                        path: path.display().to_string(),
                        source,
                    });
                }
                Err(metadata) => {
                    return Err(engineaudit::AuditError::Unreadable {
                        path: path.display().to_string(),
                        source: metadata,
                    });
                }
            }
        }
        Err(source) => {
            return Err(engineaudit::AuditError::Unreadable {
                path: path.display().to_string(),
                source,
            });
        }
    };
    entries
        .map(|entry| {
            let entry = entry.map_err(|source| engineaudit::AuditError::Unreadable {
                path: path.display().to_string(),
                source,
            })?;
            entry
                .file_name()
                .into_string()
                .map_err(|_name| engineaudit::AuditError::Unreadable {
                    path: entry.path().display().to_string(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "probe log name is not UTF-8",
                    ),
                })
        })
        .collect()
}

/// What a release is made of, as a `CycloneDX` document.
///
/// # Errors
/// A `cargo metadata` that could not be run or read, or a file that could not be written.
pub fn sbom(root: &Path, output: Option<&Path>) -> Result<String, GateFailure> {
    let asked = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| GateFailure(format!("cargo metadata: {error}")))?;
    if !asked.status.success() {
        let stderr = std::str::from_utf8(&asked.stderr)
            .map_err(|error| GateFailure(format!("cargo metadata stderr is not UTF-8: {error}")))?;
        return Err(GateFailure(format!("cargo metadata: {}", stderr.trim())));
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| GateFailure(format!("Cargo.toml: {error}")))?;
    let version = release::workspace_version(&manifest)
        .ok_or_else(|| GateFailure("Cargo.toml has no [workspace.package].version".to_owned()))?;
    let stdout = std::str::from_utf8(&asked.stdout)
        .map_err(|error| GateFailure(format!("cargo metadata stdout is not UTF-8: {error}")))?;
    let bom = crate::sbom::of(stdout, ("njutest", &version))
        .map_err(|error| GateFailure(error.to_string()))?;
    let document = serde_json::to_string_pretty(&bom)
        .map_err(|error| GateFailure(format!("the bill of materials: {error}")))?;
    match output {
        Some(path) => {
            std::fs::write(path, format!("{document}\n"))
                .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
            Ok(format!(
                "sbom: {} components of {} written to {}",
                bom.components.len(),
                version,
                path.display()
            ))
        }
        None => Ok(document),
    }
}

/// What changed between two stored reports.
///
/// # Errors
/// A document that could not be read, or is not JSON.
pub fn report_diff(before: &Path, after: &Path) -> Result<String, GateFailure> {
    let read = |path: &Path| -> Result<String, GateFailure> {
        std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))
    };
    let (left, right) = (read(before)?, read(after)?);
    let changes = reportdiff::compare(
        (&before.display().to_string(), &left),
        (&after.display().to_string(), &right),
    )
    .map_err(|error| GateFailure(error.to_string()))?;

    if changes.is_empty() {
        return Ok("reportdiff: the two reports claim the same thing".to_owned());
    }
    let mut report = String::from("SUBJECT\tBEFORE\tAFTER\n");
    for change in &changes {
        line(&mut report, format_args!("{change}"));
    }
    Ok(report.trim_end().to_owned())
}

/// What a crate's public surface means, as `[package.metadata.njutest] surface` declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// An API somebody outside depends on, where a function with no caller here is ordinary.
    Public,
    /// Public only because Rust needed it to be, so a function nothing here reaches is one nothing reaches.
    Incidental,
    /// Written to be reached from tests, which is what it is for.
    TestSupport,
}

impl Surface {
    /// The surface `declared` names, and nothing for a name this gate does not know.
    fn named(declared: &str) -> Option<Self> {
        match declared {
            "public" | "unreleased" => Some(Self::Public),
            "incidental" => Some(Self::Incidental),
            "test-support" => Some(Self::TestSupport),
            _ => None,
        }
    }
}

/// Whether a `cfg` predicate puts what it guards behind a test.
///
/// `cfg(test)` and `cfg(feature = "testkit")` and `cfg(any(test, feature = "testkit"))` all do.
/// `cfg(not(test))` is the opposite and is what ships, so naming `test` is not enough on its own.
fn only_for_a_test(predicate: &str) -> bool {
    (predicate.contains("test") || predicate.contains("testkit")) && !predicate.contains("not(test")
}

/// `text` with every item a `cfg` puts behind a test removed, so what remains is what ships.
///
/// A `pub fn` behind `cfg(feature = "testkit")` is test support, and the feature is where somebody said so.
/// Reading only `cfg(test)` reported sixteen of those as public functions nothing reaches, which is the gate believing its own omission.
fn without_test_items(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = next_test_cfg(rest) {
        let (Some(before), Some(after)) = (rest.get(..at), rest.get(at..)) else {
            break;
        };
        kept.push_str(before);
        let Some(open) = after.find('{') else {
            break;
        };
        let mut depth = 0_usize;
        let mut end = None;
        for (offset, character) in after.char_indices().skip(open) {
            match character {
                '{' => depth = depth.saturating_add(1),
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(offset.saturating_add(1));
                        break;
                    }
                }
                _ => {}
            }
        }
        match end.and_then(|end| after.get(end..)) {
            Some(remaining) => rest = remaining,
            None => break,
        }
    }
    kept.push_str(rest);
    kept
}

/// Where the next attribute that puts an item behind a test begins.
fn next_test_cfg(text: &str) -> Option<usize> {
    let mut from = 0_usize;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find("#[cfg")) {
        let start = from.saturating_add(at);
        let line_end = text
            .get(start..)
            .and_then(|rest| rest.find('\n'))
            .map_or(text.len(), |offset| start.saturating_add(offset));
        let attribute = text.get(start..line_end).unwrap_or_default();
        if only_for_a_test(attribute) {
            return Some(start);
        }
        from = line_end.max(start.saturating_add(1));
    }
    None
}
/// Every name a `pub fn` in `text` declares.
fn public_functions(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(after) = line
            .strip_prefix("pub fn ")
            .or_else(|| line.strip_prefix("pub const fn "))
            .or_else(|| line.strip_prefix("pub async fn "))
        else {
            continue;
        };
        let name: String = after
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

/// How many times `name` is used as a word in `text`.
fn mentions(text: &str, name: &str) -> usize {
    let mut count = 0_usize;
    let bytes = text.as_bytes();
    let mut from = 0_usize;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find(name)) {
        let start = from.saturating_add(at);
        let end = start.saturating_add(name.len());
        let before_is_word = start
            .checked_sub(1)
            .and_then(|at| bytes.get(at))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        let after_is_word = bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        if !before_is_word && !after_is_word {
            count = count.saturating_add(1);
        }
        from = end;
    }
    count
}

/// What every workspace crate declares its public surface to mean, and where it lives.
///
/// # Errors
/// Refuses a crate that declares nothing, and a declaration this gate does not know.
fn declared_surfaces(root: &Path) -> Result<Vec<(String, Surface, PathBuf)>, GateFailure> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateFailure(format!("cargo metadata: {error}")))?;
    let mut declared = Vec::new();
    for package in metadata.workspace_packages() {
        let named = package
            .metadata
            .get("njutest")
            .and_then(|value| value.get("surface"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                GateFailure(format!(
                    "{}: no [package.metadata.njutest] surface, so this gate cannot say whether a \
                     function only a test reaches is a finding",
                    package.name
                ))
            })?;
        let surface = Surface::named(named).ok_or_else(|| {
            GateFailure(format!(
                "{}: surface {named:?} is not one this gate knows",
                package.name
            ))
        })?;
        let directory = package
            .manifest_path
            .parent()
            .ok_or_else(|| GateFailure(format!("{}: manifest has no directory", package.name)))?;
        declared.push((
            package.name.to_string(),
            surface,
            directory.as_std_path().to_path_buf(),
        ));
    }
    Ok(declared)
}

/// Every public function `declaring` declares that `tested` names and `ships` does not.
///
/// `ships` holds the definition itself, so one mention there is the declaration and no caller; two is a caller.
/// `tested` holds the definition too, for the same reason.
#[must_use]
pub fn only_a_test_reaches(declaring: &str, ships: &str, tested: &str) -> Vec<String> {
    public_functions(declaring)
        .into_iter()
        .filter(|function| mentions(ships, function) <= 1 && mentions(tested, function) > 1)
        .collect()
}

/// Every public function of an incidental surface is one something other than a test reaches.
///
/// A capability with a test is a capability somebody believed shipped (ADR 0023).
/// `Interposer::during()` had a test, passed it, and production never called it, so the test was evidence about a function nothing used.
/// Where a crate's public surface is an API, a function with no caller here is ordinary and this says nothing.
/// Where it is public only because Rust needed it to be, a function only a test reaches is one nothing reaches.
///
/// # Errors
/// Returns every such function, and refuses a surface nobody declared.
pub fn reached(root: &Path) -> Result<String, GateFailure> {
    let declared = declared_surfaces(root)?;

    let mut ships = String::new();
    let mut tested = String::new();
    for (_name, surface, directory) in &declared {
        for file in rust_files_under(directory)? {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| GateFailure(format!("{}: {error}", file.display())))?;
            let under_src = file.starts_with(directory.join("src"));
            tested.push_str(&text);
            tested.push('\n');
            if *surface != Surface::TestSupport && under_src {
                ships.push_str(&without_test_items(&text));
                ships.push('\n');
            }
        }
    }

    let mut only_tests = Vec::new();
    for (name, surface, directory) in &declared {
        if *surface != Surface::Incidental {
            continue;
        }
        for file in rust_files_under(&directory.join("src"))? {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| GateFailure(format!("{}: {error}", file.display())))?;
            let shipped = without_test_items(&text);
            for function in only_a_test_reaches(&shipped, &ships, &tested) {
                only_tests.push(format!("{name}::{function}"));
            }
        }
    }
    only_tests.sort_unstable();
    only_tests.dedup();

    let held = counted(root, "xtask/reached_ceiling.txt")?;
    let Some(written) = held.first() else {
        return Err(GateFailure(
            "xtask/reached_ceiling.txt holds one number and holds nothing".to_owned(),
        ));
    };
    let ceiling = match written.parse::<usize>() {
        Ok(ceiling) => ceiling,
        Err(why) => {
            return Err(GateFailure(format!(
                "xtask/reached_ceiling.txt holds one number and holds {written:?}: {why}"
            )));
        }
    };
    if only_tests.len() > ceiling {
        return Err(GateFailure(format!(
            "{} public function(s) of an incidental surface are reached by a test and by nothing \
             that ships, against a ceiling of {ceiling}. A test of one of these is evidence about \
             a function nothing uses. Delete it, or call it, or raise the ceiling in a commit that \
             says which and why:\n  {}",
            only_tests.len(),
            only_tests.join("\n  ")
        )));
    }
    Ok(format!(
        "reached: {} public function(s) of an incidental surface are reached only by a test, at or \
         under the ceiling of {ceiling}",
        only_tests.len()
    ))
}

/// Every `.rs` file under `directory`, skipping anything built.
fn rust_files_under(directory: &Path) -> Result<Vec<PathBuf>, GateFailure> {
    let mut files = Vec::new();
    for entry in WalkDir::new(directory).sort_by_file_name() {
        let entry = walked(entry)?;
        let path = entry.path();
        if entry.file_type().is_file()
            && path.extension().is_some_and(|extension| extension == "rs")
            && !path.components().any(|part| part.as_os_str() == "target")
        {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}
