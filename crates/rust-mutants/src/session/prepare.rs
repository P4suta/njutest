// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Preparing a workspace: the gate it stands on, what the proof layers establish before anything is instrumented, and the one build every accepted mutant lives in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
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
use crate::syntax::{Found, Selection};
use crate::trace::{BuildRecord, InstrumentRecord};
use crate::validate::{
    Attempt, Compile, ValidateError, ValidateOptions, Validated, Validating, validate,
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
///
/// The check is also what says which files each target compiles **outside** a
/// test build, and a file no non-test unit compiled is one only the tests see:
/// without that, a library whose only compilation is its own test harness has
/// every one of its files read as test-only and nothing in it is worth
/// mutating. So the check is not only a gate and cannot be skipped.
///
/// Whether the tree *links* is a second question, and it used to be a second
/// compilation of the whole workspace on every run. It is not one any more.
/// The first validation round links the instrumented tree, and an instrumented
/// tree that links is one whose pristine form links too — the guards only add
/// code. A round that fails with nothing attributable to a mutation is a round
/// that compiles with nothing live, which is `RM4001`: the tree, in the
/// compiler's own words. The answer is the same and the successful run does
/// one build fewer.
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

/// The digest of the pristine sources every unit of the build compiled.
///
/// The bytes hashed are the ones the file held before anything was
/// instrumented: for a file with guards in it that is what the plan kept, and
/// for every other file it is the snapshot's own copy, which nothing wrote to.
/// Hashing the rewrite instead would tie the digest to the catalog, and a
/// catalog changes whenever any mutant anywhere does.
///
/// A build whose dep-info cannot be read yields nothing at all rather than a
/// partial answer, and a caller with nothing to key on remembers nothing.
fn closure_of(workspace: &Workspace, checked: &crate::cargo::Compiled) -> String {
    let root = workspace.snapshot_root();
    let units = &checked.units;
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for unit in units {
        for path in &unit.sources {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Ok(name) = crate::id::normalize_path(&relative.to_string_lossy()) else {
                continue;
            };
            if files.contains_key(&name) {
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            drop(files.insert(name, crate::id::digest(&bytes)));
        }
    }
    if files.is_empty() {
        return String::new();
    }
    folded(
        files
            .iter()
            .map(|(name, digest)| (name.as_str(), digest.as_str())),
    )
}

/// The digest of every manifest, the lock file, and the cargo configuration the build read.
fn manifests_of(workspace: &Workspace) -> String {
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
        let Ok(name) = crate::id::normalize_path(&relative.to_string_lossy()) else {
            continue;
        };
        if files.contains_key(&name) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        drop(files.insert(name, crate::id::digest(&bytes)));
    }
    folded(
        files
            .iter()
            .map(|(name, digest)| (name.as_str(), digest.as_str())),
    )
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
    /// Whether the guards are asked what they reached on the run that verifies the baseline.
    pub(super) asked: bool,
    /// The messages of the build the tree ended at, which name the test binaries to start.
    pub(super) last_build: &'a [crate::cargo::Message],
    /// What the run was asked to prepare.
    pub(super) options: &'a PrepareOptions,
}

fn built(building: &Building<'_>) -> Result<Built, EngineError> {
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
    );
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
    let skipped: Vec<String> = targets
        .iter()
        .filter(|target| options.skip_targets.iter().any(|one| one == &target.id))
        .map(|target| target.id.clone())
        .collect();
    targets.retain(|target| !skipped.contains(&target.id));
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
    let scratch = workspace.target_dir.join("scratch");
    std::fs::create_dir_all(&scratch).map_err(|source| SessionError::WriteFailed {
        path: scratch.display().to_string(),
        source,
    })?;
    let verified = if options.verify {
        verify(workspace, &mut targets, &scratch, building)?
    } else {
        Verified::default()
    };
    Ok((targets, scratch, verified))
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

/// What the proof layers establish before anything is instrumented: which tests could not have noticed a return replacement, which branch proofs the compiler vouches for, and which targets reached what.
type Layers = (
    crate::probe::tree::Probed,
    crate::prove::Established,
    crate::reach::Reached,
);

fn layers(
    asking: &crate::prove::Asking<'_>,
    remembering: Option<&crate::reach::remembered::Remembering>,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<Layers, EngineError> {
    let probed = if asking.options.probe {
        crate::probe::tree::establish(
            &crate::probe::tree::Asking {
                workspace: asking.workspace,
                discovery: asking.discovery,
                sources: asking.sources,
                options: asking.options,
            },
            cancel,
            trace,
        )?
    } else {
        crate::probe::tree::Probed::default()
    };
    let established = if asking.options.branch_proofs {
        crate::prove::establish(asking, cancel, trace)?
    } else {
        crate::prove::Established::default()
    };
    let reached = measured(asking, remembering, cancel, trace)?;
    Ok((probed, established, reached))
}

/// What measuring this tree established, made now or remembered from the last run that made it.
///
/// The measurement is the most expensive thing a run does: instrumenting for
/// coverage changes the fingerprint of every crate and rebuilds the whole
/// graph. It is also a function of the tree alone, which a mutation does not
/// change, so a tree nothing has touched since the last run has already been
/// measured. Reading that back is a whole build removed on the claim the
/// outcome store already rests on: nothing that could change the answer
/// changed.
///
/// A measurement that did not reach every target is not remembered. It is a
/// measurement of some of them — sound to route by, because what it could not
/// read stays in every route — and remembering it would hand every later run
/// of the tree a partial answer with nothing to tell it from a whole one.
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
            let id = located.found.candidate.id().ok()?;
            let mutant = discovery.catalog.by_id(&id)?;
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
    let closure = closure_of(workspace, &checked);
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
    Ok(Gated { discovery, closure })
}

/// What the gate established: what there is to mutate, and the digest of everything the build read.
struct Gated {
    discovery: discover::Discovery,
    closure: String,
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
    let Gated { discovery, closure } = gated(&workspace, options, cancel, &trace)?;
    let manifests = manifests_of(&workspace);

    let plan_phase = trace.phase("plan");
    let (sources, placements) = plan_tree(workspace.snapshot_root(), &discovery)?;
    plan_phase.end();

    let (probed, established, reached) = layers(
        &crate::prove::Asking {
            workspace: &workspace,
            discovery: &discovery,
            sources: &sources,
            options,
        },
        remembering(options, &closure, &manifests, &workspace).as_ref(),
        cancel,
        &trace,
    )?;

    let validate_phase = trace.phase("validate");
    let Instrumented {
        validated,
        last_build,
        narrowing,
    } = establish(
        &Establishing {
            workspace: &workspace,
            discovery: &discovery,
            sources: &sources,
            placements: &placements,
            established: &established,
            options,
        },
        cancel,
        &trace,
    )?;
    validate_phase.end();

    let mut workspace = workspace;
    let written_by_a_test = resealed(&mut workspace, &sources)?;
    let build_phase = trace.phase("build");
    let (targets, scratch, verified) = built(&Building {
        workspace: &workspace,
        cancel,
        trace: &trace,
        catalog: &discovery.catalog,
        asked: options.touch,
        last_build: &last_build,
        options,
    })?;
    build_phase.end();
    phase.end();
    let (packages, items) = attributed(&discovery);
    Ok(Session {
        catalog: discovery.catalog,
        skips: discovery.skips,
        claims: discovery.claims,
        sources,
        packages,
        items,
        proofs: established.proofs,
        reached,
        probed,
        validated,
        targets,
        scratch,
        baseline: verified.baseline,
        ran: verified.ran,
        touched: crate::touch::Touched {
            narrowing,
            ..verified.touched
        },
        filtered: std::sync::Mutex::new(BTreeMap::new()),
        established: std::sync::atomic::AtomicU64::new(0),
        written_by_a_test,
        closure,
        manifests,
        executions: std::sync::atomic::AtomicU64::new(0),
        mutant_timeout: options.mutant_timeout,
        workspace,
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
        options,
    } = *asking;
    let mut writer = TreeCompiler {
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
        markers: marked(&discovery.catalog, &established.proofs),
        comparable: &established.comparable,
        compared: BTreeSet::new(),
        marked: BTreeSet::new(),
    };
    let validated = validate(
        &discovery.catalog,
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
    })
}

/// The marker each mutant's branch proof rests on, keeping only the markers the instrumenter wrote.
///
/// A body inside a guard's own site takes no marker: the guard writes the site
/// twice and one splice cannot land in both. The proof survives with a
/// coverage region as its premise, and a run that measured no coverage has
/// none — so a mutant whose marker was dropped must not be discharged by a
/// record that was never going to name it.
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

/// Reads every mutable file of the snapshot and pairs its candidates with their catalog entries, which is everything instrumentation needs.
type Planned = (BTreeMap<String, Vec<u8>>, BTreeMap<String, Vec<Placement>>);

fn plan_tree(
    root: &std::path::Path,
    discovery: &discover::Discovery,
) -> Result<Planned, EngineError> {
    let found: Vec<Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let mut sources: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut placements: BTreeMap<String, Vec<Placement>> = BTreeMap::new();
    for file in &discovery.files {
        if file.candidates == 0 {
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

/// How many lines a byte string holds.
fn lines(bytes: &[u8]) -> u64 {
    u64::try_from(crate::splice::count_lines(bytes)).unwrap_or(u64::MAX)
}

/// How many lines the rewritten body holds, the appended runtime excluded.
fn body_lines(file: &FileOutput) -> u64 {
    let text = file.text.as_bytes();
    file.text
        .rfind("\n#[doc(hidden)]")
        .map_or_else(|| lines(text), |at| lines(text.get(..=at).unwrap_or(text)))
}

/// The markers each file carries, by the file the bodies they mark are in.
///
/// One body carries one marker however many claims rest on it, and the proof
/// of each of them names the same one, so what is written is the set of them
/// rather than the list.
fn marked(
    catalog: &Catalog,
    proofs: &BTreeMap<u32, crate::syntax::branch::Proof>,
) -> BTreeMap<String, Vec<crate::syntax::branch::Marker>> {
    let mut by_file: BTreeMap<String, BTreeSet<crate::syntax::branch::Marker>> = BTreeMap::new();
    for (index, proof) in proofs {
        let Some(marker) = proof.marker else {
            continue;
        };
        let Some(mutant) = catalog.by_index(*index) else {
            continue;
        };
        let _placed = by_file
            .entry(mutant.candidate.path.clone())
            .or_default()
            .insert(marker);
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
    /// Every mutant whose guard in the tree that was last built actually does compare them, which is what a proof may rest on.
    compared: BTreeSet<u32>,
    /// Every marker the tree that was last built actually holds the call for.
    marked: BTreeSet<u32>,
}

impl Compile for TreeCompiler<'_> {
    fn attempt(&mut self, condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
        let mut files: Vec<FileOutput> = Vec::new();
        let mut written: u32 = 0;
        for (path, placements) in self.placements {
            let kept: Vec<Placement> = placements
                .iter()
                .filter(|placement| !condemned.contains(&placement.index))
                .cloned()
                .collect();
            let source = self
                .sources
                .get(path)
                .ok_or_else(|| ValidateError::AttemptFailed {
                    message: format!("{path} was never read"),
                })?;
            let file = instrument_file(&crate::instrument::Instrumenting {
                path,
                source,
                placements: &kept,
                markers: self.markers.get(path).map_or(&[], Vec::as_slice),
                comparable: self.comparable,
                catalog_digest: self.catalog.digest(),
            })?;
            self.workspace.trace.instrument(InstrumentRecord {
                path: path.clone(),
                guards: u32::try_from(file.guards.len()).unwrap_or(u32::MAX),
                module: file.module.clone(),
                lines_before: lines(source),
                lines_after: body_lines(&file),
            });
            if rewrite_needed(self.written.get(path), &file.text) {
                std::fs::write(self.workspace.snapshot_root().join(path), &file.text).map_err(
                    |error| ValidateError::AttemptFailed {
                        message: format!("cannot write {path}: {error}"),
                    },
                )?;
                let _replaced = self.written.insert(path.clone(), file.text.clone());
                written = written.saturating_add(1);
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
                env: Vec::new(),
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
///
/// Every round instruments every mutable file, because attribution needs each
/// file's branch spans whatever it condemns. Writing them all back costs the
/// whole tree in bytes for every round, and a file whose live set did not
/// change holds what it already held.
#[must_use]
pub fn rewrite_needed(written: Option<&String>, next: &str) -> bool {
    written.is_none_or(|last| last != next)
}
