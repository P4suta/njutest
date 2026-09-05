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
    Finding, FindingKind, Limitation, MutantRecord, Report, RunKind, TargetRecord, TargetStatus,
    Toolchain, UNAVAILABLE, Verdict,
};
use crate::scratch::{self, Scratch};
use crate::ui::Notes;
use crate::watch::Watch;
use crate::{build_cache, rustflags};

/// The limitation a run states while the evidence identity is not computed.
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
    if !scratch.is_claimed() {
        report.limitations.push(Limitation::new(
            scratch::UNCLAIMED_LIMITATION,
            "the run works in a directory it could not claim, so a sweep may remove it \
             while the run is still using it",
        ));
    }
    report.repository.git = git::describe(&request.root, &environment.vars, watch);
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

    let layer = layer_for(&toolchain, environment, &scratch, notes)?;

    notes.phase("baseline");
    let baseline = baseline::run(
        Workspace {
            toolchain: &toolchain,
            packages: &metadata.packages,
        },
        &BaselineOptions {
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
        },
        notes,
        watch,
    )?;
    absorb(&mut report, &baseline);

    if baseline.failure.is_none() {
        run_mutation(
            &mut Mutating {
                report: &mut report,
                request,
                environment,
                baseline: &baseline,
            },
            notes,
            watch,
        )?;
    }
    finish(&mut report, request.started);
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
    UNAVAILABLE.clone_into(&mut report.repository.workspace_digest);
    report.scope.requested_packages = requested(request);
    report
        .scope
        .excluded
        .clone_from(&request.config.project.exclude);
    report.limitations.push(Limitation::new(
        DIGEST_LIMITATION,
        "the evidence identity of the tree is not computed in this release, so no result \
         can be reused between runs",
    ));
    report
}

/// How much of the workspace this run looked at.
///
/// A run that named packages looked at those, and the contract reserves
/// `ASSURED` for a run that looked at everything.
fn kind_of(request: &Request) -> RunKind {
    if requested(request).is_empty() {
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
) -> Result<
    (
        rust_mutants::cargo::Toolchain,
        rust_mutants::cargo::Metadata,
    ),
    RunnerError,
> {
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
    let metadata = rust_mutants::cargo::Metadata::load(
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
    let mutation = mutation::run(
        Subject {
            session: &session,
            baseline: &mutating.baseline.targets,
        },
        &MutationOptions {
            instrumented: mutating.baseline.instrumented.clone(),
            accepted: accepted.clone(),
            test_args: mutating.request.test_args.clone(),
        },
        notes,
        watch,
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
            verify: true,
            build_timeout: Some(request.config.execution.timeout),
            mutant_timeout: Some(request.config.execution.timeout),
            ..rust_mutants::session::PrepareOptions::default()
        },
        watch.cancel,
    )?)
}

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
            reused: false,
            source_run_id: None,
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
