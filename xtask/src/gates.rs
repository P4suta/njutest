// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gates applied to this repository: each one reads the tree, hands it to the pure checker of its module, and renders the answer.

use crate::error::Coded as _;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Arguments;
use std::path::{Component, Path, PathBuf};

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
pub struct GateError(pub String);

impl crate::error::Coded for GateError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::XtCode::GateRefused
    }
}

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
/// The repository cannot be listed; an incomplete source set proves no gate.
pub fn all_sources(root: &Path) -> Result<Vec<PathBuf>, GateError> {
    let files = crate::repository::files(root)?;
    validate_closed_source_inventory(&files)?;
    Ok(files
        .iter()
        .filter(|relative| {
            in_source_roots(relative) && crate::repository::extension_is(relative, "rs")
        })
        .map(|relative| root.join(relative))
        .collect())
}

const SOURCE_ROOTS: [&str; 4] = ["compiler-surfaces", "crates", "xtask", "fuzz"];

/// Whether a workspace-relative path lies under one of the source roots.
fn in_source_roots(relative: &str) -> bool {
    SOURCE_ROOTS
        .iter()
        .any(|root| relative.starts_with(&format!("{root}/")))
}

fn validate_closed_source_inventory(files: &[String]) -> Result<(), GateError> {
    if !files.iter().any(|file| in_source_roots(file)) {
        return Err(GateError(format!(
            "walking the repository: it holds nothing under the source roots {}, so an empty \
             tree would pass as a proved one",
            SOURCE_ROOTS.join(", ")
        )));
    }
    for relative in files {
        if !crate::repository::extension_is(relative, "rs")
            || in_source_roots(relative)
            || relative.starts_with('.')
            || relative.starts_with("fixtures/")
        {
            continue;
        }
        return Err(GateError(format!(
            "walking the repository: {relative} is Rust outside the four scanned source roots; only fixtures/ is an explicit unproved input corpus"
        )));
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
) -> Result<Vec<lint_scan::Finding>, GateError> {
    let mut labels = BTreeSet::new();
    let mut canonical_labels = BTreeMap::new();
    for path in files {
        let label = relative_slash(root, path)?;
        let canonical = std::fs::canonicalize(path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
        labels.insert(label.clone());
        canonical_labels.insert(canonical, label);
    }

    let proc_macros = cargo_source_universe(root, &canonical_labels)?;
    let mut found = Vec::new();
    for (_scope, file, source) in sources {
        for redirect in lint_scan::source_redirects(source)
            .map_err(|error| GateError(format!("{file}: {error}")))?
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
                    (ALLOWED_PATH_REDIRECTS.contains(&(file.as_str(), target.as_str()))
                        || suite_member(file, &target))
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

/// Whether `target` is a sibling test file a crate's one test suite, `file`, compiles as a module.
fn suite_member(file: &str, target: &str) -> bool {
    let path = Path::new(target);
    file.ends_with("/tests/suite.rs")
        && path.extension().is_some_and(|extension| extension == "rs")
        && matches!(
            path.components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        )
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
) -> Result<Vec<ProcMacroPackage>, GateError> {
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|error| GateError(format!("{}: {error}", root.display())))?;
    preflight_cargo_manifests(&canonical_root)?;
    validate_dependency_proc_macro_inventory(&canonical_root)?;
    let mut pending = VecDeque::from([root.join("Cargo.toml"), root.join("fuzz/Cargo.toml")]);
    let mut requested = BTreeSet::new();
    let mut packages = BTreeSet::new();
    let mut proc_macros = Vec::new();

    while let Some(manifest) = pending.pop_front() {
        let manifest = std::fs::canonicalize(&manifest)
            .map_err(|error| GateError(format!("{}: {error}", manifest.display())))?;
        if !requested.insert(manifest.clone()) {
            continue;
        }
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&manifest)
            .no_deps()
            .exec()
            .map_err(|error| {
                GateError(format!(
                    "cargo metadata for {}: {error}",
                    manifest.display()
                ))
            })?;
        let mut newly_seen = Vec::new();
        for package in metadata.workspace_packages() {
            let package_manifest = std::fs::canonicalize(package.manifest_path.as_std_path())
                .map_err(|error| {
                    GateError(format!("{}: {error}", package.manifest_path.as_str()))
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
                        GateError(format!("local dependency {}: {error}", path.as_str()))
                    })?;
                if !inside_source_roots(&canonical_root, &dependency_directory) {
                    return Err(GateError(format!(
                        "lints: local dependency {} is outside the four scanned source roots",
                        path.as_str()
                    )));
                }
                let dependency_manifest = dependency_directory.join("Cargo.toml");
                let dependency_manifest =
                    std::fs::canonicalize(&dependency_manifest).map_err(|error| {
                        GateError(format!("{}: {error}", dependency_manifest.display()))
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

fn validate_dependency_proc_macro_inventory(root: &Path) -> Result<(), GateError> {
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
                GateError(format!(
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
            return Err(GateError(format!(
                "lints: {graph} metadata reports procedural macros absent from {}: {absent_from_lock:?}",
                lockfile.display()
            )));
        }
        let declared = expected.get(graph).ok_or_else(|| {
            GateError(format!(
                "lints: {PROC_MACRO_INVENTORY} has no {graph} graph"
            ))
        })?;
        if observed != *declared {
            let added = observed.difference(declared).cloned().collect::<Vec<_>>();
            let removed = declared.difference(&observed).cloned().collect::<Vec<_>>();
            return Err(GateError(format!(
                "lints: {graph} locked dependency procedural-macro inventory drifted; added {added:?}, removed {removed:?}"
            )));
        }
    }
    Ok(())
}

fn expected_proc_macro_dependencies(
    path: &Path,
) -> Result<BTreeMap<String, BTreeSet<String>>, GateError> {
    let inventory = std::fs::read_to_string(path)
        .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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
            return Err(GateError(format!(
                "{}:{}: expected graph, package, version, and source",
                path.display(),
                line_index.saturating_add(1)
            )));
        };
        let Some(packages) = expected.get_mut(*graph) else {
            return Err(GateError(format!(
                "{}:{}: unknown Cargo graph {graph:?}",
                path.display(),
                line_index.saturating_add(1)
            )));
        };
        let entry = format!("{package} {version} {source}");
        if !packages.insert(entry.clone()) {
            return Err(GateError(format!(
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

fn lock_packages(path: &Path) -> Result<BTreeSet<String>, GateError> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
    let lock = toml::from_str::<toml::Value>(&source)
        .map_err(|error| GateError(format!("parsing {}: {error}", path.display())))?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| GateError(format!("{} has no package array", path.display())))?;
    let mut found = BTreeSet::new();
    for package in packages {
        let table = package
            .as_table()
            .ok_or_else(|| GateError(format!("{} has a non-table package", path.display())))?;
        let name = table
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| GateError(format!("{} has a package without a name", path.display())))?;
        let version = table
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                GateError(format!(
                    "{} has a package without a version",
                    path.display()
                ))
            })?;
        let source = table
            .get("source")
            .map_or(Some("path"), toml::Value::as_str)
            .ok_or_else(|| {
                GateError(format!("{} has a non-text package source", path.display()))
            })?;
        if source.starts_with("registry+") {
            let checksum = table
                .get("checksum")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| {
                    GateError(format!(
                        "{} has registry package {name} {version} without a checksum",
                        path.display()
                    ))
                })?;
            if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(GateError(format!(
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
) -> Result<Vec<ProcMacroPackage>, GateError> {
    let package_manifest = std::fs::canonicalize(package.manifest_path.as_std_path())
        .map_err(|error| GateError(format!("{}: {error}", package.manifest_path.as_str())))?;
    let directory = package_manifest.parent().ok_or_else(|| {
        GateError(format!(
            "{} has no package directory",
            package.manifest_path
        ))
    })?;
    if !inside_source_roots(root, directory) {
        return Err(GateError(format!(
            "lints: local package {} at {} is outside the four scanned source roots",
            package.name, package.manifest_path
        )));
    }
    let mut proc_macros = Vec::new();
    for target in &package.targets {
        let source = std::fs::canonicalize(target.src_path.as_std_path()).map_err(|error| {
            GateError(format!("{} target {}: {error}", package.name, target.name))
        })?;
        if !sources.contains_key(&source) {
            return Err(GateError(format!(
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

fn preflight_cargo_manifests(root: &Path) -> Result<(), GateError> {
    preflight_cargo_manifest(root, &root.join("Cargo.toml"))?;
    for relative in crate::repository::files(root)? {
        if in_source_roots(&relative) && relative.ends_with("/Cargo.toml") {
            preflight_cargo_manifest(root, &root.join(relative))?;
        }
    }
    Ok(())
}

fn preflight_cargo_manifest(root: &Path, path: &Path) -> Result<(), GateError> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
    let manifest = toml::from_str::<toml::Value>(&source)
        .map_err(|error| GateError(format!("parsing {} before Cargo: {error}", path.display())))?;
    validate_manifest_paths(root, path, &manifest)
}

fn validate_manifest_paths(
    root: &Path,
    manifest_path: &Path,
    manifest: &toml::Value,
) -> Result<(), GateError> {
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
) -> Result<(), GateError> {
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
) -> Result<(), GateError> {
    let directory = manifest_path.parent().ok_or_else(|| {
        GateError(format!(
            "{} has no parent directory",
            manifest_path.display()
        ))
    })?;
    let candidate = repository_path(root, directory, declared).ok_or_else(|| {
        GateError(format!(
            "lints: {} declares path {declared:?} outside the repository",
            manifest_path.display()
        ))
    })?;
    if !inside_source_roots(root, &candidate) {
        return Err(GateError(format!(
            "lints: {} declares path {declared:?} outside the four scanned source roots",
            manifest_path.display()
        )));
    }
    if canonical {
        let resolved = std::fs::canonicalize(&candidate)
            .map_err(|error| GateError(format!("{}: {error}", candidate.display())))?;
        if !inside_source_roots(root, &resolved) {
            return Err(GateError(format!(
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
) -> Result<(), GateError> {
    if !matches!(packages, [package] if package.name == "njutest-macros") {
        return Err(GateError(format!(
            "lints: the workspace procedural-macro package inventory drifted: {:?}",
            packages
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>()
        )));
    }
    let package = packages
        .first()
        .ok_or_else(|| GateError("lints: njutest-macros is absent".to_owned()))?;
    let target_label = labels.get(&package.target).ok_or_else(|| {
        GateError(format!(
            "lints: {} procedural-macro target escaped the source labels",
            package.name
        ))
    })?;
    if !sources
        .iter()
        .any(|(_scope, file, _source)| file == target_label)
    {
        return Err(GateError(format!("lints: {target_label} was not read")));
    }
    let source_root = std::fs::canonicalize(&package.source_root)
        .map_err(|error| GateError(format!("{}: {error}", package.source_root.display())))?;
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
                .map_err(|error| GateError(format!("{file}: {error}")))?,
        );
        found.extend(
            lint_scan::opaque_proc_macro_synthesis(file, source)
                .map_err(|error| GateError(format!("{file}: {error}")))?,
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
        return Err(GateError(format!(
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
pub fn waivers(root: &Path) -> Result<String, GateError> {
    let files = all_sources(root)?;
    let (ours, open) = sets(root, &files)?;
    let path = root.join("xtask/wildcard_allowlist.txt");
    let ledger = std::fs::read_to_string(&path)
        .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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
            .map_err(|error| GateError(format!("{file}: {error}")))?;
        let mut shaped = shapes::shapes(&source, &ours)
            .map_err(|error| GateError(format!("{file}: {error}")))?;
        let lines: Vec<usize> = lint_scan::wildcards_over(&source, &ours)
            .map_err(|error| GateError(format!("{file}: {error}")))?
            .into_iter()
            .filter(|one| one.item == item && one.over == over)
            .map(|one| one.line)
            .collect();
        if lines.len() != expected_arms {
            return Err(GateError(format!(
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
pub fn lints(root: &Path) -> Result<String, GateError> {
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
pub fn lints_scanned(root: &Path) -> Result<String, GateError> {
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
    Err(GateError(report.trim_end().to_owned()))
}

/// How many planted shapes every lint kind was found in, before any real file is read.
///
/// # Errors
/// The first kind a planted shape of it was not found as, which means the scan is blind to that shape and its silence about the tree says nothing.
pub fn lint_sentinels() -> Result<usize, GateError> {
    let mut found = 0_usize;
    for kind in lint_scan::Kind::ALL {
        let shapes = lint_sighted(*kind, kind.planted())?;
        found = found.checked_add(shapes).ok_or_else(|| {
            GateError("lints: more planted shapes than a count can hold".to_owned())
        })?;
    }
    Ok(found)
}

/// How many shapes of `planted` the lint scan found as `kind`, which is all of them or an error.
///
/// # Errors
/// The first shape the scan did not find as `kind`, or planted text that does not parse.
pub fn lint_sighted(kind: lint_scan::Kind, planted: &str) -> Result<usize, GateError> {
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
                .map_err(|error| GateError(error.to_string()))
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
    per_file: impl Fn(&str, &str) -> Result<Vec<K>, GateError>,
    whole: impl Fn(&Path) -> Result<Vec<K>, GateError>,
) -> Result<usize, GateError> {
    let Planted {
        gate,
        ref file,
        label,
        text,
    } = *planted;
    let shapes = crate::sentinel::shapes(text)
        .map_err(|error| GateError(format!("{gate}: {file}: {error}")))?;
    for shape in &shapes {
        let name = shape.name();
        let found = match shape {
            crate::sentinel::Shape::Source { path, text, .. } => {
                per_file(path, text).map_err(|GateError(error)| {
                    GateError(format!(
                        "{gate}: planted shape `{name}` of {label}: {error}"
                    ))
                })?
            }
            crate::sentinel::Shape::Tree { files, .. } => {
                let root = tempfile::tempdir().map_err(|error| {
                    GateError(format!("{gate}: a directory to plant {label} in: {error}"))
                })?;
                crate::sentinel::plant(root.path(), files).map_err(|error| {
                    GateError(format!(
                        "{gate}: planting shape `{name}` of {label}: {error}"
                    ))
                })?;
                whole(root.path())?
            }
        };
        if !found.contains(kind) {
            return Err(GateError(format!(
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
fn lint_findings(root: &Path) -> Result<(usize, Vec<lint_scan::Finding>), GateError> {
    let files = all_sources(root)?;
    let mut found = Vec::new();
    let mut sources = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        found.extend(
            lint_scan::scan_source(&label, &source)
                .map_err(|error| GateError(format!("{label}: {error}")))?,
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
        .map_err(|error| GateError(format!("cross-file Rust declarations: {error}")))?,
    );
    found.extend(
        lint_scan::manual_variant_lists_across(
            crates
                .iter()
                .map(|(scope, file, source)| (scope.as_str(), file.as_str(), source.as_str())),
        )
        .map_err(|error| GateError(format!("cross-file variant lists: {error}")))?,
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
fn wildcards(root: &Path, files: &[PathBuf]) -> Result<Vec<lint_scan::Finding>, GateError> {
    let (ours, open) = sets(root, files)?;
    let mut standing: Vec<Waived> = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        let mut grouped: BTreeMap<(String, String), Vec<lint_scan::Wildcard>> = BTreeMap::new();
        for one in lint_scan::wildcards_over(&source, &ours)
            .map_err(|error| GateError(format!("{label}: {error}")))?
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
) -> Result<(Vec<String>, BTreeMap<String, String>), GateError> {
    let mut ours: Vec<String> = Vec::new();
    let mut open: BTreeMap<String, String> = BTreeMap::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        ours.extend(
            lint_scan::declared_enums(&source)
                .map_err(|error| GateError(format!("{label}: {error}")))?,
        );
        for name in lint_scan::open_enums(&source)
            .map_err(|error| GateError(format!("{label}: {error}")))?
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
fn parted(entry: &str) -> Result<(&str, &str, &str, usize), GateError> {
    let Some((place, rest)) = entry.split_once(" over ") else {
        return Err(GateError(format!(
            "lints: malformed wildcard ledger entry {entry:?}"
        )));
    };
    let Some((over, how_many)) = rest.split_once(", ") else {
        return Err(GateError(format!(
            "lints: malformed wildcard ledger entry {entry:?}"
        )));
    };
    let Some(how_many) = how_many.strip_suffix(" arm(s)") else {
        return Err(GateError(format!(
            "lints: malformed wildcard ledger count in {entry:?}"
        )));
    };
    let how_many = how_many.parse::<usize>().map_err(|error| {
        GateError(format!(
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
fn waived_lines(root: &Path) -> Result<usize, GateError> {
    let how_many = counted(root, "xtask/wildcard_allowlist.txt")?.len();
    let ceiling = counted(root, "xtask/waiver_ceiling.txt")?;
    let [written] = ceiling.as_slice() else {
        return Err(GateError(
            "lints: xtask/waiver_ceiling.txt holds one number and nothing else.".to_owned(),
        ));
    };
    let Ok(most) = written.parse::<usize>() else {
        return Err(GateError(format!(
            "lints: xtask/waiver_ceiling.txt holds {written}, which is not a number."
        )));
    };
    if how_many > most {
        return Err(GateError(format!(
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
fn ceiling(root: &Path, relative: &str) -> Result<usize, GateError> {
    let held = counted(root, relative)?;
    let [written] = held.as_slice() else {
        return Err(GateError(format!(
            "{relative} holds one number and nothing else."
        )));
    };
    written.parse::<usize>().map_err(|_not_a_number| {
        GateError(format!(
            "{relative} holds {written}, which is not a number."
        ))
    })
}

/// The lines of a ledger that are not its header.
///
/// # Errors
/// The file cannot be read.
fn counted(root: &Path, relative: &str) -> Result<Vec<String>, GateError> {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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

fn ratcheted(root: &Path, standing: &[Waived]) -> Result<Vec<lint_scan::Finding>, GateError> {
    let path = root.join("xtask/wildcard_allowlist.txt");
    let ledger = std::fs::read_to_string(&path)
        .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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
        return Err(GateError(format!(
            "lints: xtask/wildcard_allowlist.txt names {how_many} line(s) that no longer \
             catch everything left of a set this repository closes. Take them out: a \
             ledger that keeps a waiver nobody needs is one nobody reads.{named}"
        )));
    }
    Ok(found)
}

/// joiners rather than reading the spelling.
fn loose_layouts(root: &Path, files: &[PathBuf]) -> Result<Vec<lint_scan::Finding>, GateError> {
    let mut layouts = Vec::new();
    let mut configured: Vec<String> = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
        let module = path
            .file_stem()
            .ok_or_else(|| GateError(format!("{} has no file stem", path.display())))?
            .to_str()
            .ok_or_else(|| GateError(format!("{} has a non-UTF-8 file stem", path.display())))?
            .to_owned();
        for (name, _line) in lint_scan::exported_strings(&source) {
            layouts.push((module.clone(), name));
        }
    }
    for path in production_sources(root)? {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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
fn tests_under(root: &Path) -> Result<Vec<PathBuf>, GateError> {
    Ok(crate::repository::files(root)?
        .into_iter()
        .filter(|relative| {
            in_source_roots(relative)
                && crate::repository::extension_is(relative, "rs")
                && relative.contains("/tests/")
        })
        .map(|relative| root.join(relative))
        .collect())
}

/// The production source files the seam ratchet scans.
///
/// # Errors
/// The repository cannot be listed; an incomplete source set proves no gate.
pub fn production_sources(root: &Path) -> Result<Vec<PathBuf>, GateError> {
    Ok(crate::repository::files(root)?
        .into_iter()
        .filter(|relative| {
            in_source_roots(relative)
                && crate::repository::extension_is(relative, "rs")
                && is_production(relative)
        })
        .map(|relative| root.join(relative))
        .collect())
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

fn relative_slash(root: &Path, path: &Path) -> Result<String, GateError> {
    let relative = path.strip_prefix(root).map_err(|error| {
        GateError(format!(
            "{} is outside {}: {error}",
            path.display(),
            root.display()
        ))
    })?;
    let relative = relative
        .to_str()
        .ok_or_else(|| GateError(format!("{} is not UTF-8", relative.display())))?;
    Ok(relative.replace('\\', "/"))
}

/// The seam ratchet against `xtask/seam_allowlist.txt`.
///
/// # Errors
/// Returns a disagreement between the scan and the ledger, or an unreadable file.
pub fn devgates(root: &Path) -> Result<String, GateError> {
    let planted = seam_sentinels()?;
    let (files, found) = seam_findings(root)?;
    let ledger_path = root.join("xtask/seam_allowlist.txt");
    let ledger_text = std::fs::read_to_string(&ledger_path)
        .map_err(|error| GateError(format!("{}: {error}", ledger_path.display())))?;
    let ledger = devgates::parse_ledger(&ledger_text).map_err(|error| GateError(error.coded()))?;
    devgates::compare(&found, &ledger)
        .map_err(|disagreement| GateError(disagreement.to_string()))?;
    let most = ceiling(root, "xtask/seam_ceiling.txt")?;
    if ledger.len() > most {
        return Err(GateError(format!(
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
fn seam_findings(root: &Path) -> Result<(usize, Vec<devgates::Seam>), GateError> {
    let mut found = Vec::new();
    let files = production_sources(root)?;
    for path in &files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path)?;
        let seams = devgates::scan_source(&label, &source)
            .map_err(|error| GateError(format!("{label}: {error}")))?;
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
pub fn seam_sentinels() -> Result<usize, GateError> {
    let mut found = 0_usize;
    for kind in devgates::SeamKind::ALL {
        let shapes = seam_sighted(kind, kind.planted())?;
        found = found.checked_add(shapes).ok_or_else(|| {
            GateError("devgates: more planted shapes than a count can hold".to_owned())
        })?;
    }
    Ok(found)
}

/// How many shapes of `planted` the seam scan found as `kind`, which is all of them or an error.
///
/// # Errors
/// The first shape the scan did not find as `kind`, or planted text that does not parse.
pub fn seam_sighted(kind: devgates::SeamKind, planted: &str) -> Result<usize, GateError> {
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
                .map_err(|error| GateError(error.to_string()))
        },
        |root| seam_findings(root).map(|(_files, found)| kinds(found)),
    )
}

/// Dependency direction between the workspace crates.
///
/// # Errors
/// Returns every edge the direction rule refuses, or a `cargo metadata` failure.
pub fn deps(root: &Path) -> Result<String, GateError> {
    let census = census(root)?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateError(format!("cargo metadata: {error}")))?;
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
        .map_err(|error| GateError(format!("cargo metadata for fuzz/Cargo.toml: {error}")))?;
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
    built_as_tested(root, &metadata, &members)?;
    if violations.is_empty() && prohibited.is_empty() {
        return Ok(format!(
            "deps: {} internal edges, all in the allowed direction, and every shipped \
             dependency is built as its tests build it; {census}",
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
    Err(GateError(message))
}

/// Nothing, where every direct dependency of a shipped crate is built with the features its tests build it with.
///
/// # Errors
/// A feature only a development edge turns on, or a `cargo tree` that could not be run.
fn built_as_tested(
    root: &Path,
    metadata: &cargo_metadata::Metadata,
    members: &[String],
) -> Result<(), GateError> {
    let direct: BTreeSet<String> = metadata
        .workspace_packages()
        .iter()
        .filter(|package| deps::SHIPPED.contains(&package.name.as_str()))
        .flat_map(|package| package.dependencies.iter())
        .filter(|dependency| {
            dependency.kind != cargo_metadata::DependencyKind::Development
                && !members.contains(&dependency.name)
        })
        .map(|dependency| dependency.name.clone())
        .collect();
    let ships = metadata
        .workspace_packages()
        .iter()
        .any(|package| deps::SHIPPED.contains(&package.name.as_str()));
    let tested_only = if ships {
        features_only_tests_build_with(root, &direct)?
    } else {
        Vec::new()
    };
    if !tested_only.is_empty() {
        let mut message = String::from(
            "deps: a shipped dependency is built with a feature only a development edge turns on, \
             so every test reads with it and no release does:\n",
        );
        for (package, features) in &tested_only {
            line(
                &mut message,
                format_args!("  {package}: {}", features.join(", ")),
            );
        }
        append(
            &mut message,
            format_args!("turn the feature on where the shipped crate depends on it"),
        );
        return Err(GateError(message));
    }
    Ok(())
}

/// Every feature a shipped dependency is built with only because a development edge asks for it, on any target a release builds.
///
/// # Errors
/// A `cargo tree` that could not be run or read.
fn features_only_tests_build_with(
    root: &Path,
    direct: &BTreeSet<String>,
) -> Result<Vec<(String, Vec<String>)>, GateError> {
    let tree = |target: &str, edges: &str| -> Result<String, GateError> {
        let mut args = vec![
            "tree", "--locked", "--color", "never", "--prefix", "none", "--format", "{p}|{f}",
            "--target", target, "--edges", edges,
        ];
        for shipped in deps::SHIPPED {
            args.extend(["--package", shipped]);
        }
        let asked = std::process::Command::new("cargo")
            .args(&args)
            .current_dir(root)
            .output()
            .map_err(|error| GateError(format!("cargo tree: {error}")))?;
        if !asked.status.success() {
            let stderr = std::str::from_utf8(&asked.stderr)
                .map_err(|error| GateError(format!("cargo tree stderr is not UTF-8: {error}")))?;
            return Err(GateError(format!("cargo tree: {}", stderr.trim())));
        }
        String::from_utf8(asked.stdout)
            .map_err(|error| GateError(format!("cargo tree stdout is not UTF-8: {error}")))
    };
    let mut found = Vec::new();
    for target in deps::SHIPPED_TARGETS {
        for (package, features) in deps::features_only_tests_build_with(
            (
                &tree(target, "normal,build")?,
                &tree(target, "normal,build,dev")?,
            ),
            direct,
        ) {
            found.push((format!("{package} on {target}"), features));
        }
    }
    Ok(found)
}

/// Every cargo manifest in the tree, classified, so a fourth kind cannot appear unnoticed.
///
/// A manifest belongs to the root workspace, to the fuzz workspace, or to a fixture, and each class has a command that reaches it: `cargo nextest run --workspace`, `cargo xtask fuzz-clippy`, and the fate suite.
/// `fuzz/` was the one workspace nothing compiled while testing anything, and it rotted; `compiler-surfaces` arrived as a fourth root and the seam ratchet did not see it for a campaign.
/// What a command reaches is a fact about this tree, and a fact about this tree is something a gate can hold.
///
/// # Errors
/// A manifest that belongs to none of the three, or metadata that could not be read.
fn census(root: &Path) -> Result<String, GateError> {
    let members: BTreeSet<PathBuf> = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateError(format!("cargo metadata: {error}")))?
        .workspace_packages()
        .iter()
        .map(|package| PathBuf::from(package.manifest_path.as_std_path()))
        .collect();
    let fuzz = root.join("fuzz");
    let fixtures = root.join("fixtures");
    let mut counted = [0_usize; 3];
    let mut loose = Vec::new();
    for relative in crate::repository::files(root)? {
        if !(relative == "Cargo.toml" || relative.ends_with("/Cargo.toml"))
            || relative.starts_with('.')
        {
            continue;
        }
        let whole = root.join(&relative);
        let path = whole.as_path();
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
        return Err(GateError(format!(
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
pub fn fixtures(root: &Path) -> Result<String, GateError> {
    let dir = root.join("fixtures");
    let names = fixtures::discover(&dir)
        .map_err(|error| GateError(format!("{}: {error}", dir.display())))?;
    let mut problems = Vec::new();
    for name in &names {
        for problem in fixtures::check_fixture(&dir.join(name))
            .map_err(|error| GateError(format!("fixtures/{name}: {error}")))?
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
    Err(GateError(format!(
        "fixtures: {}\n{}",
        problems.join("\n"),
        fixtures::RULE
    )))
}

/// Version consistency between the workspace and the release manifest.
///
/// # Errors
/// Returns every inconsistency.
pub fn release_check(root: &Path) -> Result<String, GateError> {
    let read = |relative: &str| {
        std::fs::read_to_string(root.join(relative))
            .map_err(|error| GateError(format!("{relative}: {error}")))
    };
    let workspace = read("Cargo.toml")?;
    let mut members = Vec::new();
    for label in crate::repository::files(root)? {
        let depth = label.split('/').count();
        if label.starts_with("crates/") && label.ends_with("/Cargo.toml") && depth <= 3 {
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
    Err(GateError(format!(
        "release-check:\n  {}",
        problems.join("\n  ")
    )))
}

/// Every milestone-shaped name in the book resolves to exactly one row in the roadmap.
///
/// # Errors
/// The roadmap registry is malformed, a page cannot be read, or a page names a milestone the registry does not declare.
pub fn milestones(root: &Path) -> Result<String, GateError> {
    let roadmap_path = root.join("docs/roadmap.md");
    let roadmap = std::fs::read_to_string(&roadmap_path)
        .map_err(|error| GateError(format!("{}: {error}", roadmap_path.display())))?;
    let registered = crate::milestones::registry(&roadmap)
        .map_err(|error| GateError(format!("milestones: docs/roadmap.md: {error}")))?;
    let mut unresolved = Vec::new();
    let mut pages = 0usize;
    for page in crate::repository::files(root)? {
        if !(page.starts_with("docs/") && crate::repository::extension_is(&page, "md")) {
            continue;
        }
        pages = pages.saturating_add(1);
        let text = std::fs::read_to_string(root.join(&page))
            .map_err(|error| GateError(format!("{page}: {error}")))?;
        for reference in crate::milestones::references(&text) {
            if !registered.contains(&reference) {
                unresolved.push(format!("{page} names {reference}"));
            }
        }
    }
    unresolved.sort();
    unresolved.dedup();
    if !unresolved.is_empty() {
        return Err(GateError(format!(
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
pub fn surfaces(root: &Path) -> Result<String, GateError> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateError(format!("cargo metadata: {error}")))?;
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
            .map_err(|error| GateError(format!("{} library target: {error}", package.name)))?;
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
        return Err(GateError(format!(
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
) -> Result<Vec<crate::surface::Harness>, GateError> {
    package
        .targets
        .iter()
        .filter(|target| target.is_bin())
        .map(|target| {
            let source = std::fs::read_to_string(target.src_path.as_std_path())
                .map_err(|error| GateError(format!("{}: {error}", target.src_path)))?;
            let product = crate::surface::harness_product(&source, target.src_path.as_std_path())
                .map_err(|error| GateError(format!("{}: invalid Rust: {error}", target.src_path)))?
                .map(|path| {
                    std::fs::canonicalize(&path)
                        .map_err(|error| GateError(format!("{}: {error}", path.display())))
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

/// The decision records against their numbers, their headings, the book's list, and every name of one in the tree.
///
/// # Errors
/// The first record that does not hold together, or every name of a record that no record has.
pub fn adrs(root: &Path) -> Result<String, GateError> {
    let repository = crate::repository::files(root)?;
    let mut files = Vec::new();
    for relative in &repository {
        let Some(name) = relative.strip_prefix("docs/adr/") else {
            continue;
        };
        if let Some((directory, _inside)) = name.split_once('/') {
            return Err(GateError(format!(
                "adrs: docs/adr/{directory} is not a file, and the directory holds decision records only"
            )));
        }
        let text = std::fs::read_to_string(root.join(relative))
            .map_err(|error| GateError(format!("{relative}: {error}")))?;
        files.push((name.to_owned(), text));
    }
    let records = crate::adrs::records(&files)
        .map_err(|error| GateError(format!("adrs: {}", error.coded())))?;
    let book = root.join("docs/SUMMARY.md");
    let listed = std::fs::read_to_string(&book)
        .map_err(|error| GateError(format!("{}: {error}", book.display())))?;
    crate::adrs::summary(&listed, &records)
        .map_err(|error| GateError(format!("adrs: {}", error.coded())))?;
    let mut dangling = Vec::new();
    let mut pages = 0_usize;
    for page in &repository {
        let read = !page.starts_with('.')
            && !page.split('/').any(|part| part.starts_with('.'))
            && (crate::repository::extension_is(page, "md")
                || crate::repository::extension_is(page, "rs"));
        if !read {
            continue;
        }
        pages = pages.saturating_add(1);
        let text = std::fs::read_to_string(root.join(page))
            .map_err(|error| GateError(format!("{page}: {error}")))?;
        dangling.extend(
            crate::adrs::dangling(page, &text, &records)
                .iter()
                .map(crate::error::Coded::coded),
        );
    }
    if !dangling.is_empty() {
        return Err(GateError(format!(
            "adrs: these name a decision record that is not there:\n  {}",
            dangling.join("\n  ")
        )));
    }
    Ok(format!(
        "adrs: {} decision records, one number each and each listed once in the book under it, and \
         {pages} files name no other",
        records.len()
    ))
}

/// Every variable that points git at a repository other than the one it is standing in, which a hook or a wrapper may have set.
pub const REDIRECTING_GIT: [&str; 10] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// Whether nothing a build writes is committed: no path git tracks lies under a directory named `target`.
///
/// # Errors
/// The detector misses planted build output or refuses a lookalike, git cannot list what it tracks, or something under a `target` directory is tracked.
pub fn tracked(root: &Path) -> Result<String, GateError> {
    let planted: Vec<&str> = PLANTED_BUILT
        .iter()
        .chain(PLANTED_SOURCE.iter())
        .copied()
        .collect();
    if built(&planted) != PLANTED_BUILT {
        return Err(GateError(format!(
            "tracked: the detector does not find exactly the planted build output among {}, so \
             its silence about the tree would not be evidence",
            planted.join(", ")
        )));
    }
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(root).args(["ls-files", "-z"]);
    for variable in REDIRECTING_GIT {
        git.env_remove(variable);
    }
    let listed = git
        .output()
        .map_err(|error| GateError(format!("tracked: git ls-files could not run: {error}")))?;
    if !listed.status.success() {
        let said = match String::from_utf8(listed.stderr) {
            Ok(said) => said,
            Err(_not_text) => "its diagnostics are not text".to_owned(),
        };
        return Err(GateError(format!(
            "tracked: git ls-files failed, so what the repository commits is unknown: {said}"
        )));
    }
    let listed = String::from_utf8(listed.stdout).map_err(|_not_text| {
        GateError(
            "tracked: git ls-files printed a path that is not UTF-8, which no path this \
             repository commits is"
                .to_owned(),
        )
    })?;
    let paths: Vec<&str> = listed.split('\0').filter(|path| !path.is_empty()).collect();
    let found = built(&paths);
    if !found.is_empty() {
        let mut directories: BTreeMap<&str, usize> = BTreeMap::new();
        for path in &found {
            let count = directories.entry(written_into(path)).or_default();
            *count = count.saturating_add(1);
        }
        let named: Vec<String> = directories
            .iter()
            .map(|(directory, count)| format!("{directory}/ ({count})"))
            .collect();
        return Err(GateError(format!(
            "tracked: {} committed path(s) lie under a `target` directory, which a build wrote \
             and the next build writes again: {}. `git rm -r --cached` on each directory takes \
             them out of the index, and `.gitignore` keeps every `target/` out after that",
            found.len(),
            named.join(", ")
        )));
    }
    Ok(format!(
        "tracked: {} planted build outputs found first and {} lookalikes passed; then {} \
         tracked paths read, none under a `target` directory",
        PLANTED_BUILT.len(),
        PLANTED_SOURCE.len(),
        paths.len()
    ))
}

/// Build output the detector has to find before its silence about a tree is believed.
const PLANTED_BUILT: [&str; 3] = [
    "target/debug/build/x.o",
    "crates/njutest-macros/target/tests/trybuild/CACHEDIR.TAG",
    "fixtures/fixture-simple/target/.rustc_info.json",
];

/// Sources the detector has to pass, each one only looking like build output.
const PLANTED_SOURCE: [&str; 3] = [
    "fixtures/fixture-targets/src/lib.rs",
    "crates/njutest/src/targets.rs",
    "docs/target",
];

/// The paths of `paths` with a directory named `target` above them, which is where cargo and everything it runs write.
fn built<'a>(paths: &[&'a str]) -> Vec<&'a str> {
    paths
        .iter()
        .copied()
        .filter(|path| {
            let mut directories = path.split('/').rev().skip(1);
            directories.any(|directory| directory == "target")
        })
        .collect()
}

/// The `target` directory `path` was written into: everything up to its first one.
fn written_into(path: &str) -> &str {
    let mut end: usize = 0;
    for directory in path.split('/') {
        end = end.saturating_add(directory.len());
        if directory == "target" {
            break;
        }
        end = end.saturating_add(1);
    }
    path.get(..end).unwrap_or(path)
}

/// A name no target of any workspace has, which the target check must refuse before its silence about the configuration counts.
const PLANTED_UNKNOWN_TARGET: &str = "xtask/test/no-such-target-anywhere";

/// Every target `.rust-mutants.toml` skips is one a member declares, as cargo's metadata names it.
///
/// # Errors
/// The configuration cannot be read, cargo's metadata cannot be read, the check misses a planted name, or a name is no target.
pub fn skipped(root: &Path) -> Result<String, GateError> {
    let path = root.join(".rust-mutants.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| GateError(format!("skipped: {}: {error}", path.display())))?;
    let config = text
        .parse::<toml::Table>()
        .map_err(|error| GateError(format!("skipped: {}: {error}", path.display())))?;
    let named = skip_targets_of(&config, &path)?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateError(format!("skipped: cargo metadata: {error}")))?;
    let declared = declared_targets(&metadata);
    let every_package_named = metadata.workspace_packages().iter().all(|package| {
        let prefix = format!("{}/", package.name);
        declared.iter().any(|one| one.starts_with(&prefix))
    });
    if declared.contains(PLANTED_UNKNOWN_TARGET) || !every_package_named {
        return Err(GateError(format!(
            "skipped: the targets derived from cargo's metadata do not refuse \
             {PLANTED_UNKNOWN_TARGET} and name a target of every member, so their silence about \
             the configuration would not be evidence"
        )));
    }
    let unknown: Vec<&String> = named
        .iter()
        .filter(|name| !declared.contains(name.as_str()))
        .collect();
    if let Some(first) = unknown.first() {
        let package = match first.split_once('/') {
            Some((package, _rest)) => package,
            None => first.as_str(),
        };
        let nearby: Vec<&str> = declared
            .iter()
            .filter(|one| one.starts_with(&format!("{package}/")))
            .map(String::as_str)
            .collect();
        return Err(GateError(format!(
            "skipped: {} names {}, which no member of this workspace declares; a run refuses it \
             with RM5004 before it measures anything. {package} declares: {}",
            path.display(),
            unknown
                .iter()
                .map(|one| one.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            nearby.join(", ")
        )));
    }
    Ok(format!(
        "skipped: {} skipped target(s), each one this workspace declares among its {}, and a planted unknown name refused first",
        named.len(),
        declared.len()
    ))
}

/// The strings `[execution] skip_targets` holds, or none where the table or the key is absent.
fn skip_targets_of(config: &toml::Table, path: &Path) -> Result<Vec<String>, GateError> {
    let refused = |what: String| GateError(format!("skipped: {}: {what}", path.display()));
    let Some(execution) = config.get("execution") else {
        return Ok(Vec::new());
    };
    let Some(table) = execution.as_table() else {
        return Err(refused("`execution` is not a table".to_owned()));
    };
    let Some(listed) = table.get("skip_targets") else {
        return Ok(Vec::new());
    };
    let Some(listed) = listed.as_array() else {
        return Err(refused(
            "`execution.skip_targets` is not an array".to_owned(),
        ));
    };
    listed
        .iter()
        .map(|one| match one.as_str() {
            Some(name) => Ok(name.to_owned()),
            None => Err(refused(format!(
                "`execution.skip_targets` holds {one}, which is not a string"
            ))),
        })
        .collect()
}

/// Every target the engine could start in this workspace, named as it names them: `package/kind/name`, and `package/doc/name` for a library whose documentation cargo tests.
fn declared_targets(metadata: &cargo_metadata::Metadata) -> BTreeSet<String> {
    let mut declared = BTreeSet::new();
    for package in metadata.workspace_packages() {
        for target in &package.targets {
            let kind = if target.is_custom_build() || target.is_bench() {
                None
            } else if target.is_proc_macro() {
                Some("proc-macro")
            } else if target.is_lib() {
                Some("lib")
            } else if target.is_bin() {
                Some("bin")
            } else if target.is_test() {
                Some("test")
            } else if target.is_example() {
                Some("example")
            } else {
                None
            };
            if let Some(kind) = kind {
                declared.insert(format!("{}/{kind}/{}", package.name, target.name));
            }
            if target.is_lib() && !target.is_proc_macro() && target.doctest {
                declared.insert(format!("{}/doc/{}", package.name, target.name));
            }
        }
    }
    declared
}

/// Every critical decision has a row saying what holds it at every layer, each cell naming an item the tree defines, and every open layer is a hole the gaps ledger gives an owner.
///
/// # Errors
/// The registry or ledger cannot be read or is malformed, or they and the tree disagree.
pub fn invariants(root: &Path) -> Result<String, GateError> {
    let read = |relative: &str| {
        std::fs::read_to_string(root.join(relative))
            .map_err(|error| GateError(format!("invariants: {relative}: {error}")))
    };
    let coded = |error: crate::invariants::InvariantError| {
        GateError(format!("invariants: {}", error.coded()))
    };
    let rows = crate::invariants::rows(&read("docs/invariants.md")?).map_err(coded)?;
    let gaps = crate::invariants::gaps(&read("xtask/invariant_gaps.txt")?).map_err(coded)?;
    let mut defined = BTreeSet::new();
    for path in all_sources(root)? {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| GateError(format!("invariants: {}: {error}", path.display())))?;
        defined.extend(crate::invariants::defined(&text));
    }
    let (decisions, held) =
        crate::invariants::check(&rows, &gaps, &defined).map_err(|refused| {
            GateError(format!(
                "invariants: the registry and the tree disagree:\n  {}",
                refused
                    .iter()
                    .map(crate::error::Coded::coded)
                    .collect::<Vec<_>>()
                    .join("\n  ")
            ))
        })?;
    Ok(format!(
        "invariants: {decisions} critical decisions, {held} layer cells each naming what the tree \
         defines, and {} open, each owned in xtask/invariant_gaps.txt; `ratchets` holds them to \
         the base",
        gaps.len()
    ))
}

/// The ledgers whose header says they may shrink and never grow, each one number.
const NEVER_GROW: [&str; 2] = ["xtask/waiver_ceiling.txt", "xtask/seam_ceiling.txt"];

/// Where this change meets `origin/main`, which is what everything that may never grow is held to.
struct Base {
    commit: String,
    said: String,
}

/// What `git` answers in `root`, or a refusal that names what was asked and how to give it what it needs.
///
/// # Errors
/// Git could not be started, or answered with a failure.
fn git_answer(root: &Path, args: &[&str]) -> Result<String, GateError> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| GateError(format!("ratchets: git could not run: {error}")))?;
    if !output.status.success() {
        return Err(GateError(format!(
            "ratchets: `git {}` failed, so what this change is compared with is unknown; fetch \
             origin/main (CI fetches it whole): {}",
            args.join(" "),
            match std::str::from_utf8(&output.stderr) {
                Ok(said) => said.trim(),
                Err(_not_text) => "it said something that is not text",
            }
        )));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|_not_text| {
            GateError(format!(
                "ratchets: `git {}` answered in no text",
                args.join(" ")
            ))
        })
}

impl Base {
    /// The merge base of `HEAD` with `origin/main`, named the way a person checks it against CI's.
    ///
    /// # Errors
    /// There is no Git, no `origin/main`, or no commit both reach.
    fn of(root: &Path) -> Result<Self, GateError> {
        let commit = git_answer(root, &["merge-base", "HEAD", "origin/main"])?;
        let said = git_answer(root, &["log", "-1", "--format=%h of %cs", &commit])?;
        Ok(Self { commit, said })
    }

    /// What `relative` held at the base, or nothing where it did not exist there.
    ///
    /// # Errors
    /// Git could not say.
    fn read(&self, root: &Path, relative: &str) -> Result<Option<String>, GateError> {
        let named = format!("{}:{relative}", self.commit);
        let present = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["cat-file", "-e", &named])
            .output()
            .map_err(|error| GateError(format!("ratchets: git could not run: {error}")))?;
        if !present.status.success() {
            return Ok(None);
        }
        git_answer(root, &["show", &named]).map(Some)
    }
}

/// Everything this repository says may shrink and never grow, held to what it was where this change meets `origin/main` rather than to a bound the same change can raise.
///
/// # Errors
/// The base cannot be found, a ledger or the registry cannot be read at either end, or something grew or fell back.
pub fn ratchets(root: &Path) -> Result<String, GateError> {
    let base = Base::of(root)?;
    let mut refused = Vec::new();
    for ledger in NEVER_GROW {
        let now = ceiling(root, ledger)?;
        let Some(then) = base.read(root, ledger)? else {
            continue;
        };
        let then = held_number(ledger, &then)?;
        if now > then {
            refused.push(format!(
                "{ledger} holds {now}, and held {then} at the base: it may shrink and never grow, \
                 and a bound the same change raises bounds nothing"
            ));
        }
    }
    let registry = "docs/invariants.md";
    if let Some(before) = base.read(root, registry)? {
        let coded = |error: crate::invariants::InvariantError| {
            GateError(format!("ratchets: {}", error.coded()))
        };
        let head = std::fs::read_to_string(root.join(registry))
            .map_err(|error| GateError(format!("ratchets: {registry}: {error}")))?;
        let now = crate::invariants::rows(&head).map_err(coded)?;
        let then = crate::invariants::rows(&before).map_err(coded)?;
        refused.extend(
            crate::invariants::regressions(&then, &now)
                .iter()
                .map(crate::error::Coded::coded),
        );
    }
    if !refused.is_empty() {
        return Err(GateError(format!(
            "ratchets: compared with {} where this change meets origin/main:\n  {}",
            base.said,
            refused.join("\n  ")
        )));
    }
    Ok(format!(
        "ratchets: {} and the registry's held layers, none grown or fallen back since {} where \
         this change meets origin/main (fetch origin/main to compare with CI's base)",
        NEVER_GROW.join(" and "),
        base.said
    ))
}

/// The one number a ledger's text at the base holds.
///
/// # Errors
/// The text holds anything but one number.
fn held_number(relative: &str, text: &str) -> Result<usize, GateError> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let [written] = lines.as_slice() else {
        return Err(GateError(format!(
            "ratchets: {relative} at the base holds something other than one number"
        )));
    };
    written.parse::<usize>().map_err(|_not_a_number| {
        GateError(format!(
            "ratchets: {relative} at the base holds {written}, which is not a number"
        ))
    })
}

/// Every gate, in order, stopping at the first failure.
///
/// # Errors
/// Returns the first gate's failure.
pub fn all(root: &Path) -> Result<String, GateError> {
    let mut report = String::new();
    for gate in [
        devgates,
        lints,
        deps,
        fixtures,
        release_check,
        milestones,
        adrs,
        invariants,
        ratchets,
        surfaces,
        reached,
        defaulted,
        waivers,
        tracked,
        skipped,
        crate::claims::claims,
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
    checkers: &crate::schemas::Checkers,
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
    let kept = recordings(trace)?;
    proofaudit::audit_with(
        checkers,
        proofaudit::Reported {
            path: &label,
            text: &text,
        },
        kept.recorded(),
        Some(run),
    )
}

/// The runner's recording and every configured build's engine recording one run kept, each as its path and its text.
struct Recordings {
    runner: Option<(String, String)>,
    engines: Vec<(String, String)>,
    outputs: Vec<(String, proofaudit::soundness::Kept)>,
}

impl Recordings {
    /// What the audit reads of them.
    fn recorded(&self) -> proofaudit::Recorded<'_> {
        proofaudit::Recorded {
            runner: self
                .runner
                .as_ref()
                .map(|(recording_path, text)| (recording_path.as_str(), text.as_str())),
            engines: &self.engines,
            outputs: &self.outputs,
        }
    }
}

/// The runner's recording and every configured build's engine recording under `trace`, each as its path and its text; nothing where no recording was given.
fn recordings(trace: Option<&Path>) -> Result<Recordings, proofaudit::AuditError> {
    let runner = trace
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
    let outputs = match (trace, runner.as_ref()) {
        (Some(directory), Some((_path, text))) => kept_outputs(directory, text),
        (None, _) | (_, None) => Vec::new(),
    };
    Ok(Recordings {
        runner,
        engines,
        outputs,
    })
}

/// Whether the merged report at `merged` is the merge of the shards `shards`, each re-decided against its recording under `traces`.
///
/// # Errors
/// A document that cannot be read, is not JSON or is off its schema, a report that is not a merge, a shard given twice, and a shard the report was not merged from.
pub fn proofaudit_merged(
    checkers: &crate::schemas::Checkers,
    merged: &Path,
    shards: &[PathBuf],
    traces: Option<&Path>,
) -> Result<proofaudit::Audit, proofaudit::AuditError> {
    let merged = assurance_document(merged)?;
    let audited = shards
        .iter()
        .map(|shard| {
            let document = assurance_document(shard)?;
            let run_id = proofaudit::merge::shard_run(checkers, &document.label, &document.text)?;
            let trace = traces.map(|directory| directory.join(&run_id));
            let kept = recordings(trace.as_deref())?;
            proofaudit::merge::audited(
                checkers,
                proofaudit::Reported {
                    path: &document.label,
                    text: &document.text,
                },
                kept.recorded(),
                document.run.as_deref(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    proofaudit::merge::merged_with(checkers, &merged.label, &merged.text, &audited)
}

/// An assurance document as the audit reads it: where it is, what it says, and the run directory holding it when one was named.
struct AssuranceDocument {
    label: String,
    text: String,
    run: Option<PathBuf>,
}

/// The assurance document at `path`, or in the run directory `path` names; a symbolic link is refused rather than followed.
fn assurance_document(path: &Path) -> Result<AssuranceDocument, proofaudit::AuditError> {
    let unreadable = |at: &Path, source: std::io::Error| proofaudit::AuditError::Unreadable {
        path: at.display().to_string(),
        source,
    };
    let metadata = std::fs::symlink_metadata(path).map_err(|source| unreadable(path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(unreadable(
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "an assurance document path must not be a symbolic link",
            ),
        ));
    }
    let (document, run) = if metadata.is_dir() {
        (path.join(proofaudit::REPORT_FILE), Some(path.to_path_buf()))
    } else {
        (path.to_path_buf(), None)
    };
    let text =
        std::fs::read_to_string(&document).map_err(|source| unreadable(&document, source))?;
    Ok(AssuranceDocument {
        label: document.display().to_string(),
        text,
        run,
    })
}

/// What the recording kept of each interpreter run in the runner's recording `text`, read from beside it in `directory` and held to the size and digest its exec record gives; a copy that is cut, missing, or unreadable is left out, which the audit says it could not re-derive.
fn kept_outputs(directory: &Path, text: &str) -> Vec<(String, proofaudit::soundness::Kept)> {
    let mut kept = Vec::new();
    for line in text.lines() {
        let Ok(event) = crate::strictjson::from_str(line) else {
            continue;
        };
        let Some(exec) = event.pointer("/payload/exec") else {
            continue;
        };
        if !proofaudit::soundness::interprets(exec)
            && !exec
                .get("argv")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|argv| argv.iter().any(|word| word == "--version"))
        {
            continue;
        }
        if exec
            .get("output_truncated")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        {
            continue;
        }
        let Some(relative) = exec.get("output_path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        match std::fs::read(directory.join(relative)) {
            Ok(bytes) => kept.push((relative.to_owned(), held_to(exec, bytes))),
            Err(_not_kept) => {}
        }
    }
    kept
}

/// A kept output held to the size and digest `exec` gives it.
fn held_to(exec: &serde_json::Value, bytes: Vec<u8>) -> proofaudit::soundness::Kept {
    use sha2::Digest as _;
    let digest = hex::encode(sha2::Sha256::digest(&bytes));
    let size = match u64::try_from(bytes.len()) {
        Ok(size) => Some(size),
        Err(_beyond_any_record) => None,
    };
    let described = exec
        .get("output_sha256")
        .and_then(serde_json::Value::as_str)
        == Some(digest.as_str())
        && exec.get("output_bytes").and_then(serde_json::Value::as_u64) == size;
    if !described {
        return proofaudit::soundness::Kept::Mismatched;
    }
    match String::from_utf8(bytes) {
        Ok(text) => proofaudit::soundness::Kept::Whole(text),
        Err(_not_text) => proofaudit::soundness::Kept::NotText,
    }
}

/// Every configured build's engine recording under a runner recording, in namespace order, each as its path and its text.
fn engine_recordings(trace: &Path) -> Result<Vec<(String, String)>, proofaudit::AuditError> {
    let builds = trace.join("builds");
    let unreadable = |path: &Path, source: std::io::Error| proofaudit::AuditError::Unreadable {
        path: path.display().to_string(),
        source,
    };
    let namespaces = match crate::repository::entries(&builds) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(unreadable(&builds, source)),
    };
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
pub fn proofaudit_sentinels(checkers: &crate::schemas::Checkers) -> Result<usize, GateError> {
    let clean = proofaudit::sentinel::clean();
    let merged = proofaudit::sentinel::sharded_clean().map_err(|error| {
        GateError(format!(
            "proofaudit: the clean specimen cannot be measured in shards: {error}"
        ))
    })?;
    for specimen in [&clean, &merged] {
        silent(checkers, specimen)?;
    }
    let mut found = 0_usize;
    for layer in proofaudit::Layer::ALL {
        let planted = proofaudit_sighted(checkers, layer, &layer.planted())?;
        found = found.checked_add(planted).ok_or_else(|| {
            GateError("proofaudit: more planted defects than a count can hold".to_owned())
        })?;
    }
    for rule in proofaudit::merge::MergeRule::ALL {
        merge_rule_sighted(checkers, rule)?;
    }
    for rule in crate::confirm::ConfirmRule::ALL {
        confirm_rule_sighted(checkers, rule)?;
    }
    Ok(found)
}

/// Nothing, where every defect planted for `rule` draws a confirmation violation of that rule by name.
///
/// # Errors
/// A rule with nothing planted for it, or a plant no violation of its rule names.
fn confirm_rule_sighted(
    checkers: &crate::schemas::Checkers,
    rule: crate::confirm::ConfirmRule,
) -> Result<(), GateError> {
    let planted = proofaudit::sentinel::confirm_plants(rule);
    if planted.is_empty() {
        return Err(GateError(format!(
            "proofaudit: the confirmation rule {} has nothing planted for it",
            rule.label()
        )));
    }
    let prefix = format!("{}: ", rule.label());
    for plant in &planted {
        let audit = proofaudit_specimen(checkers, plant)?;
        if !audit.remarks.iter().any(|remark| {
            remark.layer == proofaudit::Layer::Confirmations
                && remark.standing == proofaudit::Standing::Violated
                && remark.detail.starts_with(&prefix)
        }) {
            return Err(GateError(format!(
                "proofaudit: the confirmation rule {label} is blind. Its planted defect `{name}` drew no violation of it, so a run it is silent about says nothing. Nothing this gate would have said is believed until the planted defect is found again.\n{audit}",
                label = rule.label(),
                name = plant.name,
            )));
        }
    }
    Ok(())
}

/// Nothing, where the defect planted for `rule` draws a merge violation of that rule by name.
///
/// # Errors
/// A plant that cannot be built or read, or one no violation of its rule names.
fn merge_rule_sighted(
    checkers: &crate::schemas::Checkers,
    rule: proofaudit::merge::MergeRule,
) -> Result<(), GateError> {
    let plant = proofaudit::sentinel::merge_plant(rule).map_err(|error| {
        GateError(format!(
            "proofaudit: the merge rule {} has no defect planted for it: {error}",
            rule.label()
        ))
    })?;
    let audit = proofaudit_specimen(checkers, &plant)?;
    let prefix = format!("{}: ", rule.label());
    if audit.remarks.iter().any(|remark| {
        remark.layer == proofaudit::Layer::Merge
            && remark.standing == proofaudit::Standing::Violated
            && remark.detail.starts_with(&prefix)
    }) {
        Ok(())
    } else {
        Err(GateError(format!(
            "proofaudit: the merge rule {label} is blind. Its planted defect `{name}` drew no \
             violation of it, so a merge it is silent about says nothing. Nothing this gate would \
             have said is believed until the planted defect is found again.\n{audit}",
            label = rule.label(),
            name = plant.name,
        )))
    }
}

/// Nothing, where no layer finds anything in the clean `specimen`.
///
/// # Errors
/// A violation in it, which means that layer fires on anything.
fn silent(
    checkers: &crate::schemas::Checkers,
    specimen: &proofaudit::sentinel::Perturbation,
) -> Result<(), GateError> {
    let audit = proofaudit_specimen(checkers, specimen)?;
    if audit.violations() > 0 {
        return Err(GateError(format!(
            "proofaudit: the clean specimen `{}` draws {} violation(s), so a layer that fires on \
             it fires on anything and its violations about a real run say nothing. Nothing this \
             gate would have said is believed until the clean specimen is silent again.\n{audit}",
            specimen.name,
            audit.violations()
        )));
    }
    Ok(())
}

/// How many of `planted` the proof audit found as a violation of `layer`, which is all of them or an error.
///
/// # Errors
/// The first perturbation `layer` found nothing in, an empty `planted`, or a specimen that could not be laid out or read.
pub fn proofaudit_sighted(
    checkers: &crate::schemas::Checkers,
    layer: proofaudit::Layer,
    planted: &[proofaudit::sentinel::Perturbation],
) -> Result<usize, GateError> {
    if planted.is_empty() {
        return Err(GateError(format!(
            "proofaudit: the {} layer is blind. Nothing is planted for it, so its silence \
             about a real run says nothing. Nothing this gate would have said is believed \
             until a defect is planted for it and found.",
            layer.label()
        )));
    }
    for perturbation in planted {
        let audit = proofaudit_specimen(checkers, perturbation)?;
        if !audit.violated(layer) {
            return Err(GateError(format!(
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
    checkers: &crate::schemas::Checkers,
    specimen: &proofaudit::sentinel::Perturbation,
) -> Result<proofaudit::Audit, GateError> {
    let name = specimen.name;
    let laid = specimen
        .lay()
        .map_err(|error| GateError(format!("proofaudit: specimen `{name}`: {error}")))?;
    let shards: Vec<PathBuf> = laid.shards().into_iter().map(Path::to_path_buf).collect();
    if shards.is_empty() {
        proofaudit(checkers, laid.run(), laid.trace())
    } else {
        proofaudit_merged(checkers, laid.run(), &shards, laid.traces())
    }
    .map_err(|error| GateError(format!("proofaudit: specimen `{name}`: {error}")))
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
    /// The tree the run measured, which the carry evidence is read again from.
    pub root: Option<&'a Path>,
}

/// Re-decides one completed engine run from its own report, recording, and ledger.
///
/// # Errors
/// The report that is not there, is not JSON, or is not a run report.
pub fn engine_audit(
    checkers: &crate::schemas::Checkers,
    asked: &EngineRun<'_>,
) -> Result<engineaudit::Audit, engineaudit::AuditError> {
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
    let skeletons = read_optional_engine_document(&asked.run.join("skeletons-v1.json"))?;
    let carried = read_optional_engine_document(&asked.run.join("carried-v1.json"))?;
    let probe_logs = read_probe_logs(&asked.run.join("probe"))?;
    engineaudit::audit(
        checkers,
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
            skeletons: skeletons
                .as_ref()
                .map(|(path, text)| engineaudit::Source { path, text }),
            carried: carried
                .as_ref()
                .map(|(path, text)| engineaudit::Source { path, text }),
            root: asked.root,
        },
    )
}

/// How many planted defects every layer of the engine audit found, after the clean specimen drew nothing from any of them.
///
/// # Errors
/// A clean specimen some layer finds a violation in, which means that layer fires on anything, or the first layer that did not find a defect planted for it.
pub fn engine_audit_sentinels(checkers: &crate::schemas::Checkers) -> Result<usize, GateError> {
    let clean = engineaudit::sentinel::clean();
    let audit = engine_audit_specimen(checkers, &clean)?;
    if audit.violations() > 0 {
        return Err(GateError(format!(
            "engine-audit: the clean specimen draws {} violation(s), so a layer that fires on it \
             fires on anything and its violations about a real run say nothing. Nothing this \
             gate would have said is believed until the clean specimen is silent again.\n{audit}",
            audit.violations()
        )));
    }
    let mut found = 0_usize;
    for layer in engineaudit::Layer::ALL {
        let planted = engine_audit_sighted(checkers, layer, &layer.planted())?;
        found = found.checked_add(planted).ok_or_else(|| {
            GateError("engine-audit: more planted defects than a count can hold".to_owned())
        })?;
    }
    Ok(found)
}

/// How many of `planted` the audit found as a violation of `layer`, which is all of them or an error.
///
/// # Errors
/// The first perturbation `layer` found nothing in, an empty `planted`, or a specimen that could not be laid out or read.
pub fn engine_audit_sighted(
    checkers: &crate::schemas::Checkers,
    layer: engineaudit::Layer,
    planted: &[engineaudit::sentinel::Perturbation],
) -> Result<usize, GateError> {
    if planted.is_empty() {
        return Err(GateError(format!(
            "engine-audit: the {} layer is blind. Nothing is planted for it, so its silence \
             about a real run says nothing. Nothing this gate would have said is believed \
             until a defect is planted for it and found.",
            layer.label()
        )));
    }
    for perturbation in planted {
        let audit = engine_audit_specimen(checkers, perturbation)?;
        if !audit.violated(layer) {
            return Err(GateError(format!(
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
    checkers: &crate::schemas::Checkers,
    specimen: &engineaudit::sentinel::Perturbation,
) -> Result<engineaudit::Audit, GateError> {
    let name = specimen.name;
    let laid = specimen
        .lay()
        .map_err(|error| GateError(format!("engine-audit: specimen `{name}`: {error}")))?;
    engine_audit(
        checkers,
        &EngineRun {
            run: laid.run(),
            trace: Some(laid.trace()),
            shards: laid.shards(),
            ledger: laid.ledger(),
            sites: true,
            root: laid.root(),
        },
    )
    .map_err(|error| GateError(format!("engine-audit: specimen `{name}`: {error}")))
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
    let entries = match crate::repository::entries(path) {
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
        .into_iter()
        .map(|entry| {
            let Some(named) = entry.file_name() else {
                return Err(engineaudit::AuditError::Unreadable {
                    path: entry.display().to_string(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "a probe log path ends in no name",
                    ),
                });
            };
            named.to_os_string().into_string().map_err(|_name| {
                engineaudit::AuditError::Unreadable {
                    path: entry.display().to_string(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "probe log name is not UTF-8",
                    ),
                }
            })
        })
        .collect()
}

/// What a release is made of, as a `CycloneDX` document.
///
/// # Errors
/// A `cargo metadata` that could not be run or read, or a file that could not be written.
pub fn sbom(root: &Path, output: Option<&Path>) -> Result<String, GateError> {
    let asked = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| GateError(format!("cargo metadata: {error}")))?;
    if !asked.status.success() {
        let stderr = std::str::from_utf8(&asked.stderr)
            .map_err(|error| GateError(format!("cargo metadata stderr is not UTF-8: {error}")))?;
        return Err(GateError(format!("cargo metadata: {}", stderr.trim())));
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| GateError(format!("Cargo.toml: {error}")))?;
    let version = release::workspace_version(&manifest)
        .ok_or_else(|| GateError("Cargo.toml has no [workspace.package].version".to_owned()))?;
    let stdout = std::str::from_utf8(&asked.stdout)
        .map_err(|error| GateError(format!("cargo metadata stdout is not UTF-8: {error}")))?;
    let bom =
        crate::sbom::of(stdout, ("njutest", &version)).map_err(|error| GateError(error.coded()))?;
    let document = serde_json::to_string_pretty(&bom)
        .map_err(|error| GateError(format!("the bill of materials: {error}")))?;
    match output {
        Some(path) => {
            std::fs::write(path, format!("{document}\n"))
                .map_err(|error| GateError(format!("{}: {error}", path.display())))?;
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
pub fn report_diff(before: &Path, after: &Path) -> Result<String, GateError> {
    let read = |path: &Path| -> Result<String, GateError> {
        std::fs::read_to_string(path)
            .map_err(|error| GateError(format!("{}: {error}", path.display())))
    };
    let (left, right) = (read(before)?, read(after)?);
    let changes = reportdiff::compare(
        (&before.display().to_string(), &left),
        (&after.display().to_string(), &right),
    )
    .map_err(|error| GateError(error.coded()))?;

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
fn declared_surfaces(root: &Path) -> Result<Vec<(String, Surface, PathBuf)>, GateError> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .map_err(|error| GateError(format!("cargo metadata: {error}")))?;
    let mut declared = Vec::new();
    for package in metadata.workspace_packages() {
        let named = package
            .metadata
            .get("njutest")
            .and_then(|value| value.get("surface"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                GateError(format!(
                    "{}: no [package.metadata.njutest] surface, so this gate cannot say whether a \
                     function only a test reaches is a finding",
                    package.name
                ))
            })?;
        let surface = Surface::named(named).ok_or_else(|| {
            GateError(format!(
                "{}: surface {named:?} is not one this gate knows",
                package.name
            ))
        })?;
        let directory = package
            .manifest_path
            .parent()
            .ok_or_else(|| GateError(format!("{}: manifest has no directory", package.name)))?;
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
pub fn reached(root: &Path) -> Result<String, GateError> {
    let declared = declared_surfaces(root)?;

    let mut ships = String::new();
    let mut tested = String::new();
    for (_name, surface, directory) in &declared {
        for file in rust_files_under(root, directory)? {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| GateError(format!("{}: {error}", file.display())))?;
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
        for file in rust_files_under(root, &directory.join("src"))? {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| GateError(format!("{}: {error}", file.display())))?;
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
        return Err(GateError(
            "xtask/reached_ceiling.txt holds one number and holds nothing".to_owned(),
        ));
    };
    let ceiling = match written.parse::<usize>() {
        Ok(ceiling) => ceiling,
        Err(why) => {
            return Err(GateError(format!(
                "xtask/reached_ceiling.txt holds one number and holds {written:?}: {why}"
            )));
        }
    };
    if only_tests.len() > ceiling {
        return Err(GateError(format!(
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

/// Every audit reader supplies no more values its input never gave than `xtask/defaulted_ceiling.txt` allows it, and exactly that many.
///
/// # Errors
/// Every file above or below its ceiling, and a source or ceiling that does not read.
pub fn defaulted(root: &Path) -> Result<String, GateError> {
    let mut counted = BTreeMap::new();
    for file in rust_files_under(root, &root.join("xtask/src"))? {
        let relative = file
            .strip_prefix(root)
            .map_err(|error| GateError(format!("{}: {error}", file.display())))?
            .components()
            .map(|part| {
                part.as_os_str().to_str().ok_or_else(|| {
                    GateError(format!("{}: a path that is not UTF-8", file.display()))
                })
            })
            .collect::<Result<Vec<&str>, GateError>>()?
            .join("/");
        if !crate::defaulted::reads_for_an_audit(&relative) {
            continue;
        }
        let text = std::fs::read_to_string(&file)
            .map_err(|error| GateError(format!("{}: {error}", file.display())))?;
        let count = crate::defaulted::defaulted_in(&text)
            .map_err(|error| GateError(format!("{relative}: {error}")))?;
        counted.insert(relative, count);
    }
    let ceiling = root.join("xtask/defaulted_ceiling.txt");
    let written = std::fs::read_to_string(&ceiling)
        .map_err(|error| GateError(format!("{}: {error}", ceiling.display())))?;
    match crate::defaulted::held(&counted, &written) {
        Ok(total) => Ok(format!(
            "defaulted: {total} value(s) supplied where an audit reader's input gave none, each \
             file at its ceiling"
        )),
        Err(refused) => Err(GateError(format!(
            "defaulted: an audit reader holds a record to a schema and then answers for an absent \
             field anyway:\n  {}",
            refused.join("\n  ")
        ))),
    }
}

/// Every `.rs` file under `directory`, skipping anything built.
fn rust_files_under(root: &Path, directory: &Path) -> Result<Vec<PathBuf>, GateError> {
    let base = relative_slash(root, directory)?;
    Ok(crate::repository::under(root, &base)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect())
}
