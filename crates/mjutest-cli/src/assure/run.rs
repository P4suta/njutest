// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One verification, from a request to a report.

use std::collections::BTreeSet;
use std::path::PathBuf;

use jiff::Timestamp;

use crate::assure::baseline::{self, BaselineOptions, Workspace};
use crate::assure::mutation::{self, MutationOptions, Subject};
use crate::build::{Cargo, Selection};
use crate::cli::Environment;
use crate::config::Config;
use crate::error::RunnerError;
use crate::git;
use crate::report::{
    Finding, FindingKind, Limitation, MutantRecord, Report, RunKind, SoundnessAccounting,
    TargetRecord, TargetStatus, Toolchain, UNAVAILABLE, Verdict,
};
use crate::scratch::{self, Scratch};
use crate::soundness;
use crate::ui::Notes;
use crate::watch::Watch;
use crate::{build_cache, rustflags};
use rust_mutants::cargo::Metadata;

/// The limitation a run states when the tree could not be read as one number.
pub const DIGEST_LIMITATION: &str = "workspace-digest-not-computed";

/// The limitation a run states when a test wrote into the tree it was being measured in.
pub const DRIFT_LIMITATION: &str = "tree-written-during-measurement";

/// What one run was asked to do.
#[derive(Debug, Clone)]
pub struct Request {
    /// The workspace root.
    pub root: PathBuf,
    /// The effective configuration.
    pub config: Config,
    /// Packages the command line asked for, which narrow the configuration's.
    pub packages: Vec<String>,
    /// Arguments for the test binaries, after `--`.
    pub test_args: Vec<String>,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// Preserve the directories the run worked in.
    pub keep_temp: bool,
    /// The run's identity.
    pub run_id: String,
    /// When it started.
    pub started: Timestamp,
    /// Where the engine records its own stream.
    pub engine_trace: rust_mutants::trace::Recorder,
    /// What this run is, as numbers. Empty when the tree could not be read, which states a limitation rather than failing the run.
    pub evidence: crate::assure::identity::Evidence,
    /// The change set to mutate within, when the run was asked for one.
    pub changed: Option<git::Change>,
    /// Where scheduling state for an interrupted run is kept. `None` keeps none, which is what a run told to establish everything afresh does.
    pub checkpoints: Option<PathBuf>,
    /// Where what earlier runs established about individual mutants is kept. `None` establishes everything afresh.
    pub evidence_store: Option<PathBuf>,
}

/// What one run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The report, audited and ready to persist.
    pub report: Report,
    /// What the run preserved, for a person to look at.
    pub kept: Vec<PathBuf>,
}

/// Runs one verification.
///
/// # Errors
/// Whatever stopped a phase from observing anything: no scratch directory,
/// no toolchain, a build that could not be started, a coverage tool that
/// failed. A workspace that does not compile and a test that fails are not
/// errors — they are findings in the report.
pub fn run(
    request: &Request,
    environment: &Environment,
    notes: &mut Notes<'_>,
    watch: Watch<'_>,
) -> Result<Outcome, RunnerError> {
    let mut report = identity(request);
    notes.phase("open");
    let scratch = Scratch::create(
        &environment.temp_directory,
        &request.run_id,
        request.started,
    )?;
    open_phase(
        &mut report,
        &Opening {
            request,
            environment,
            scratch: &scratch,
        },
        watch,
    );
    if !report.repository.git.available {
        report.limitations.push(Limitation::new(
            git::UNAVAILABLE_LIMITATION,
            "git could not be asked, so the run cannot name the commit it verified",
        ));
    }

    let (toolchain, metadata) = locate(request, environment, watch)?;
    report.toolchain = describe(&toolchain);
    report.repository.packages = metadata
        .packages
        .iter()
        .map(|package| package.name.clone())
        .collect();
    report.scope.resolved_packages = resolved(request, &report.repository.packages);

    notes.phase("soundness");
    take_inventory(&mut report, request, &metadata);
    let layer = layer_for(&toolchain, environment, &scratch, notes)?;

    notes.phase("baseline");
    let restore = resume_state(request, &mut report);
    let mut journal = Journal::of(request, restore.as_ref());
    let baseline = baseline::run_resuming(
        Workspace {
            toolchain: &toolchain,
            packages: &metadata.packages,
        },
        &baseline_options(
            &Opening {
                request,
                environment,
                scratch: &scratch,
            },
            &report,
            layer,
        ),
        &mut baseline::Resume {
            state: restore.as_ref(),
            record: &mut |measured| journal.keep_target(measured),
        },
        baseline::Reporting { notes, watch },
    )?;
    absorb(&mut report, &baseline);

    if baseline.failure.is_none() {
        run_mutation(
            &mut Mutating {
                report: &mut report,
                request,
                environment,
                baseline: &baseline,
                metadata: &metadata,
                restore: restore.as_ref(),
                journal: &mut journal,
            },
            notes,
            watch,
        )?;
    }
    finish(&mut report, request.started);
    journal.finished();
    let kept = if request.keep_temp {
        scratch.keep()
    } else {
        scratch.close()
    };
    Ok(Outcome { report, kept })
}

/// The report as it is before anything has run: what the run is, what it was asked to verify, and what it already knows it will not claim.
fn identity(request: &Request) -> Report {
    let mut report = Report::new(&request.run_id, kind_of(request), request.config.contract);
    report.timing.started = request.started.to_string();
    report.repository.root_name = root_name(&request.root);
    report.repository.configuration_digest = request.config.digest();
    if request.evidence.is_known() {
        report
            .repository
            .workspace_digest
            .clone_from(&request.evidence.tree);
        report
            .provenance
            .identity
            .clone_from(&request.evidence.identity);
    } else {
        UNAVAILABLE.clone_into(&mut report.repository.workspace_digest);
    }
    report.scope.requested_packages = requested(request);
    report
        .scope
        .excluded
        .clone_from(&request.config.project.exclude);
    if !request.evidence.is_known() {
        report.limitations.push(Limitation::new(
            DIGEST_LIMITATION,
            "the tree could not be read as one number, so no result of this run can be \
             reused by another",
        ));
    }
    report
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// What the baseline is asked to build and run.
fn baseline_options(opening: &Opening<'_>, report: &Report, layer: PathBuf) -> BaselineOptions {
    let Opening {
        request,
        environment,
        scratch,
    } = *opening;
    BaselineOptions {
        root: request.root.clone(),
        selection: Selection {
            packages: report.scope.resolved_packages.clone(),
            features: request.config.execution.features.clone(),
            all_features: request.config.execution.all_features,
            default_features: !request.config.execution.no_default_features,
        },
        cargo: request.cargo,
        env: environment.vars.clone(),
        target_dir: layer,
        scratch_build_dir: scratch.build_dir(),
        profiles_dir: scratch.profiles_dir(),
        timeout: Some(request.config.execution.timeout),
        test_args: request.test_args.clone(),
    }
}

/// What an interrupted run left for this one, or nothing. A state this release cannot continue from is a state it does not continue from: the run starts cold and says so.
fn resume_state(request: &Request, report: &mut Report) -> Option<crate::checkpoint::State> {
    let directory = request.checkpoints.as_ref()?;
    if !request.evidence.is_known() {
        return None;
    }
    let state = match crate::checkpoint::read(directory, &request.evidence.identity) {
        Ok(state) => state?,
        Err(_unusable) => return None,
    };
    if state.is_empty() {
        return None;
    }
    report.limitations.push(Limitation::new(
        crate::checkpoint::RESUMED_LIMITATION,
        &format!(
            "an interrupted run had already measured {} targets and established {} mutants; \
             a restored target carries the files it reached and not the regions inside them, \
             so it keeps reaching its whole file",
            state.targets.len(),
            state.mutants.len()
        ),
    ));
    Some(state)
}

/// What this run has established so far, written where an interrupted run's successor will find it.
struct Journal {
    directory: Option<PathBuf>,
    state: crate::checkpoint::State,
}

impl Journal {
    fn of(request: &Request, restore: Option<&crate::checkpoint::State>) -> Self {
        let mut state = restore
            .cloned()
            .unwrap_or_else(|| crate::checkpoint::State::new(&request.evidence.identity));
        state.attempts = state.attempts.saturating_add(1);
        Self {
            directory: request
                .checkpoints
                .clone()
                .filter(|_directory| request.evidence.is_known()),
            state,
        }
    }

    fn keep_target(&mut self, measured: &baseline::Measured) {
        self.state.record_target(crate::checkpoint::SavedTarget {
            id: measured.target.id.clone(),
            status: measured.status,
            duration_ms: measured.duration_ms,
            message: measured.message.clone(),
            files: measured
                .covered
                .iter()
                .map(|block| block.file.to_string_lossy().into_owned())
                .collect::<BTreeSet<String>>()
                .into_iter()
                .collect(),
        });
        self.write();
    }

    fn keep_mutant(&mut self, judged: &mutation::Judged) {
        let (disposition, by) = match &judged.disposition {
            mutation::Disposition::Killed { by } => ("killed", by.clone()),
            mutation::Disposition::TimedOut { on } => ("timed_out", on.clone()),
            _ => return,
        };
        self.state.record_mutant(crate::checkpoint::SavedMutant {
            id: judged.id.clone(),
            disposition: disposition.to_owned(),
            killed_by: Some(by),
            duration_ms: 0,
        });
        self.write();
    }

    /// A checkpoint that cannot be written is a run that cannot be continued, which is not a reason to stop the run that is under way.
    fn write(&self) {
        if let Some(directory) = &self.directory {
            drop(crate::checkpoint::write(directory, &self.state));
        }
    }

    fn finished(&self) {
        if let Some(directory) = &self.directory {
            crate::checkpoint::clear(directory, &self.state.identity);
        }
    }
}

/// Where a run works and what it is about, before it has compiled anything.
struct Opening<'a> {
    request: &'a Request,
    environment: &'a Environment,
    scratch: &'a Scratch,
}

/// What the run can say before it has compiled anything: where it works, and what the repository was.
fn open_phase(report: &mut Report, opening: &Opening<'_>, watch: Watch<'_>) {
    let Opening {
        request,
        environment,
        scratch,
    } = *opening;
    if !scratch.is_claimed() {
        report.limitations.push(Limitation::new(
            scratch::UNCLAIMED_LIMITATION,
            "the run works in a directory it could not claim, so a sweep may remove it \
             while the run is still using it",
        ));
    }
    report.repository.git = git::describe(&request.root, &environment.vars, watch);
    let Some(change) = &request.changed else {
        return;
    };
    report
        .repository
        .git
        .merge_base
        .clone_from(&change.merge_base);
    report
        .repository
        .git
        .changed_files
        .clone_from(&change.files);
}

/// The limitation a run states when it counted the places the compiler stops vouching for and did not execute any of them.
pub const SOUNDNESS_LIMITATION: &str = "soundness-not-executed";

/// The limitation a run states when a file it inventoried could not be read as Rust.
pub const SOUNDNESS_UNREADABLE_LIMITATION: &str = "soundness-source-unreadable";

/// Counts every place a selected package steps outside what the compiler guarantees.
///
/// `standard-v1` does not execute any of them: a non-empty inventory is a
/// limitation the report states rather than a claim it makes (ADR 0009).
fn take_inventory(report: &mut Report, request: &Request, metadata: &Metadata) {
    let selected: Vec<(String, PathBuf)> = metadata
        .packages
        .iter()
        .filter(|package| {
            report.scope.resolved_packages.is_empty()
                || report.scope.resolved_packages.contains(&package.name)
        })
        .filter_map(|package| {
            let directory = package.manifest_path.parent()?;
            Some((package.name.clone(), directory.to_path_buf()))
        })
        .collect();
    let Ok(taken) = soundness::inventory(&request.root, &selected) else {
        report.limitations.push(Limitation::new(
            SOUNDNESS_UNREADABLE_LIMITATION,
            "the tree could not be walked for the places the compiler stops vouching for, so \
             the run makes no claim about them",
        ));
        return;
    };
    report.accounting.soundness = SoundnessAccounting {
        unsafe_items: count(taken.items.len()),
        packages_with_unsafe: count(taken.packages.len()),
        executed: false,
    };
    if !taken.unreadable.is_empty() {
        report.limitations.push(Limitation::new(
            SOUNDNESS_UNREADABLE_LIMITATION,
            &format!(
                "{} files could not be read as Rust this release understands, so what they \
                 hold is not in the inventory: {}",
                taken.unreadable.len(),
                taken.unreadable.join(", ")
            ),
        ));
    }
    if !taken.is_empty() {
        report.limitations.push(Limitation::new(
            SOUNDNESS_LIMITATION,
            &format!(
                "{} places in {} packages step outside what the compiler guarantees, and this \
                 contract counts them rather than executing them",
                taken.items.len(),
                taken.packages.len()
            ),
        ));
    }
}

/// How much of the workspace this run looked at.
///
/// A run that named packages looked at those, and the contract reserves
/// `ASSURED` for a run that looked at everything.
fn kind_of(request: &Request) -> RunKind {
    if request.changed.is_some() {
        RunKind::Changed
    } else if requested(request).is_empty() {
        RunKind::Full
    } else {
        RunKind::Scoped
    }
}

/// The toolchain that will build the tree, and what it says the workspace holds. Located inside the workspace, so a `rust-toolchain.toml` there is what answers.
fn locate(
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<(rust_mutants::cargo::Toolchain, Metadata), RunnerError> {
    let toolchain = rust_mutants::cargo::Toolchain::locate(
        &rust_mutants::cargo::LocateOptions {
            cargo: None,
            search_path: environment.var("PATH").map(std::ffi::OsStr::to_owned),
            env: Some(environment.vars.clone()),
        },
        &request.root,
        watch.cancel,
    )
    .map_err(rust_mutants::EngineError::from)?;
    let metadata = Metadata::load(
        &rust_mutants::cargo::Driver {
            toolchain: &toolchain,
            dir: &request.root,
            cancel: watch.cancel,
            trace: &rust_mutants::trace::Recorder::disabled(),
        },
        rust_mutants::cargo::MetadataOptions {
            locked: request.cargo.locked,
            offline: request.cargo.offline,
        },
    )
    .map_err(rust_mutants::EngineError::from)?;
    Ok((toolchain, metadata))
}

/// Where the instrumented build goes: the machine's coverage layer, or a directory inside this run's scratch when that layer cannot be used. A build cache is never a reason to fail ([ADR 0005] §7).
fn layer_for(
    toolchain: &rust_mutants::cargo::Toolchain,
    environment: &Environment,
    scratch: &Scratch,
    notes: &mut Notes<'_>,
) -> Result<PathBuf, RunnerError> {
    let version = toolchain.rustc_version();
    let cache = build_cache::BuildCache::new(
        &environment.cache_directory.join("mjutest"),
        version.commit_hash.as_deref().unwrap_or(&version.release),
    );
    match cache.prepare(build_cache::Layer::Coverage) {
        Ok(dir) => Ok(dir),
        Err(error) => {
            notes.note("cache", &error.to_string());
            Ok(scratch.round_dir("layer")?)
        }
    }
}

/// The verdict, the canonical order, and how long it all took.
fn finish(report: &mut Report, started: Timestamp) {
    report.verdict = verdict(report);
    report.sort_targets();
    let finished = Timestamp::now();
    report.timing.finished = finished.to_string();
    report.timing.duration_ms =
        u64::try_from(finished.duration_since(started).as_millis()).unwrap_or(0);
}

/// Puts what the baseline observed into the report.
struct Mutating<'a> {
    report: &'a mut Report,
    request: &'a Request,
    environment: &'a Environment,
    baseline: &'a baseline::Baseline,
    metadata: &'a Metadata,
    restore: Option<&'a crate::checkpoint::State>,
    journal: &'a mut Journal,
}

fn run_mutation(
    mutating: &mut Mutating<'_>,
    notes: &mut Notes<'_>,
    watch: Watch<'_>,
) -> Result<(), RunnerError> {
    notes.phase("mutation");
    let session = prepare(mutating.request, mutating.environment, watch)?;
    let accepted: BTreeSet<String> = mutating
        .request
        .config
        .acceptance
        .iter()
        .map(|acceptance| acceptance.id.clone())
        .collect();
    let mutation = mutation::run_resuming(
        Subject {
            session: &session,
            baseline: &mutating.baseline.targets,
        },
        &MutationOptions {
            instrumented: mutating.baseline.instrumented.clone(),
            accepted: accepted.clone(),
            test_args: mutating.request.test_args.clone(),
            evidence: evidence_of(mutating),
        },
        &mut mutation::Resume {
            state: mutating.restore,
            record: &mut |judged| mutating.journal.keep_mutant(judged),
        },
        baseline::Reporting { notes, watch },
    )?;
    if !session.changes()?.is_empty() {
        mutating.report.limitations.push(Limitation::new(
            DRIFT_LIMITATION,
            "a test wrote into the tree while it was being measured, so every later \
             mutation was measured against what it wrote",
        ));
    }
    for path in session.close()? {
        notes.note("kept", &path.display().to_string());
    }
    record(mutating.report, &mutation, &accepted);
    Ok(())
}

fn prepare(
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<rust_mutants::session::Session, RunnerError> {
    let workspace = rust_mutants::workspace::Workspace::open(
        &request.root,
        rust_mutants::workspace::OpenOptions {
            cargo: None,
            search_path: environment.var("PATH").map(std::ffi::OsStr::to_owned),
            env: environment.vars.clone(),
            temp_directory: environment.temp_directory.clone(),
            report_directory: Some("reports".to_owned()),
            exclude: Vec::new(),
            keep_temp: request.keep_temp,
            offline: request.cargo.offline,
            locked: request.cargo.locked,
            trace: request.engine_trace.clone(),
        },
        watch.cancel,
    )?;
    Ok(workspace.prepare(
        &rust_mutants::session::PrepareOptions {
            packages: request.packages.clone(),
            include: within(request.changed.as_ref()),
            verify: true,
            probe: request.config.mutation.probe,
            build_timeout: Some(request.config.execution.timeout),
            mutant_timeout: Some(request.config.execution.timeout),
            ..rust_mutants::session::PrepareOptions::default()
        },
        watch.cancel,
    )?)
}

/// Where this run reads and writes what is established about individual mutants, or nothing when it may not.
///
/// Reuse is confined to a run that looked at the whole project with no
/// configured resources: a run that looked at less established less, and a
/// resource a run started is a fact about the world its tests ran in that no
/// key covers.
fn evidence_of(mutating: &Mutating<'_>) -> Option<mutation::Evidence> {
    let request = mutating.request;
    let root = request.evidence_store.as_ref()?;
    if mutating.report.run_kind != RunKind::Full || !request.config.resources.is_empty() {
        return None;
    }
    let keying = request.evidence.keying.as_ref()?;
    let mut standing = crate::evidence::store::Standing::default();
    let mut names = std::collections::BTreeMap::new();
    for measured in &mutating.baseline.targets {
        if measured.status != TargetStatus::Passed || measured.restored {
            continue;
        }
        let Some(id) = package_id(mutating.metadata, &measured.target.package) else {
            continue;
        };
        let linked = crate::evidence::key::linked_by(
            &crate::evidence::key::Reading {
                metadata: mutating.metadata,
                scan: &keying.scan,
                root: &request.root,
                dependencies: &keying.dependencies,
            },
            &id,
        );
        standing.passing.insert(
            measured.target.id.clone(),
            crate::evidence::key::behaviour(&linked, &keying.common),
        );
        names.insert(measured.target.id.clone(), measured.target.name());
    }
    Some(mutation::Evidence {
        root: root.clone(),
        run_id: request.run_id.clone(),
        standing,
        names,
    })
}

/// The package id cargo gave the package called `name`.
fn package_id(metadata: &Metadata, name: &str) -> Option<String> {
    metadata
        .packages
        .iter()
        .find(|package| package.name == name)
        .map(|package| package.id.clone())
}

/// The files a change set names, as patterns the engine mutates within. An empty list is every file, so a change set that names no Rust file at all gets one pattern nothing matches: a run about nothing changing must mutate nothing, not everything.
fn within(change: Option<&git::Change>) -> Vec<rust_mutants::glob::Pattern> {
    let Some(change) = change else {
        return Vec::new();
    };
    let sources: Vec<&String> = change
        .files
        .iter()
        .filter(|path| std::path::Path::new(path).extension() == Some(std::ffi::OsStr::new("rs")))
        .collect();
    if sources.is_empty() {
        return rust_mutants::glob::Pattern::compile(NOTHING_CHANGED)
            .map(|pattern| vec![pattern])
            .unwrap_or_default();
    }
    sources
        .into_iter()
        .filter_map(|path| rust_mutants::glob::Pattern::compile(path).ok())
        .collect()
}

/// The pattern a run about an empty change set mutates within. No file is called this.
pub const NOTHING_CHANGED: &str = ".mjutest-nothing-changed";

fn record(report: &mut Report, mutation: &mutation::Mutation, accepted: &BTreeSet<String>) {
    report.accounting.mutants = mutation.accounting(accepted);
    report.mutants = mutation
        .judged
        .iter()
        .map(|judged| MutantRecord {
            id: judged.id.clone(),
            display_id: judged.display_id.clone(),
            path: judged.path.clone(),
            position: judged.position.unwrap_or(crate::report::Position {
                line: 1,
                column: 1,
                character_column: 1,
            }),
            rule: judged.rule.clone(),
            outcome: judged.disposition.name().to_owned(),
            killed_by: judged.disposition.decided_by().map(ToOwned::to_owned),
            reused: judged.source_run_id.is_some(),
            source_run_id: judged.source_run_id.clone(),
        })
        .collect();
    report.findings.extend(mutation.findings(accepted));
    for (reason, count) in &mutation.skips {
        report.limitations.push(Limitation::new(
            &format!("skipped-{reason}"),
            &format!("{count} places were not mutated: {reason}"),
        ));
    }
}

fn absorb(report: &mut Report, baseline: &baseline::Baseline) {
    for name in &baseline.limitations {
        report
            .limitations
            .push(Limitation::new(name, &limitation_detail(name)));
    }
    if let Some(failure) = &baseline.failure {
        report.findings.push(Finding::new(
            FindingKind::BuildFailure,
            &report.repository.root_name,
            &first_line(failure),
        ));
        return;
    }
    for measured in &baseline.targets {
        let subject = measured.target.name();
        match measured.status {
            TargetStatus::Failed => report.findings.push(Finding::new(
                FindingKind::FailingTest,
                &subject,
                measured
                    .message
                    .as_deref()
                    .unwrap_or("the target failed and said nothing"),
            )),
            TargetStatus::Missing => report.findings.push(Finding::new(
                FindingKind::TargetMissing,
                &subject,
                measured
                    .message
                    .as_deref()
                    .unwrap_or("the target could not be found, so nothing was observed"),
            )),
            TargetStatus::Passed | TargetStatus::Skipped => {}
        }
        report.targets.push(TargetRecord {
            id: measured.target.id.clone(),
            name: subject,
            package: measured.target.package.clone(),
            status: measured.status,
            duration_ms: measured.duration_ms,
            message: measured.message.clone(),
        });
    }

    let counts = &mut report.accounting.targets;
    counts.selected = u32::try_from(report.targets.len()).unwrap_or(u32::MAX);
    for target in &report.targets {
        match target.status {
            TargetStatus::Passed => counts.passed = counts.passed.saturating_add(1),
            TargetStatus::Failed => counts.failed = counts.failed.saturating_add(1),
            TargetStatus::Skipped => counts.skipped = counts.skipped.saturating_add(1),
            TargetStatus::Missing => counts.missing = counts.missing.saturating_add(1),
        }
    }
}

/// What the observations support.
fn verdict(report: &Report) -> Verdict {
    if report
        .findings
        .iter()
        .any(|finding| finding.kind.is_defect())
    {
        return Verdict::Defect;
    }
    if !report.findings.is_empty() {
        return Verdict::Insufficient;
    }
    let observed = report.accounting.targets.passed > 0;
    let asked = report.accounting.mutants.executed > 0;
    if !observed || !asked {
        return Verdict::Insufficient;
    }
    match report.run_kind {
        RunKind::Full => Verdict::Assured,
        RunKind::Changed => Verdict::ChangeAssured,
        RunKind::Scoped => Verdict::ScopeAssured,
    }
}

/// The `rustc -vV` and `cargo -vV` facts a report records.
fn describe(toolchain: &rust_mutants::cargo::Toolchain) -> Toolchain {
    let rustc = toolchain.rustc_version();
    Toolchain {
        rustc: rustc.summary.clone(),
        cargo: toolchain.cargo_version().summary.clone(),
        target: rustc.host.clone(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
    }
}

/// The packages the command line asked for, or the configuration's.
fn requested(request: &Request) -> Vec<String> {
    if request.packages.is_empty() {
        request.config.project.packages.clone()
    } else {
        request.packages.clone()
    }
}

/// The packages the run settled on: what was asked for, or every member.
fn resolved(request: &Request, members: &[String]) -> Vec<String> {
    let asked = requested(request);
    if asked.is_empty() {
        Vec::new()
    } else {
        asked
            .into_iter()
            .filter(|name| members.contains(name))
            .collect()
    }
}

/// The name a person calls the workspace.
fn root_name(root: &std::path::Path) -> String {
    root.file_name().map_or_else(
        || UNAVAILABLE.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// One sentence of a compiler's several.
fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("the workspace does not compile")
        .to_owned()
}

/// What a named limitation means, for the ones a phase reports by name.
fn limitation_detail(name: &str) -> String {
    match name {
        rustflags::TARGET_RUSTFLAGS_LIMITATION => {
            "the project configures compiler flags for a target, and the instrumented build \
             does not merge them: which of them apply is cargo's decision"
        }
        rustflags::UNREADABLE_CONFIG_LIMITATION => {
            "a cargo configuration file could not be read, so the flags it asks for are not \
             in the instrumented build"
        }
        _ => "stated by a phase of the run",
    }
    .to_owned()
}
