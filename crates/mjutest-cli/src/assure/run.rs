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
    watch.trace.stage("open");
    let scratch = Scratch::create(
        &environment.temp_directory,
        &request.run_id,
        request.started,
    )?;
    opened(&mut report, request, environment, (&scratch, watch));

    let (toolchain, metadata) = locate(request, environment, watch)?;
    surveyed(&mut report, request, (&toolchain, &metadata));

    notes.phase("soundness");
    watch.trace.stage("soundness");
    take_inventory(&mut report, request, &metadata);
    deepened(&mut report, request, (&toolchain, environment), watch)?;
    let layer = layer_for(&toolchain, environment, &scratch, notes)?;

    let mut resources = holding(request, environment, &mut report, (notes, watch))?;
    let held = with_resources(environment, &resources);
    let environment = &held;

    notes.phase("baseline");
    watch.trace.stage("baseline");
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
    afterwards(
        &mut report,
        (request, environment, &toolchain),
        (notes, watch),
    );
    released(&mut resources, &mut report);
    finish(&mut report, request.started);
    journal.finished();
    let kept = if request.keep_temp {
        scratch.keep()
    } else {
        scratch.close()
    };
    Ok(Outcome { report, kept })
}

/// What the toolchain and the workspace are, before anything is built.
fn surveyed(
    report: &mut Report,
    request: &Request,
    found: (&rust_mutants::cargo::Toolchain, &Metadata),
) {
    let (toolchain, metadata) = found;
    report.toolchain = describe(toolchain);
    report.repository.packages = metadata
        .packages
        .iter()
        .map(|package| package.name.clone())
        .collect();
    report.scope.resolved_packages = resolved(request, &report.repository.packages);
}

/// What the `deep-v1` contract adds to a run: the suite interpreted under Miri, and run again under every sanitizer the configuration asks for.
///
/// # Errors
/// A toolchain with no Miri, which a contract that promises interpretation
/// cannot answer for. A sanitizer that will not run is a limitation, because
/// it is asked for by configuration rather than promised by the contract.
fn deepened(
    report: &mut Report,
    request: &Request,
    with: (&rust_mutants::cargo::Toolchain, &Environment),
    watch: Watch<'_>,
) -> Result<(), RunnerError> {
    if request.config.contract != crate::config::Contract::DeepV1 {
        return Ok(());
    }
    let (toolchain, environment) = with;
    let done = super::deep::interpret(
        &super::deep::Interpreting {
            root: &request.root,
            cargo: toolchain.cargo(),
            env: environment.vars.clone(),
            packages: &report.scope.resolved_packages,
            flags: &request.config.soundness.miri_flags,
            timeout: Some(request.config.execution.timeout),
            offline: request.cargo.offline,
        },
        watch,
    )?;
    report.accounting.soundness.executed = done.executed;
    report.findings.extend(done.findings);
    report.limitations.extend(done.limitations);

    let checked = super::sanitize::sanitize(
        &super::sanitize::Sanitizing {
            root: &request.root,
            cargo: toolchain.cargo(),
            host: toolchain.host(),
            env: environment.vars.clone(),
            packages: &report.scope.resolved_packages,
            sanitizers: &request.config.soundness.sanitizers,
            timeout: Some(request.config.execution.timeout),
            offline: request.cargo.offline,
        },
        watch,
    );
    report.findings.extend(checked.findings);
    report.limitations.extend(checked.limitations);
    Ok(())
}

/// What a run reads before it builds anything: the tree, the identity, and the repository.
fn opened(
    report: &mut Report,
    request: &Request,
    environment: &Environment,
    within: (&Scratch, Watch<'_>),
) {
    let (scratch, watch) = within;
    open_phase(
        report,
        &Opening {
            request,
            environment,
            scratch,
        },
        watch,
    );
    if !report.repository.git.available {
        report.limitations.push(Limitation::new(
            git::UNAVAILABLE_LIMITATION,
            "git could not be asked, so the run cannot name the commit it verified",
        ));
    }
}

/// What a run does once it has measured: drive the fuzz targets, and ask for repairs for what it found.
fn afterwards(
    report: &mut Report,
    within: (&Request, &Environment, &rust_mutants::cargo::Toolchain),
    telling: (&mut Notes<'_>, Watch<'_>),
) {
    let (request, environment, toolchain) = within;
    let (notes, watch) = telling;
    driven(report, request, toolchain, (notes, watch));
    proposed(report, request, environment, (notes, watch));
}

/// Drives the fuzz targets the tree holds, when the configuration asks for it, and keeps what crashed one as a candidate for the corpus.
fn driven(
    report: &mut Report,
    request: &Request,
    toolchain: &rust_mutants::cargo::Toolchain,
    telling: (&mut Notes<'_>, Watch<'_>),
) {
    let (notes, watch) = telling;
    let held = super::fuzz::targets_of(&request.root);
    if held.is_empty() {
        return;
    }
    if !request.config.fuzz.run {
        report.limitations.push(super::fuzz::found(&held));
        return;
    }
    notes.phase("fuzz");
    watch.trace.stage("fuzz");
    let done = super::fuzz::fuzz(
        &super::fuzz::Fuzzing {
            root: &request.root,
            cargo: toolchain.cargo(),
            env: environment_of(toolchain),
            targets: &request.config.fuzz.targets,
            max_total_time: request.config.fuzz.max_total_time,
            timeout: Some(
                request
                    .config
                    .fuzz
                    .max_total_time
                    .saturating_add(request.config.execution.timeout),
            ),
        },
        watch,
    );
    report.findings.extend(done.findings);
    report.limitations.extend(done.limitations);
    for crash in &done.crashes {
        kept(report, request, crash);
    }
}

/// The environment the fuzzer runs with: the toolchain's own, since a fuzz build is a build.
fn environment_of(
    toolchain: &rust_mutants::cargo::Toolchain,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    toolchain
        .env()
        .map(<[(std::ffi::OsString, std::ffi::OsString)]>::to_vec)
        .unwrap_or_default()
}

/// Keeps one crashing input as a candidate for the corpus.
///
/// It is not written into the tree: an input a fuzzer found is a proposal
/// like any other, and `fix --apply` is what puts it where a later run will
/// read it.
fn kept(report: &mut Report, request: &Request, crash: &super::fuzz::Crash) {
    let proposal = crate::repair::Proposal {
        kind: crate::repair::Kind::Corpus,
        path: crash.corpus.clone(),
        preimage: crate::repair::preimage_of(&request.root, &crash.corpus),
        digest: hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&crash.content)),
        content: crash.content.clone(),
    };
    if crate::repair::keep(&request.root, &proposal).is_err() {
        report.limitations.push(Limitation::new(
            super::fuzz::UNAVAILABLE_LIMITATION,
            &format!(
                "the input that crashed {} could not be kept, so it cannot be promoted",
                crash.target
            ),
        ));
        return;
    }
    report.candidates.push(crate::report::CandidateRecord {
        finding: format!("fuzz:{}", crash.target),
        mutant: String::new(),
        kind: crate::repair::Kind::Corpus.name().to_owned(),
        path: proposal.path,
        digest: proposal.digest,
        preimage: proposal.preimage,
        stability_runs: 0,
        kill_runs: 1,
        accepted: true,
        why: None,
    });
}

/// The limitation a run states when a generation provider could not be asked.
pub const GENERATION_LIMITATION: &str = "generation-provider-unavailable";

/// Asks the generation provider, when the configuration names one.
fn proposed(
    report: &mut Report,
    request: &Request,
    environment: &Environment,
    telling: (&mut Notes<'_>, Watch<'_>),
) {
    let (notes, watch) = telling;
    if request.config.generation.is_none() {
        return;
    }
    notes.phase("generation");
    watch.trace.stage("generation");
    propose(report, request, environment, watch);
}

/// Asks the generation provider to close what the run found, and puts every candidate to the tests before keeping it.
///
/// Nothing here fails a run. A provider that cannot be asked leaves a
/// limitation; a candidate that does not work is kept as a record of what was
/// tried, marked as one nobody may apply.
fn propose(report: &mut Report, request: &Request, environment: &Environment, watch: Watch<'_>) {
    let Some(generation) = &request.config.generation else {
        return;
    };
    let allowed = crate::repair::allowed(&generation.allowed_paths);
    let subjects: Vec<(String, String)> = report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::SurvivingMutant)
        .map(|finding| (finding.subject.clone(), finding.detail.clone()))
        .collect();
    for (mutant, detail) in subjects {
        if watch.cancel.is_cancelled() {
            break;
        }
        let asking = ask(report, request, (&mutant, &detail), &allowed);
        let Ok(asked) = serde_json::to_string(&asking) else {
            continue;
        };
        let seen = crate::resource::visible(&environment.vars, &generation.environment);
        let said = crate::provider::once(&crate::provider::Once {
            command: &generation.command,
            dir: &request.root,
            env: &seen,
            question: &asked,
            timeout: request.config.execution.timeout,
            limit: crate::repair::OUTPUT_LIMIT,
        });
        let said = match said {
            Ok(said) => said,
            Err(refusal) => {
                report.limitations.push(Limitation::new(
                    GENERATION_LIMITATION,
                    &format!("the generation provider could not be asked: {refusal}"),
                ));
                return;
            }
        };
        match crate::repair::take(&said, &request.root, &allowed) {
            Ok(proposals) => {
                for proposal in &proposals {
                    considered(report, (request, environment, watch), (proposal, &mutant));
                }
            }
            Err(refusal) => report.limitations.push(Limitation::new(
                GENERATION_LIMITATION,
                &format!("a candidate for {mutant} was not read: {refusal}"),
            )),
        }
    }
}

/// What the provider is told about one finding.
fn ask(
    report: &Report,
    request: &Request,
    about: (&str, &str),
    allowed: &[String],
) -> crate::repair::Ask {
    let (mutant, detail) = about;
    let found = report.mutants.iter().find(|one| one.id == mutant);
    crate::repair::Ask {
        version: crate::repair::VERSION,
        finding: crate::repair::AskedFinding {
            id: mutant.to_owned(),
            kind: "surviving-mutant".to_owned(),
            path: found.map(|one| one.path.clone()).unwrap_or_default(),
            line: found.map_or(0, |one| one.position.line),
            summary: detail.to_owned(),
            replay: format!("mjutest replay {mutant}"),
            mutant: found.map(|one| one.rule.clone()).unwrap_or_default(),
            mutant_id: mutant.to_owned(),
        },
        allowed_paths: allowed.to_vec(),
        workspace: crate::repair::AskedWorkspace {
            workspace_digest: report.repository.workspace_digest.clone(),
            run_id: request.run_id.clone(),
        },
    }
}

/// Puts one candidate to the tests and records what that established.
fn considered(
    report: &mut Report,
    within: (&Request, &Environment, Watch<'_>),
    about: (&crate::repair::Proposal, &str),
) {
    let (request, environment, watch) = within;
    let (proposal, mutant) = about;
    let verdict = super::repair::check(
        &super::repair::Checking {
            root: &request.root,
            environment,
            cargo: request.cargo,
            timeout: request.config.execution.timeout,
        },
        proposal,
        mutant,
        watch,
    );
    let verdict = match verdict {
        Ok(verdict) => verdict,
        Err(refusal) => super::repair::Verdict::refused(
            0,
            0,
            format!("the candidate could not be put to the tests: {refusal}"),
        ),
    };
    if verdict.accepted && crate::repair::keep(&request.root, proposal).is_err() {
        report.limitations.push(Limitation::new(
            GENERATION_LIMITATION,
            &format!("a candidate for {mutant} could not be kept, so it cannot be applied"),
        ));
        return;
    }
    report.candidates.push(crate::report::CandidateRecord {
        finding: mutant.to_owned(),
        mutant: mutant.to_owned(),
        kind: proposal.kind.name().to_owned(),
        path: proposal.path.clone(),
        digest: proposal.digest.clone(),
        preimage: proposal.preimage.clone(),
        stability_runs: verdict.stable,
        kill_runs: verdict.killed,
        accepted: verdict.accepted,
        why: verdict.why,
    });
}

/// The limitation a run states when a resource it held would not stop.
pub const RESOURCE_UNSTOPPED_LIMITATION: &str = "resource-not-stopped";

/// Stops everything the run held, and says what would not stop.
fn released(resources: &mut crate::resource::Manager, report: &mut Report) {
    for refusal in resources.release() {
        report.limitations.push(Limitation::new(
            RESOURCE_UNSTOPPED_LIMITATION,
            &format!("a resource would not stop: {refusal}"),
        ));
    }
}

/// The resources the configuration declares, started and held for the rest of the run.
///
/// # Errors
/// Whatever stopped one from starting.
fn holding(
    request: &Request,
    environment: &Environment,
    report: &mut Report,
    telling: (&mut Notes<'_>, Watch<'_>),
) -> Result<crate::resource::Manager, RunnerError> {
    let (notes, watch) = telling;
    let mut resources = crate::resource::Manager::new(crate::resource::Where {
        dir: request.root.clone(),
        env: environment.vars.clone(),
    });
    if !request.config.resources.is_empty() {
        notes.phase("resources");
        watch.trace.stage("resources");
        hold(&mut resources, request, report, watch)?;
    }
    Ok(resources)
}

/// Starts every resource the configuration declares, in name order, and records what the run holds.
///
/// A resource that will not start ends the run: the tests would pass in a
/// world the configuration does not describe, and the report would say so
/// without knowing it.
fn hold(
    resources: &mut crate::resource::Manager,
    request: &Request,
    report: &mut Report,
    watch: Watch<'_>,
) -> Result<(), RunnerError> {
    for (capability, resource) in &request.config.resources {
        if watch.cancel.is_cancelled() {
            break;
        }
        let started = resources.start(capability, resource);
        let lease = match started {
            Ok(lease) => lease,
            Err(refusal) => {
                let _stopped = resources.release();
                return Err(refusal.into());
            }
        };
        report.resources.push(crate::report::ResourceRecord {
            capability: lease.capability.clone(),
            instance: lease.instance.clone(),
            environment: lease
                .environment
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        });
    }
    Ok(())
}

/// The environment every later phase runs with: this run's own, and what the resources it holds told it.
fn with_resources(environment: &Environment, resources: &crate::resource::Manager) -> Environment {
    let mut held = environment.clone();
    for (name, value) in resources.environment() {
        let name = std::ffi::OsString::from(name);
        held.vars.retain(|(other, _)| *other != name);
        held.vars.push((name, std::ffi::OsString::from(value)));
    }
    held.vars.sort();
    held
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
/// Whether a resource only one test may hold at a time forces this run to measure one thing at a time.
fn alone(config: &Config) -> bool {
    config.resources.values().any(|resource| resource.exclusive)
}

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
        jobs: request.config.execution.jobs,
        exclusive: alone(&request.config),
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
    watch.trace.stage("mutation");
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
            jobs: mutating.request.config.execution.jobs,
            exclusive: alone(&mutating.request.config),
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

/// The files a change set names, as the patterns the engine mutates within. A run that is not about a change set restricts nothing.
fn within(change: Option<&git::Change>) -> Vec<rust_mutants::glob::Pattern> {
    change.map_or_else(Vec::new, |change| rust_mutants::git::within(change, &[]))
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
