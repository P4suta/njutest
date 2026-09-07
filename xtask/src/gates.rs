// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gates applied to this repository: each one reads the tree, hands it to the pure checker of its module, and renders the answer.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::{
    deps, devgates, engineaudit, fixtures, lints as lint_scan, proofaudit, release, reportdiff,
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

/// Every Rust file the repository commits, tests included.
#[must_use]
pub fn all_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for base in ["crates", "xtask", "fuzz"] {
        for entry in WalkDir::new(root.join(base))
            .sort_by_file_name()
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            let relative = relative_slash(root, path);
            if entry.file_type().is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
                && !relative.split('/').any(|part| part == "target")
            {
                files.push(path.to_path_buf());
            }
        }
    }
    files
}

/// Refuses `#[allow]` and `Box<dyn Trait>` anywhere in the repository.
///
/// # Errors
/// Every finding, one per line, or a file that could not be read or parsed.
pub fn lints(root: &Path) -> Result<String, GateFailure> {
    let files = all_sources(root);
    let mut found = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path);
        found.extend(
            lint_scan::scan_source(&label, &source)
                .map_err(|error| GateFailure(format!("{label}: {error}")))?,
        );
    }
    if found.is_empty() {
        return Ok(format!(
            "lints: {} files carry no #[allow] and no Box<dyn Trait>",
            files.len()
        ));
    }
    let mut report = String::new();
    for finding in &found {
        let _written = writeln!(report, "{finding}");
    }
    Err(GateFailure(report.trim_end().to_owned()))
}

/// The production source files the seam ratchet scans.
#[must_use]
pub fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for base in ["crates", "xtask"] {
        for entry in WalkDir::new(root.join(base))
            .sort_by_file_name()
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if entry.file_type().is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                let relative = relative_slash(root, path);
                if is_production(&relative) {
                    files.push(path.to_path_buf());
                }
            }
        }
    }
    files
}

fn is_production(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    if parts.first() == Some(&"crates") && parts.get(1) == Some(&"mjutest-devkit") {
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

fn relative_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The seam ratchet against `xtask/seam_allowlist.txt`.
///
/// # Errors
/// Returns a disagreement between the scan and the ledger, or an unreadable file.
pub fn devgates(root: &Path) -> Result<String, GateFailure> {
    let mut found = Vec::new();
    let files = production_sources(root);
    for path in &files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path);
        let seams = devgates::scan_source(&label, &source)
            .map_err(|error| GateFailure(format!("{label}: {error}")))?;
        found.extend(seams);
    }
    found.sort();
    found.dedup();
    let ledger_path = root.join("xtask/seam_allowlist.txt");
    let ledger_text = std::fs::read_to_string(&ledger_path)
        .map_err(|error| GateFailure(format!("{}: {error}", ledger_path.display())))?;
    let ledger =
        devgates::parse_ledger(&ledger_text).map_err(|error| GateFailure(error.to_string()))?;
    devgates::compare(&found, &ledger)
        .map_err(|disagreement| GateFailure(disagreement.to_string()))?;
    Ok(format!(
        "devgates: {} production files scanned, {} seams recorded in the ledger",
        files.len(),
        ledger.len()
    ))
}

/// Dependency direction between the workspace crates.
///
/// # Errors
/// Returns every edge the direction rule refuses, or a `cargo metadata` failure.
pub fn deps(root: &Path) -> Result<String, GateFailure> {
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
    for package in metadata.workspace_packages() {
        for dependency in &package.dependencies {
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
    if violations.is_empty() {
        return Ok(format!(
            "deps: {} internal edges, all in the allowed direction",
            edges.len()
        ));
    }
    let mut message = String::from("deps: the dependency direction rule refuses:\n");
    for violation in violations {
        let _written = writeln!(message, "  {violation}");
    }
    let _written = write!(message, "{}", deps::RULE);
    Err(GateFailure(message))
}

/// Conventions of the fixture projects.
///
/// # Errors
/// Returns every convention a fixture breaks.
pub fn fixtures(root: &Path) -> Result<String, GateFailure> {
    let dir = root.join("fixtures");
    let mut names = Vec::new();
    let mut problems = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(Result::ok) {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            for problem in fixtures::check_fixture(&entry.path()) {
                problems.push(format!("fixtures/{name}: {problem}"));
            }
            names.push(name);
        }
    }
    if problems.is_empty() {
        names.sort();
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
    let manifest = read(".release-please-manifest.json")?;
    let mut members = Vec::new();
    for entry in WalkDir::new(root.join("crates"))
        .max_depth(2)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_name() == "Cargo.toml" {
            let label = relative_slash(root, entry.path());
            members.push((label.clone(), read(&label)?));
        }
    }
    let member_refs: Vec<(&str, &str)> = members
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let problems = release::check(&workspace, &manifest, &member_refs);
    if problems.is_empty() {
        let version = release::workspace_version(&workspace).unwrap_or_default();
        return Ok(format!(
            "release-check: version {version} is consistent across {} member manifests",
            members.len()
        ));
    }
    Err(GateFailure(format!(
        "release-check:\n  {}",
        problems.join("\n  ")
    )))
}

/// Every gate, in order, stopping at the first failure.
///
/// # Errors
/// Returns the first gate's failure.
pub fn all(root: &Path) -> Result<String, GateFailure> {
    let mut report = String::new();
    for gate in [devgates, lints, deps, fixtures, release_check] {
        let _written = writeln!(report, "{}", gate(root)?);
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
        .and_then(|path| std::fs::read_to_string(path).ok());
    proofaudit::audit(&label, &text, recorded.as_deref())
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
        .map(|directory| directory.join("trace.jsonl"))
        .and_then(|path| std::fs::read_to_string(path).ok());
    let parts: Vec<(String, String)> = asked
        .shards
        .iter()
        .filter_map(|part| {
            let path = if part.is_dir() {
                part.join(engineaudit::REPORT_FILE)
            } else {
                part.clone()
            };
            let text = std::fs::read_to_string(&path).ok()?;
            Some((path.display().to_string(), text))
        })
        .collect();
    let ledger = asked
        .ledger
        .and_then(|path| std::fs::read_to_string(path).ok());
    let reached = std::fs::read_to_string(asked.run.join("reached-v1.json")).ok();
    let catalog = std::fs::read_to_string(asked.run.join("catalog-v1.json")).ok();
    let probe_logs = std::fs::read_dir(asked.run.join("probe"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(ToOwned::to_owned))
        .collect();
    engineaudit::audit(
        &label,
        &text,
        &engineaudit::Evidence {
            recorded: recorded.as_deref(),
            shards: parts
                .iter()
                .map(|(name, text)| (name.clone(), text.as_str()))
                .collect(),
            ledger: ledger.as_deref(),
            sites: asked.sites,
            reached: reached.as_deref(),
            catalog: catalog.as_deref(),
            probe_logs,
        },
    )
}

/// What a release is made of, as a `CycloneDX` document.
///
/// # Errors
/// A `cargo metadata` that could not be run or read, or a file that could not
/// be written.
pub fn sbom(root: &Path, output: Option<&Path>) -> Result<String, GateFailure> {
    let asked = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| GateFailure(format!("cargo metadata: {error}")))?;
    if !asked.status.success() {
        return Err(GateFailure(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&asked.stderr).trim()
        )));
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| GateFailure(format!("Cargo.toml: {error}")))?;
    let version = release::workspace_version(&manifest)
        .ok_or_else(|| GateFailure("Cargo.toml has no [workspace.package].version".to_owned()))?;
    let bom = crate::sbom::of(
        &String::from_utf8_lossy(&asked.stdout),
        ("mjutest", &version),
    )
    .map_err(GateFailure)?;
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
        let _written = writeln!(report, "{change}");
    }
    Ok(report.trim_end().to_owned())
}
