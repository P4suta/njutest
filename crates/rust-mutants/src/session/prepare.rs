// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Preparing a workspace: the gate it stands on, what the proof layers establish before anything is instrumented, and the one build every accepted mutant lives in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{PrepareOptions, Session, Verified, verify};
use crate::EngineError;
use crate::cargo::{CompileKind, CompileOptions, compile};
use crate::catalog::Catalog;
use crate::discover::{self, DiscoverOptions};
use crate::execute::{self, TestTarget};
use crate::instrument::{FileOutput, Placement, instrument_file, plan_file};
use crate::rule::Registry;
use crate::runner::Cancel;
use crate::snapshot::Drift;
use crate::syntax::{Found, LineIndex, Selection};
use crate::trace::{BuildRecord, InstrumentRecord};
use crate::validate::{
    Attempt, Compile, ValidateError, ValidateOptions, Validated, Validating, validate_selected,
};
use crate::workspace::{SessionError, Workspace};

/// The rules a set of options selects.
pub(super) fn selection(options: &PrepareOptions) -> Result<Selection<'static>, EngineError> {
    static REGISTRY: Registry = Registry::canonical();
    if options.operators.is_empty() {
        return Ok(Selection::tier(&REGISTRY, options.tier));
    }
    let names: Vec<&str> = options.operators.iter().map(String::as_str).collect();
    Ok(Selection::rules(&REGISTRY, &names)?)
}

/// Refuses a tree that does not compile before anything is instrumented, and hands back the units the check compiled.
fn gate(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<crate::cargo::Compiled, EngineError> {
    pristine(workspace, options, cancel)
}

/// Compiles the tree as it was copied, which is both the gate a run stands on and the source of every unit's file set.
pub(super) fn pristine(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<crate::cargo::Compiled, EngineError> {
    let checked = compile(
        &workspace.driver(cancel),
        &CompileOptions {
            kind: CompileKind::Check,
            packages: Vec::new(),
            target_dir: Some(workspace.target_dir.clone()),
            locked: workspace.locked,
            offline: workspace.offline,
            timeout: Workspace::timeout(options.build_timeout),
            env: Vec::new(),
            build: options.build.clone(),
        },
    )?;
    if checked.success {
        Ok(checked)
    } else {
        Err(EngineError::from(SessionError::PristineBroken {
            first: crate::validate::first_error_of(&checked.messages),
        }))
    }
}

/// What the build read that no survey of the tree sees: every file outside the copy and outside the build's own output, and every variable the compiler read.
fn inputs_of(
    workspace: &Workspace,
    checked: &crate::cargo::Compiled,
) -> Result<crate::select::Inputs, EngineError> {
    let mut inputs = crate::select::Inputs::default();
    for unit in &checked.units {
        for path in &unit.inputs {
            if path.starts_with(workspace.snapshot.dir())
                || path.starts_with(workspace.target_dir())
            {
                continue;
            }
            let name = path
                .to_str()
                .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: path.clone() })?
                .to_owned();
            if let std::collections::btree_map::Entry::Vacant(entry) = inputs.outside.entry(name) {
                let bytes =
                    std::fs::read(path).map_err(|source| SessionError::EvidenceReadFailed {
                        path: path.clone(),
                        source,
                    })?;
                entry.insert(crate::id::HexDigest::of(&bytes));
            }
        }
        inputs.env.extend(
            unit.env
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }
    let root = workspace.snapshot_root();
    for path in crate::cargo::compile_time_inputs(&checked.messages, root)? {
        let Ok(relative) = path.strip_prefix(root) else {
            if !path.starts_with(workspace.snapshot.dir())
                && !path.starts_with(workspace.target_dir())
            {
                let name = path
                    .to_str()
                    .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: path.clone() })?
                    .to_owned();
                let bytes =
                    std::fs::read(&path).map_err(|source| SessionError::EvidenceReadFailed {
                        path: path.clone(),
                        source,
                    })?;
                inputs
                    .outside
                    .insert(name, crate::id::HexDigest::of(&bytes));
            }
            continue;
        };
        let text = relative
            .to_str()
            .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: path.clone() })?;
        let name = crate::id::normalize_path(text).map_err(|source| {
            SessionError::EvidencePathInvalid {
                path: path.clone(),
                source,
            }
        })?;
        inputs.compile_time.insert(name);
    }
    scripts_of(workspace, checked, &mut inputs)?;
    Ok(inputs)
}

/// What every build script of the build set in the compiler's environment, and what each said, through `rerun-if-changed` and `rerun-if-env-changed`, its output depends on.
fn scripts_of(
    workspace: &Workspace,
    checked: &crate::cargo::Compiled,
    inputs: &mut crate::select::Inputs,
) -> Result<(), SessionError> {
    let root = workspace.snapshot_root();
    for message in &checked.messages {
        let crate::cargo::Message::BuildScriptExecuted(script) = message else {
            continue;
        };
        inputs
            .scripted
            .extend(script.env.iter().map(|(name, _)| name.clone()));
        let package = workspace
            .metadata
            .packages
            .iter()
            .find(|package| package.id == script.package_id);
        let directory = package.map(|package| package.manifest_dir().to_path_buf());
        let named = match directory
            .as_deref()
            .map(|directory| directory.strip_prefix(root))
        {
            Some(Ok(relative)) => relative
                .to_str()
                .ok_or_else(|| SessionError::EvidencePathNotUtf8 {
                    path: relative.to_path_buf(),
                })?
                .replace('\\', "/"),
            Some(Err(_)) | None => format!("<outside>:{}", script.package_id),
        };
        let said = script
            .out_dir
            .as_deref()
            .and_then(Path::parent)
            .map(|build| std::fs::read_to_string(build.join("output")));
        let text = match said {
            Some(Ok(text)) => text,
            Some(Err(_)) | None => String::new(),
        };
        let (changed, env) = said_by(&text, &workspace.base_env);
        let watched = watched_of(changed, directory.as_deref(), root)?;
        inputs
            .scripts
            .insert(named, crate::select::Script { watched, env });
    }
    Ok(())
}

/// The paths and the variables one build script's output named as what it depends on.
fn said_by(
    text: &str,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> (Vec<String>, BTreeMap<String, Option<String>>) {
    let mut changed = Vec::new();
    let mut watched = BTreeMap::new();
    for line in text.lines() {
        let line = line
            .strip_prefix("cargo::")
            .or_else(|| line.strip_prefix("cargo:"))
            .unwrap_or_default();
        if let Some(path) = line.strip_prefix("rerun-if-changed=") {
            changed.push(path.to_owned());
        } else if let Some(name) = line.strip_prefix("rerun-if-env-changed=") {
            let value = crate::vars::var(env, name)
                .and_then(std::ffi::OsStr::to_str)
                .map(ToOwned::to_owned);
            watched.insert(name.to_owned(), value);
        }
    }
    (changed, watched)
}

/// What one build script watches: its whole package where it named no path, and otherwise each path it named, inside the tree by place and outside it by what is there.
fn watched_of(
    changed: Vec<String>,
    directory: Option<&Path>,
    root: &Path,
) -> Result<crate::select::Watched, SessionError> {
    if changed.is_empty() {
        return Ok(crate::select::Watched::Package);
    }
    let mut inside = BTreeSet::new();
    let mut outside = BTreeMap::new();
    for path in changed {
        let full =
            directory.map_or_else(|| PathBuf::from(&path), |directory| directory.join(&path));
        let text = full
            .to_str()
            .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: full.clone() })?;
        match full.strip_prefix(root) {
            Ok(relative) => {
                let relative = relative
                    .to_str()
                    .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: full.clone() })?;
                inside.insert(relative.replace('\\', "/"));
            }
            Err(_) => {
                outside.insert(text.to_owned(), crate::select::fingerprint(&full));
            }
        }
    }
    Ok(crate::select::Watched::Paths { inside, outside })
}

/// The digest of everything the build read: every file any unit's dep-info names, build scripts and generated files included, and every environment variable rustc recorded reading.
/// A file under the root is named relative to it and one the build generated relative to the target directory, so the digest travels with the tree; anything else outside is the lock file's to key.
fn closure_of(
    workspace: &Workspace,
    checked: &crate::cargo::Compiled,
) -> Result<String, SessionError> {
    let root = workspace.snapshot_root();
    let target = workspace.target_dir();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for path in &checked.inputs.files {
        let (relative, class) = match (path.strip_prefix(root), path.strip_prefix(target)) {
            (Ok(relative), _) => (relative, ""),
            (Err(_), Ok(relative)) => (relative, "$target/"),
            (Err(_), Err(_)) => continue,
        };
        let relative_text = relative
            .to_str()
            .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: path.clone() })?;
        let name = crate::id::normalize_path(relative_text).map_err(|source| {
            SessionError::EvidencePathInvalid {
                path: path.clone(),
                source,
            }
        })?;
        if let std::collections::btree_map::Entry::Vacant(entry) =
            files.entry(format!("{class}{name}"))
        {
            let bytes = std::fs::read(path).map_err(|source| SessionError::EvidenceReadFailed {
                path: path.clone(),
                source,
            })?;
            entry.insert(crate::id::digest(&bytes));
        }
    }
    let (root_text, target_text) = (text_of(root)?, text_of(target)?);
    let portable = |text: &str| {
        text.replace(target_text, "$target")
            .replace(root_text, "$root")
    };
    for read in &checked.inputs.env {
        files.insert(
            format!("$env/{}", read.name),
            env_value(read.value.as_deref(), portable),
        );
    }
    for told in &checked.inputs.emitted {
        let (name, digest) = emitted_entry(told, (root, target), portable)?;
        files.insert(name, digest);
    }
    if files.is_empty() {
        return Ok(String::new());
    }
    Ok(folded(
        files
            .iter()
            .map(|(name, digest)| (name.as_str(), digest.as_str())),
    ))
}

/// What one build script emitted, as a closure entry: named by the directory it wrote into, and digested over every configuration, variable, and link request, with the run's own directories spelled portably.
fn emitted_entry(
    told: &crate::cargo::Emitted,
    (root, target): (&Path, &Path),
    portable: impl Fn(&str) -> String,
) -> Result<(String, String), SessionError> {
    let named = match told.out_dir.as_deref() {
        None => "unnamed".to_owned(),
        Some(directory) => match (directory.strip_prefix(root), directory.strip_prefix(target)) {
            (Ok(relative), _) => format!("$root/{}", portable_path(directory, relative)?),
            (Err(_), Ok(relative)) => format!("$target/{}", portable_path(directory, relative)?),
            (Err(_), Err(_)) => portable(text_of(directory)?),
        },
    };
    let mut said = String::new();
    for (kind, values) in [
        ("cfg", told.cfgs.clone()),
        (
            "env",
            told.env
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect(),
        ),
        ("lib", told.linked_libs.clone()),
        ("path", told.linked_paths.clone()),
    ] {
        for value in values {
            said.push_str(kind);
            said.push('\0');
            said.push_str(&portable(&value));
            said.push('\n');
        }
    }
    Ok((
        format!("$emitted/{named}"),
        crate::id::digest(said.as_bytes()),
    ))
}

/// `relative`, a part of `full`, with forward slashes, as a portable name spells it.
fn portable_path(full: &Path, relative: &Path) -> Result<String, SessionError> {
    crate::id::normalize_path(text_of(relative)?).map_err(|source| {
        SessionError::EvidencePathInvalid {
            path: full.to_path_buf(),
            source,
        }
    })
}

/// The digest of every manifest, the lock file, and the cargo configuration the build read.
fn manifests_of(workspace: &Workspace) -> Result<String, SessionError> {
    let root = workspace.snapshot_root();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let named: Vec<PathBuf> = workspace
        .metadata
        .packages
        .iter()
        .map(|package| package.manifest_path.clone())
        .chain([
            root.join("Cargo.toml"),
            root.join("Cargo.lock"),
            root.join(".cargo").join("config.toml"),
            root.join(".cargo").join("config"),
            root.join("rust-toolchain.toml"),
            root.join("rust-toolchain"),
        ])
        .collect();
    for path in named {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative_text = relative
            .to_str()
            .ok_or_else(|| SessionError::EvidencePathNotUtf8 { path: path.clone() })?;
        let name = crate::id::normalize_path(relative_text).map_err(|source| {
            SessionError::EvidencePathInvalid {
                path: path.clone(),
                source,
            }
        })?;
        if let std::collections::btree_map::Entry::Vacant(entry) = files.entry(name) {
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(SessionError::EvidenceReadFailed {
                        path: path.clone(),
                        source,
                    });
                }
            };
            entry.insert(crate::id::digest(&bytes));
        }
    }
    Ok(folded(
        files
            .iter()
            .map(|(name, digest)| (name.as_str(), digest.as_str())),
    ))
}

/// One digest over a sorted list of names and their own digests.
fn folded<'a>(entries: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut text = String::new();
    for (name, digest) in entries {
        text.push_str(name);
        text.push('\0');
        text.push_str(digest);
        text.push('\n');
    }
    crate::id::digest(text.as_bytes())
}

/// The test binaries the instrumented build produced, the directory their processes work in, and what running them once established.
type Built = (Vec<TestTarget>, PathBuf, Verified);

/// What the run doing the building is: what stops it, what it records to, and the catalog its guards name.
#[derive(Debug, Clone, Copy)]
pub(super) struct Building<'a> {
    /// The workspace being prepared, whose toolchain starts every process and whose recording every phase writes to.
    pub(super) workspace: &'a Workspace,
    /// What the run is stopped by.
    pub(super) cancel: &'a Cancel,
    /// What it records to.
    pub(super) trace: &'a crate::trace::Recorder,
    /// The catalog every guard of the tree was generated from, which is what a record must be about.
    pub(super) catalog: &'a Catalog,
    /// The guards the final build contains, in catalog order.
    pub(super) accepted: &'a [u32],
    /// Which comparison and body markers the final build can record.
    pub(super) narrowing: &'a crate::touch::Narrowing,
    /// How many items the tree's entry markers can name.
    pub(super) items: u32,
    /// The digest of the pristine sources the build read.
    pub(super) closure: &'a str,
    /// The digest of the manifests and Cargo configuration the build read.
    pub(super) manifests: &'a str,
    /// Whether the guards are asked what they reached on the run that verifies the baseline.
    pub(super) asked: bool,
    /// The messages of the build the tree ended at, which name the test binaries to start.
    pub(super) last_build: &'a [crate::cargo::Message],
    /// What the run was asked to prepare.
    pub(super) options: &'a PrepareOptions,
}

fn built(building: &Building<'_>) -> Result<Built, EngineError> {
    let phase = building.trace.phase("build");
    let built = built_untraced(building)?;
    phase.end();
    Ok(built)
}

/// The test binaries the instrumented build produced, and what running them once established.
fn built_untraced(building: &Building<'_>) -> Result<Built, EngineError> {
    let Building {
        workspace,
        trace,
        last_build,
        options,
        ..
    } = *building;
    let mut targets = execute::targets_of(
        last_build,
        &workspace.metadata.packages,
        Some(&workspace.target_dir),
    )?;
    if targets.is_empty() {
        return Err(EngineError::from(SessionError::NoTargets {
            packages: options.packages.clone(),
        }));
    }
    let members: Vec<&crate::cargo::Package> = workspace
        .metadata
        .members()
        .filter(|package| options.packages.is_empty() || options.packages.contains(&package.name))
        .collect();
    if options.doctests {
        targets.extend(execute::documentation_targets(
            &members,
            workspace.toolchain.cargo(),
            &documentation_arguments(workspace),
        ));
    }
    let built: Vec<crate::trace::TargetRecord> = targets
        .iter()
        .map(|target| crate::trace::TargetRecord {
            id: target.id.clone(),
            kind: target.kind.name().to_owned(),
            harness: target.harness,
            limitations: target.limitations.clone(),
        })
        .collect();
    let skipped = left_out(workspace, &targets, &options.skip_targets)?;
    targets = execute::startable(&targets, &skipped);
    let mut details = built;
    for detail in &mut details {
        if skipped.contains(&detail.id) {
            detail
                .limitations
                .push(crate::limitation::TARGET_SKIPPED_BY_CONFIGURATION.to_owned());
        }
    }
    trace.build(BuildRecord {
        targets: details.iter().map(|target| target.id.clone()).collect(),
        details,
    });
    if targets.is_empty() {
        return Err(EngineError::from(SessionError::NoTargets {
            packages: options.packages.clone(),
        }));
    }
    let scratch = workspace.scratch_dir.clone();
    std::fs::create_dir_all(&scratch).map_err(|source| SessionError::WriteFailed {
        path: scratch.display().to_string(),
        source,
    })?;
    let verified = if options.verify {
        let verified = verify(workspace, &mut targets, &scratch, building)?;
        excluded(&mut targets, &verified, options);
        verified
    } else {
        Verified::default()
    };
    Ok((targets, scratch, verified))
}

/// Which of the built targets a run was told never to start, refusing a name no target of the workspace has.
fn left_out(
    workspace: &Workspace,
    targets: &[TestTarget],
    named: &[String],
) -> Result<Vec<String>, EngineError> {
    let declared = execute::declared_targets(&workspace.metadata.members().collect::<Vec<_>>());
    if let Some(unknown) = named.iter().find(|one| !declared.contains(one.as_str())) {
        return Err(EngineError::from(SessionError::SkippedTargetUnknown {
            name: unknown.clone(),
            available: declared.into_iter().collect(),
        }));
    }
    Ok(targets
        .iter()
        .filter(|target| named.iter().any(|one| one == &target.id))
        .map(|target| target.id.clone())
        .collect())
}

/// Leaves out every target whose own baseline did not pass, where the run asked for that rather than for a refusal.
fn excluded(targets: &mut Vec<TestTarget>, verified: &Verified, options: &PrepareOptions) {
    if options.failing == super::Failing::Refuse {
        return;
    }
    let failing = verified.failing();
    targets.retain(|target| !failing.contains(&target.id.as_str()));
}

/// What cargo is told before the documentation examples' own arguments, so that running them reuses the build this session already made.
fn documentation_arguments(workspace: &Workspace) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        std::ffi::OsString::from("--target-dir"),
        workspace.target_dir.clone().into_os_string(),
    ];
    if workspace.locked {
        args.push(std::ffi::OsString::from("--locked"));
    }
    if workspace.offline {
        args.push(std::ffi::OsString::from("--offline"));
    }
    args
}

/// What the proof layers establish before anything is instrumented: which branch proofs and which comparisons the compiler vouches for, and which targets reached what.
type Layers = (crate::prove::Established, crate::reach::Reached);

fn layers(
    asking: &crate::prove::Asking<'_>,
    eligible: &BTreeSet<u32>,
    remembering: Option<&crate::reach::remembered::Remembering>,
    cancel: &Cancel,
) -> Result<Layers, EngineError> {
    let trace = &asking.workspace.trace;
    let established = if asking.options.branch_proofs {
        crate::prove::establish_selected(asking, eligible, cancel, trace)?
    } else {
        crate::prove::Established::default()
    };
    let reached = measured(asking, remembering, cancel, trace)?;
    Ok((established, reached))
}

/// What measuring this tree established, made now or remembered from the last run that made it.
fn measured(
    asking: &crate::prove::Asking<'_>,
    remembering: Option<&crate::reach::remembered::Remembering>,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<crate::reach::Reached, EngineError> {
    if let Some(remembering) = remembering
        && let Some(reached) = remembering.read()
    {
        let phase = trace.phase("coverage");
        trace.note(
            "coverage-remembered",
            &format!(
                "the measurement of this tree is the one an earlier run made, filed under {}",
                remembering.key
            ),
        );
        phase.end();
        return Ok(reached);
    }
    let reached = crate::reach::establish(
        &crate::reach::Asking {
            workspace: asking.workspace,
            options: asking.options,
        },
        cancel,
        trace,
    )?;
    if let Some(remembering) = remembering
        && reached.measured()
    {
        remembering.write(&reached);
    }
    Ok(reached)
}

/// Which package and which item each mutant belongs to, which is what a report names it by.
fn attributed(discovery: &discover::Discovery) -> (BTreeMap<u32, String>, BTreeMap<u32, String>) {
    let indexed: Vec<(u32, &discover::Located)> = discovery
        .candidates
        .iter()
        .filter_map(|located| {
            let mutant = discovery
                .catalog
                .mutants()
                .iter()
                .find(|mutant| mutant.candidate == located.found.candidate)?;
            Some((mutant.index, located))
        })
        .collect();
    (
        indexed
            .iter()
            .map(|(index, located)| (*index, located.package.clone()))
            .collect(),
        indexed
            .iter()
            .map(|(index, located)| (*index, located.found.item.clone()))
            .collect(),
    )
}

/// Where this run may remember what it measured, when it was given somewhere and asked to measure.
fn remembering(
    options: &PrepareOptions,
    closure: &str,
    manifests: &str,
    workspace: &Workspace,
) -> Option<crate::reach::remembered::Remembering> {
    if !options.coverage || closure.is_empty() {
        return None;
    }
    let directory = options.measurements.as_ref()?;
    let toolchain = format!(
        "{} {} {}",
        workspace.toolchain.cargo_version().summary,
        workspace.toolchain.rustc_version().summary,
        workspace.toolchain.host()
    );
    Some(crate::reach::remembered::Remembering::of(
        directory,
        &crate::reach::remembered::Of {
            closure,
            manifests,
            toolchain: &toolchain,
            build: &options.build.arguments(),
        },
    ))
}

/// What a mutant no measured target reached amounts to: nothing ran, because nothing that ran could have noticed.
/// The gate a run stands on, and what discovery found on the tree it passed.
///
/// # Errors
/// The pristine gate and the failures of discovery.
fn gated(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<Gated, EngineError> {
    let pristine_phase = trace.phase("pristine");
    let checked = gate(workspace, options, cancel)?;
    let read = Digested {
        closure: super::Closure {
            digest: closure_of(workspace, &checked)?,
            units: unit_sources(workspace, &checked)?,
            carrying: super::Carrying::fresh(),
        },
        inputs: inputs_of(workspace, &checked)?,
        manifests: manifests_of(workspace)?,
    };
    pristine_phase.end();
    let discover_phase = trace.phase("discover");
    let discovery = discover::discover(
        &discover::Input {
            root: workspace.snapshot_root(),
            metadata: &workspace.metadata,
            units: &checked.units,
        },
        &DiscoverOptions {
            selection: selection(options)?,
            include: options.include.clone(),
            exclude: options.exclude.clone(),
            packages: options.packages.clone(),
            skips: options.skips.clone(),
        },
        trace,
    )?;
    discover_phase.end();
    Ok(Gated { discovery, read })
}

/// What the gate established: what there is to mutate, and everything the build read.
struct Gated {
    discovery: discover::Discovery,
    read: Digested,
}

/// What the build read, as digests a later run or a selection compares against, and what each unit read.
struct Digested {
    closure: super::Closure,
    inputs: crate::select::Inputs,
    manifests: String,
}

/// Every unit the pristine build compiled, named without a package id, with each file it read under the root or the target directory spelled by its class.
fn unit_sources(
    workspace: &Workspace,
    checked: &crate::cargo::Compiled,
) -> Result<Vec<crate::skeleton::UnitSource>, EngineError> {
    let root = workspace.snapshot_root();
    let target = workspace.target_dir();
    let (root_text, target_text) = (text_of(root)?, text_of(target)?);
    let portable = |text: &str| {
        text.replace(target_text, "$target")
            .replace(root_text, "$root")
    };
    let names: BTreeMap<&str, &str> = workspace
        .metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package.name.as_str()))
        .collect();
    let mut read: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
    let mut units = Vec::new();
    for unit in crate::cargo::unit_inputs_of(&checked.messages, root)? {
        let mut files = BTreeMap::new();
        for path in &unit.inputs.files {
            let (relative, class) = match (path.strip_prefix(root), path.strip_prefix(target)) {
                (Ok(relative), _) => (relative, "$root/"),
                (Err(_), Ok(relative)) => (relative, "$target/"),
                (Err(_), Err(_)) => continue,
            };
            let name = portable_path(path, relative)?;
            let bytes = match read.entry(path.clone()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.get().clone(),
                std::collections::btree_map::Entry::Vacant(entry) => entry
                    .insert(std::fs::read(path).map_err(|source| {
                        SessionError::EvidenceReadFailed {
                            path: path.clone(),
                            source,
                        }
                    })?)
                    .clone(),
            };
            files.insert(format!("{class}{name}"), bytes);
        }
        units.push(crate::skeleton::UnitSource {
            package: names
                .get(unit.package_id.as_str())
                .map_or_else(|| unit.package_id.clone(), |name| (*name).to_owned()),
            target: unit.target.name.clone(),
            kind: unit.target.kind.join(","),
            test: unit.test,
            files,
            env: unit
                .inputs
                .env
                .iter()
                .map(|read| {
                    (
                        read.name.clone(),
                        env_value(read.value.as_deref(), portable),
                    )
                })
                .collect(),
            emitted: unit
                .inputs
                .emitted
                .iter()
                .map(|told| emitted_entry(told, (root, target), portable))
                .collect::<Result<_, _>>()?,
        });
    }
    Ok(units)
}

/// A path as exact text, which every portable name is built from.
fn text_of(path: &Path) -> Result<&str, SessionError> {
    path.to_str()
        .ok_or_else(|| SessionError::EvidencePathNotUtf8 {
            path: path.to_path_buf(),
        })
}

/// A variable's value as a key holds it: unset, or the digest of what it was set to with the run's own directories spelled portably.
fn env_value(value: Option<&str>, portable: impl Fn(&str) -> String) -> String {
    value.map_or_else(
        || "unset".to_owned(),
        |value| format!("set:{}", crate::id::digest(portable(value).as_bytes())),
    )
}

/// The pristine sources, selected placements, and complete-catalog indices one preparation must validate.
type SelectionPlan = (
    BTreeMap<String, Vec<u8>>,
    BTreeMap<String, Vec<Placement>>,
    BTreeSet<u32>,
);

fn selection_plan(
    workspace: &Workspace,
    discovery: &discover::Discovery,
    options: &PrepareOptions,
    trace: &crate::trace::Recorder,
) -> Result<SelectionPlan, EngineError> {
    let phase = trace.phase("plan");
    let (sources, placements) = plan_tree(workspace.snapshot_root(), discovery)?;
    let eligible = eligible(
        &discovery.catalog,
        &sources,
        options.validation_filter.as_ref(),
    )?;
    let placements = selected_placements(placements, &eligible);
    phase.end();
    Ok((sources, placements, eligible))
}

/// Runs validation as one traced phase.
fn validated(
    asking: &Establishing<'_>,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<Instrumented, EngineError> {
    let phase = trace.phase("validate");
    let instrumented = establish(asking, cancel, trace)?;
    phase.end();
    Ok(instrumented)
}

/// Discovers, instruments, validates, builds, and verifies.
///
/// # Errors
/// Every failure of the phases it runs.
pub fn prepare(
    workspace: Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<Session, EngineError> {
    let trace = workspace.trace.clone();
    let phase = trace.phase("prepare");
    let Gated { discovery, read } = gated(&workspace, options, cancel, &trace)?;
    let (sources, placements, eligible) = selection_plan(&workspace, &discovery, options, &trace)?;
    let asking = crate::prove::Asking {
        workspace: &workspace,
        discovery: &discovery,
        sources: &sources,
        options,
    };
    let remembered = remembering(options, &read.closure.digest, &read.manifests, &workspace);
    let (established, reached) = layers(&asking, &eligible, remembered.as_ref(), cancel)?;

    let Instrumented {
        validated,
        last_build,
        narrowing,
        items: item_catalog,
    } = validated(
        &Establishing {
            workspace: &workspace,
            discovery: &discovery,
            sources: &sources,
            placements: &placements,
            established: &established,
            eligible: &eligible,
            options,
        },
        cancel,
        &trace,
    )?;

    let mut workspace = workspace;
    let written_by_a_test = resealed(&mut workspace, &sources)?;
    let (targets, scratch, verified) = built(&Building {
        workspace: &workspace,
        cancel,
        trace: &trace,
        catalog: &discovery.catalog,
        accepted: &validated.accepted,
        narrowing: &narrowing,
        items: item_count(&item_catalog)?,
        closure: &read.closure.digest,
        manifests: &read.manifests,
        asked: options.touch,
        last_build: &last_build,
        options,
    })?;
    phase.end();
    let (packages, items) = attributed(&discovery);
    let item_refs = item_refs(&item_catalog)?;
    let verified = narrowed(verified, narrowing, item_catalog.items);
    let sources = prepared_sources(sources)?;
    Ok(Session {
        item_refs,
        catalog: discovery.catalog,
        files: discovery.files,
        skips: discovery.skips,
        claims: discovery.claims,
        sources,
        packages,
        items,
        proofs: established.proofs,
        reached,
        validated,
        eligible,
        targets,
        scratch,
        verified,
        established: std::sync::Mutex::new(super::EstablishmentState::fresh()),
        written_by_a_test,
        closure: read.closure,
        inputs: read.inputs,
        manifests: read.manifests,
        executions: std::sync::Mutex::new(0),
        leaders: crate::orphan::Leaders::default(),
        mutant_timeout: options.mutant_timeout,
        mutant_steps: options.mutant_steps,
        harness_args: options.harness_args.clone(),
        scratch_working_directory: options.scratch_working_directory,
        workspace,
    })
}

/// Turns the mutable-file snapshot into the text a prepared session exposes.
///
/// Discovery parses Rust source as UTF-8, but the immutable session boundary checks that fact again instead of allowing every later position lookup to reinterpret an encoding failure as an absent position.
fn prepared_sources(
    sources: BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<String, String>, EngineError> {
    sources
        .into_iter()
        .map(|(path, source)| {
            String::from_utf8(source)
                .map(|source| (path.clone(), source))
                .map_err(|invalid| {
                    SessionError::SelectionSourceNotUtf8 {
                        path,
                        source: invalid.utf8_error(),
                    }
                    .into()
                })
        })
        .collect()
}

/// The verification with what the instrumented tree can say about a mutant it never named folded in, and the items its entry markers name.
fn narrowed(
    verified: Verified,
    narrowing: crate::touch::Narrowing,
    items: Vec<crate::touch::Item>,
) -> Verified {
    Verified {
        touched: crate::touch::Touched {
            narrowing,
            items,
            ..verified.touched
        },
        ..verified
    }
}

/// Every item of every mutable file, numbered in path order.
fn cataloged_items(
    discovery: &discover::Discovery,
    sources: &BTreeMap<String, Vec<u8>>,
) -> Result<crate::instrument::ItemCatalog, EngineError> {
    let files: Vec<crate::instrument::ItemSource<'_>> = discovery
        .files
        .iter()
        .filter_map(|file| {
            sources
                .get(&file.path)
                .map(|source| crate::instrument::ItemSource {
                    path: &file.path,
                    package: &file.package,
                    source,
                })
        })
        .collect();
    Ok(crate::instrument::catalog_items(&files)?)
}

/// Every cataloged item's portable name, by item index, which is what an entered union is written in.
///
/// # Errors
/// An index the catalog numbers names no item it holds, which a dense catalog never does.
fn item_refs(
    catalog: &crate::instrument::ItemCatalog,
) -> Result<Vec<crate::touch::ItemRef>, EngineError> {
    catalog
        .items
        .iter()
        .map(|item| {
            catalog.item_ref(item.index).ok_or_else(|| {
                EngineError::from(SessionError::ItemCatalogGap { index: item.index })
            })
        })
        .collect()
}

/// How many items the catalog holds, which is what a record of an entered item is checked against.
fn item_count(items: &crate::instrument::ItemCatalog) -> Result<u32, EngineError> {
    u32::try_from(items.items.len()).map_err(|_outside_range| {
        EngineError::from(SessionError::TraceCountTooLarge {
            subject: "cataloged items",
            count: items.items.len(),
        })
    })
}

/// What instrumenting and validating the tree established, which is everything a run needs about the tree it will start.
struct Instrumented {
    /// Which mutants compile, and what the rounds cost.
    validated: Validated,
    /// The messages of the last attempt that compiled, which name the test binaries this session will run.
    last_build: Vec<crate::cargo::Message>,
    /// What the tree that was built can say about a mutant it never named, which is what narrowing by silence rests on.
    narrowing: crate::touch::Narrowing,
    /// Every item of the tree, numbered as its entry markers name them.
    items: crate::instrument::ItemCatalog,
}

/// What instrumenting the tree is done from: the snapshot to write into, the mutants to place, and what the proof layers established about them.
#[derive(Debug, Clone, Copy)]
struct Establishing<'a> {
    /// The workspace whose snapshot the instrumented tree is written into.
    workspace: &'a Workspace,
    /// What there is to mutate, with the catalog every guard names.
    discovery: &'a discover::Discovery,
    /// The pristine bytes of every mutable file.
    sources: &'a BTreeMap<String, Vec<u8>>,
    /// The mutants placed in each file.
    placements: &'a BTreeMap<String, Vec<Placement>>,
    /// What the proof layers established about them: the branch proofs, and which guards may compare their two branches.
    established: &'a crate::prove::Established,
    /// The catalog indices this preparation will validate and place.
    eligible: &'a BTreeSet<u32>,
    /// What the run was asked to prepare.
    options: &'a PrepareOptions,
}

/// Instruments the tree and lets the compiler say which mutants are real, returning what it established and the build it ended with.
fn establish(
    asking: &Establishing<'_>,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<Instrumented, EngineError> {
    let Establishing {
        workspace,
        discovery,
        sources,
        placements,
        established,
        eligible,
        options,
    } = *asking;
    let items = cataloged_items(discovery, sources)?;
    let mut writer = TreeCompiler {
        first_items: &items.first,
        workspace,
        sources,
        placements,
        catalog: &discovery.catalog,
        cancel,
        timeout: Workspace::timeout(options.build_timeout),
        last_build: Vec::new(),
        packages: options.packages.clone(),
        build: options.build.clone(),
        written: BTreeMap::new(),
        markers: marked(&discovery.catalog, &established.proofs, eligible),
        comparable: &established.comparable,
        probed: &established.probed,
        compared: BTreeSet::new(),
        marked: BTreeSet::new(),
    };
    let validated = validate_selected(
        &discovery.catalog,
        eligible,
        &mut writer,
        &Validating {
            options: ValidateOptions {
                max_rounds: options.max_rounds,
            },
            cancel,
            trace,
        },
    )?;
    Ok(Instrumented {
        validated,
        last_build: writer.last_build,
        narrowing: crate::touch::Narrowing {
            compared: writer.compared,
            bodies: resting(&established.proofs, &writer.marked),
        },
        items,
    })
}

/// The marker each mutant's branch proof rests on, keeping only the markers the instrumenter wrote.
fn resting(
    proofs: &BTreeMap<u32, crate::syntax::branch::Proof>,
    marked: &BTreeSet<u32>,
) -> BTreeMap<u32, u32> {
    proofs
        .iter()
        .filter_map(|(index, proof)| {
            proof
                .marker
                .filter(|marker| marked.contains(&marker.index))
                .map(|marker| (*index, marker.index))
        })
        .collect()
}

/// What a test wrote into the tree while the proof layers ran, which is drift about the project rather than about the run.
fn resealed(
    workspace: &mut Workspace,
    sources: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<Drift>, EngineError> {
    Ok(workspace
        .snapshot
        .reseal()?
        .into_iter()
        .filter(|drift| !sources.contains_key(drift.rel_path()))
        .collect())
}

/// Reads every mutable file of the snapshot and pairs its candidates with their catalog entries.
/// Files without a candidate are retained because a mutation activated elsewhere can enter their loops or functions later in the same process, and those boundaries share the same step allowance.
type Planned = (BTreeMap<String, Vec<u8>>, BTreeMap<String, Vec<Placement>>);

fn plan_tree(root: &Path, discovery: &discover::Discovery) -> Result<Planned, EngineError> {
    let found: Vec<Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let mut sources: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut placements: BTreeMap<String, Vec<Placement>> = BTreeMap::new();
    for file in &discovery.files {
        if file.whole_file.is_some() {
            continue;
        }
        let source =
            std::fs::read(root.join(&file.path)).map_err(|source| SessionError::WriteFailed {
                path: file.path.clone(),
                source,
            })?;
        sources.insert(file.path.clone(), source);
        placements.insert(
            file.path.clone(),
            plan_file(&discovery.catalog, &file.path, &found)?,
        );
    }
    Ok((sources, placements))
}

/// The catalog indices compiler validation has to decide for this preparation.
fn eligible(
    catalog: &Catalog,
    sources: &BTreeMap<String, Vec<u8>>,
    filter: Option<&crate::run::Filter>,
) -> Result<BTreeSet<u32>, EngineError> {
    let Some(filter) = filter.filter(|filter| !filter.is_empty()) else {
        return Ok(catalog
            .mutants()
            .iter()
            .map(|mutant| mutant.index)
            .collect());
    };
    let mut selected = BTreeSet::new();
    for mutant in catalog.mutants() {
        let path = &mutant.candidate.path;
        let source = sources
            .get(path)
            .ok_or_else(|| SessionError::SelectionSourceMissing { path: path.clone() })?;
        let text =
            std::str::from_utf8(source).map_err(|source| SessionError::SelectionSourceNotUtf8 {
                path: path.clone(),
                source,
            })?;
        let index =
            LineIndex::new(text).map_err(|source| SessionError::SelectionPositionInvalid {
                path: path.clone(),
                source,
            })?;
        let line = index
            .position(mutant.candidate.span.start)
            .map_err(|source| SessionError::SelectionPositionInvalid {
                path: path.clone(),
                source,
            })?
            .line;
        if filter.selects(mutant, line) {
            selected.extend([mutant.index]);
        }
    }
    Ok(selected)
}

/// Keeps only placements validation was asked to decide, without renumbering them.
/// Empty files remain in the plan so their function and loop boundaries still charge a mutation selected in another file.
fn selected_placements(
    placements: BTreeMap<String, Vec<Placement>>,
    eligible: &BTreeSet<u32>,
) -> BTreeMap<String, Vec<Placement>> {
    placements
        .into_iter()
        .map(|(path, placements)| {
            let selected: Vec<Placement> = placements
                .into_iter()
                .filter(|placement| eligible.contains(&placement.index))
                .collect();
            (path, selected)
        })
        .collect()
}

/// How many lines a byte string holds.
fn lines(bytes: &[u8]) -> Result<u64, ValidateError> {
    let count = crate::splice::count_lines(bytes);
    u64::try_from(count).map_err(|_overflow| ValidateError::AttemptFailed {
        message: format!("source line count {count} does not fit the trace wire"),
    })
}

/// How many lines the rewritten body holds, the appended runtime excluded.
fn body_lines(file: &FileOutput) -> Result<u64, ValidateError> {
    let text = file.text.as_bytes();
    match file.text.rfind("\n#[doc(hidden)]") {
        Some(at) => {
            let body = text
                .get(..=at)
                .ok_or_else(|| ValidateError::AttemptFailed {
                    message: format!("runtime boundary {at} is not a source byte boundary"),
                })?;
            lines(body)
        }
        None => lines(text),
    }
}

/// The markers each file carries, by the file the bodies they mark are in.
fn marked(
    catalog: &Catalog,
    proofs: &BTreeMap<u32, crate::syntax::branch::Proof>,
    eligible: &BTreeSet<u32>,
) -> BTreeMap<String, Vec<crate::syntax::branch::Marker>> {
    let mut by_file: BTreeMap<String, BTreeSet<crate::syntax::branch::Marker>> = BTreeMap::new();
    for (index, proof) in proofs {
        if !eligible.contains(index) {
            continue;
        }
        let Some(marker) = proof.marker else {
            continue;
        };
        let Some(mutant) = catalog.by_index(*index) else {
            continue;
        };
        by_file
            .entry(mutant.candidate.path.clone())
            .or_default()
            .extend([marker]);
    }
    by_file
        .into_iter()
        .map(|(path, markers)| (path, markers.into_iter().collect()))
        .collect()
}

/// Instruments the snapshot with a set of mutants left out and compiles it: the [`Compile`] seam validation drives.
struct TreeCompiler<'a> {
    workspace: &'a Workspace,
    sources: &'a BTreeMap<String, Vec<u8>>,
    placements: &'a BTreeMap<String, Vec<Placement>>,
    pub(super) catalog: &'a Catalog,
    pub(super) cancel: &'a Cancel,
    timeout: Option<Duration>,
    /// The messages of the last attempt that compiled, which name the test binaries this session will run.
    last_build: Vec<crate::cargo::Message>,
    /// The member packages the run is about, which are the ones whose test binaries it will start.
    packages: Vec<String>,
    /// What the project is compiled as, which every attempt compiles the same way.
    build: crate::cargo::BuildConfig,
    /// What each file held when this last wrote it, so a round writes only what its condemnations changed.
    written: BTreeMap<String, String>,
    /// The markers each file's branch proofs put in it, so entering a body is a thing the guards record.
    markers: BTreeMap<String, Vec<crate::syntax::branch::Marker>>,
    /// Every mutant whose guard may compare its two branches, so a run records whether they ever differed.
    comparable: &'a BTreeSet<u32>,
    /// Every return replacement whose guard may ask what the value it replaces already held, with the question to ask.
    probed: &'a BTreeMap<u32, crate::probe::Question>,
    /// Every mutant whose guard in the tree that was last built actually does compare them, which is what a proof may rest on.
    compared: BTreeSet<u32>,
    /// Every marker the tree that was last built actually holds the call for.
    marked: BTreeSet<u32>,
    /// The item index each file's first item takes.
    first_items: &'a BTreeMap<String, u32>,
}

impl TreeCompiler<'_> {
    /// Instruments and writes one planned file, reporting whether its bytes changed since the preceding validation round.
    fn instrument_one(
        &mut self,
        path: &str,
        kept: &[Placement],
    ) -> Result<(FileOutput, bool), ValidateError> {
        let source = self
            .sources
            .get(path)
            .ok_or_else(|| ValidateError::AttemptFailed {
                message: format!("{path} was never read"),
            })?;
        let file = instrument_file(&crate::instrument::Instrumenting {
            path,
            source,
            placements: kept,
            markers: self.markers.get(path).map_or(&[], Vec::as_slice),
            comparable: self.comparable,
            probed: self.probed,
            catalog_digest: self.catalog.digest(),
            watched: self.workspace.watched(),
            first_item: self.first_items.get(path).copied().ok_or_else(|| {
                ValidateError::AttemptFailed {
                    message: format!("{path} was never given item indices"),
                }
            })?,
        })?;
        let guards =
            u32::try_from(file.guards.len()).map_err(|_overflow| ValidateError::AttemptFailed {
                message: format!(
                    "{} guards in {path} do not fit the trace wire",
                    file.guards.len()
                ),
            })?;
        self.workspace.trace.instrument(InstrumentRecord {
            path: path.to_owned(),
            guards,
            module: file.module.clone(),
            lines_before: lines(source)?,
            lines_after: body_lines(&file)?,
        });
        let changed = rewrite_needed(self.written.get(path), &file.text);
        if changed {
            std::fs::write(self.workspace.snapshot_root().join(path), &file.text).map_err(
                |error| ValidateError::AttemptFailed {
                    message: format!("cannot write {path}: {error}"),
                },
            )?;
            if self
                .written
                .insert(path.to_owned(), file.text.clone())
                .as_ref()
                .is_some_and(|previous| previous == &file.text)
            {
                return Err(ValidateError::AttemptFailed {
                    message: format!("unchanged instrumentation for {path} was rewritten"),
                });
            }
        }
        Ok((file, changed))
    }
}

impl Compile for TreeCompiler<'_> {
    fn attempt(&mut self, condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
        let mut files: Vec<FileOutput> = Vec::new();
        let mut written: u32 = 0;
        let planned: Vec<(String, Vec<Placement>)> = self
            .placements
            .iter()
            .map(|(path, placements)| {
                let kept = placements
                    .iter()
                    .filter(|placement| !condemned.contains(&placement.index))
                    .cloned()
                    .collect();
                (path.clone(), kept)
            })
            .collect();
        for (path, kept) in planned {
            let (file, changed) = self.instrument_one(&path, &kept)?;
            if changed {
                written = written
                    .checked_add(1)
                    .ok_or_else(|| ValidateError::AttemptFailed {
                        message: "rewritten-file count exceeds the validation wire".to_owned(),
                    })?;
            }
            files.push(file);
        }
        let compiled = compile(
            &self.workspace.driver(self.cancel),
            &CompileOptions {
                kind: CompileKind::Tests,
                packages: self.packages.clone(),
                target_dir: Some(self.workspace.target_dir.clone()),
                locked: self.workspace.locked,
                offline: self.workspace.offline,
                timeout: self.timeout,
                env: vec![(
                    std::ffi::OsString::from(crate::instrument::COMPILED_CATALOG_ENV),
                    std::ffi::OsString::from(self.catalog.digest()),
                )],
                build: self.build.clone(),
            },
        )?;
        let success = compiled.success;
        if success {
            self.last_build.clone_from(&compiled.messages);
            self.compared = files
                .iter()
                .flat_map(|file| file.compared.iter().copied())
                .collect();
            self.marked = files
                .iter()
                .flat_map(|file| file.marked.iter().copied())
                .collect();
        }
        Ok(Attempt {
            files,
            messages: compiled.messages,
            success,
            written,
        })
    }
}

/// Whether a round has to write this file again: only what its condemnations changed.
#[must_use]
pub fn rewrite_needed(written: Option<&String>, next: &str) -> bool {
    written.is_none_or(|last| last != next)
}
