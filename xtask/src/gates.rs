// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gates applied to this repository: each one reads the tree, hands it to the pure checker of its module, and renders the answer.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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

/// Whether a waiving file and a declaring file are compiled as one crate, which is what decides whether an arm could have been left out.
///
/// An integration test is its own crate, so an enum that says it may grow
/// forces a place to be left for the growth there even though the same match
/// inside the declaring crate would not need one.
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
/// This never refuses anything. The ledger is a reviewed list and the review
/// is a person's; what a machine can add is a reading that was not derived
/// from theirs, so the two can disagree. An audit that shares the
/// implementation it audits agrees with it for free
/// (ADR 0023), which is why this reads only the syntax and says so in every line it
/// prints.
///
/// # Errors
/// A file the ledger names that cannot be read.
pub fn waivers(root: &Path) -> Result<String, GateFailure> {
    let files = all_sources(root);
    let (ours, open) = sets(root, &files)?;
    let path = root.join("xtask/wildcard_allowlist.txt");
    let ledger = std::fs::read_to_string(&path)
        .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
    let mut said = String::new();
    let mut counted: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut entries: Vec<&'static str> = Vec::new();
    for entry in ledger
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let Some((file, item, over)) = parted(entry) else {
            continue;
        };
        let source = std::fs::read_to_string(root.join(file))
            .map_err(|error| GateFailure(format!("{file}: {error}")))?;
        let mut shaped = shapes::shapes(&source, &ours);
        let lines: Vec<usize> = lint_scan::wildcards_over(&source, &ours)
            .into_iter()
            .filter(|one| one.item == item && one.over == over)
            .map(|one| one.line)
            .collect();
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
        let _written = writeln!(said, "{entry}\t{word}\t{hint}");
    }
    for word in &entries {
        counted.insert(word, entries.iter().filter(|one| *one == word).count());
    }
    let tally = counted
        .iter()
        .map(|(word, how_many)| format!("{how_many} {word}"))
        .collect::<Vec<_>>()
        .join(", ");
    let _written = writeln!(
        said,
        "waivers: {tally}, read from the syntax and from nothing else. This is a \
         second reading, not a verdict, and it refuses nothing: where it disagrees \
         with the ledger, the disagreement is the thing worth looking at."
    );
    let _written = writeln!(
        said,
        "waivers: a line counted `required` is one the compiler demands and the gate \
         asked a waiver for anyway, which is the gate to fix rather than the ledger."
    );
    Ok(said)
}

/// Refuses `#[allow]`, `Box<dyn Trait>`, and a comment that is not documentation, anywhere in the repository's own code.
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
    found.extend(loose_layouts(root, &files)?);
    found.extend(wildcards(root, &files)?);
    found.sort();
    if found.is_empty() {
        return Ok(format!(
            "lints: {} files carry no #[allow], no Box<dyn Trait>, no comment beside the \
             code, no layout anybody but the configuration has decided, no colour \
             anybody but rust_mutants::telling has decided, and no type that publishes \
             its whole list and also says the list is open. {} catch-all waiver(s) are \
             still standing; `cargo xtask waivers` reads each of them a second time",
            files.len(),
            waived_lines(root)?
        ));
    }
    let mut report = String::new();
    for finding in &found {
        let _written = writeln!(report, "{finding}");
    }
    Err(GateFailure(report.trim_end().to_owned()))
}

/// Every exported constant that more than one module joins onto a path for itself.
///
/// This is the shape the report layout had: a `pub const` spelling a structure,
/// joined in six places and in the tests, so the configuration could not own
/// it and moving it meant moving all of them. One module joining its own
/// constant is not that — it is a name it happens to have written down — and a
/// document type or a URL is not a path at all, which is why this counts the
/// Every catch-all over a set this repository closes, over the whole tree at once.
///
/// Two passes, because whether an arm may catch everything depends on who owns
/// the enum, and that is a fact about the workspace rather than about the file
/// being read. A foreign enum keeps its catch-all: the values of `syn::Expr`
/// are not ours to list, so an arm that stands for the rest is the handling.
fn wildcards(root: &Path, files: &[PathBuf]) -> Result<Vec<lint_scan::Finding>, GateFailure> {
    let (ours, open) = sets(root, files)?;
    let mut standing: Vec<Waived> = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, path);
        let mut grouped: std::collections::BTreeMap<(String, String), Vec<lint_scan::Wildcard>> =
            std::collections::BTreeMap::new();
        for one in lint_scan::wildcards_over(&source, &ours) {
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
/// Two answers from one read of the tree, because a caller that needs the
/// second always needs the first and two walks would be two chances to
/// disagree about what an enum of ours is.
///
/// # Errors
/// A file that cannot be read.
fn sets(
    root: &Path,
    files: &[PathBuf],
) -> Result<(Vec<String>, std::collections::BTreeMap<String, String>), GateFailure> {
    let mut ours: Vec<String> = Vec::new();
    let mut open: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for path in files {
        let source = std::fs::read_to_string(path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        ours.extend(lint_scan::declared_enums(&source));
        let label = relative_slash(root, path);
        for name in lint_scan::open_enums(&source) {
            open.insert(name, label.clone());
        }
    }
    ours.sort();
    ours.dedup();
    Ok((ours, open))
}

/// The file, the item and the set a ledger name is made of.
///
/// A name carries no line, which is the point of it, so the second reading
/// finds the arms for itself rather than being handed a coordinate that may
/// by now be pointing at something else.
fn parted(entry: &str) -> Option<(&str, &str, &str)> {
    let (place, rest) = entry.split_once(" over ")?;
    let (over, _how_many) = rest.split_once(", ")?;
    let (file, item) = place.split_once("::").unwrap_or((place, ""));
    Some((file, item, over))
}

/// How many waivers the catch-all ledger still carries, held to the ceiling beside it.
///
/// In the pass line because a number somebody sees every run is a number they
/// notice moving, and a file of forty-four that nobody could shorten was a
/// file nobody opened. Held to `xtask/waiver_ceiling.txt` because noticing is
/// not holding: the ledger's header has always said it may shrink and never
/// grow, and until now the count was printed and compared against nothing, so
/// a waiver could be granted by the same hand that wrote the code wanting one.
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
/// The ledger may shrink and never grow. A line that is still there is
/// reported as nothing; a catch-all that is not on it is refused, so writing
/// one is not a thing anybody decides while writing — it is a line somebody
/// else reads.
/// A group of catch-all arms the ledger either waives or has never been shown.
///
/// `name` is what the ledger holds and `line` is only where to look: a
/// coordinate cannot say what is being waived, and a ledger that keyed on
/// one waived whatever happened to be standing there when it was next read.
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
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (name, _line) in lint_scan::exported_strings(&source) {
            layouts.push((module.clone(), name));
        }
    }
    for path in production_sources(root) {
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
        let label = relative_slash(root, path);
        for line in lint_scan::spelled(&source, &configured) {
            found.push(lint_scan::Finding {
                kind: lint_scan::Kind::LooseLayout,
                file: label.clone(),
                line,
            });
        }
    }
    for path in tests_under(root) {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| GateFailure(format!("{}: {error}", path.display())))?;
        let label = relative_slash(root, &path);
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
fn tests_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for base in ["crates", "xtask"] {
        for entry in WalkDir::new(root.join(base))
            .sort_by_file_name()
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            let relative = relative_slash(root, path);
            if entry.file_type().is_file()
                && path.extension().is_some_and(|one| one == "rs")
                && relative.contains("/tests/")
            {
                found.push(path.to_path_buf());
            }
        }
    }
    found
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
    for gate in [devgates, lints, deps, fixtures, release_check, waivers] {
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
    let touched = std::fs::read_to_string(asked.run.join("touched-v1.json")).ok();
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
            touched: touched.as_deref(),
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
        ("njutest", &version),
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
