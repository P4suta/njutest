// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One verification, from a request to a report.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use jiff::Timestamp;
use rust_mutants::id::RunId;

use crate::assure::baseline;
use crate::assure::equivalence;
use crate::assure::model;
use crate::assure::mutation::{self, MutationOptions, Subject};
use crate::build::Cargo;
use crate::cli::Environment;
use crate::config::{Acceptance, Config};
use crate::error::RunnerError;
use crate::git;
use crate::report::{
    BuildReport, Finding, FindingKind, Limitation, MutantRecord, RunKind, SoundnessAccounting,
    TargetRecord, TargetStatus, Toolchain, UNAVAILABLE,
};
use crate::scratch::Scratch;
use crate::soundness;
use crate::ui::Notes;
use crate::watch::Watch;
use rust_mutants::cargo::Metadata;

/// An exact run fact that could not be represented without changing it.
#[derive(Debug, thiserror::Error)]
pub enum RunInvariantError {
    /// A control execution answered under a target other than the requested target.
    #[error("the control for target {requested:?} returned evidence for {observed:?}")]
    ControlTargetMismatch {
        /// The target the assurance phase selected.
        requested: String,
        /// The target named by the engine result.
        observed: String,
    },
    /// Combining two configured time allowances exceeded `Duration`.
    #[error("the {phase} timeout exceeds std::time::Duration")]
    TimeoutOverflow {
        /// The phase whose bound could not be formed.
        phase: &'static str,
    },
    /// A checkpoint's attempt counter reached the end of its wire range.
    #[error("the checkpoint attempt counter exceeds u32")]
    AttemptOverflow,
    /// The measured wall duration cannot be carried by the report wire.
    #[error("the run duration {milliseconds}ms is outside the report's u64 range")]
    ElapsedOutsideWire {
        /// The exact signed duration returned by the clock.
        milliseconds: i128,
    },
    /// A probe infected more mutations than the trace wire can count exactly.
    #[error("the probe infection count {count} is outside the trace's u64 range")]
    ProbeCountOutsideWire {
        /// The exact in-memory count.
        count: usize,
    },
    /// A mutation execution lasted longer than the trace wire can represent.
    #[error("the mutation execution duration {milliseconds}ms is outside the trace's u64 range")]
    MutationDurationOutsideWire {
        /// The exact measured duration.
        milliseconds: u128,
    },
    /// The aggregation ledger lost the row it had just recorded for a target.
    #[error("the mutation answer ledger lost the recorded row for target {target}")]
    MissingMutationAnswer {
        /// The target whose answer should have been present.
        target: String,
    },
}

/// What one run was asked to do.
#[derive(Debug, Clone)]
pub struct Request {
    /// The workspace root.
    pub root: PathBuf,
    /// The effective configuration.
    pub config: Config,
    /// What cargo is told to build, which is what decides which program the run measures.
    pub build: rust_mutants::cargo::BuildConfig,
    /// Packages the command line asked for, which narrow the configuration's.
    pub packages: Vec<String>,
    /// Arguments for the test binaries, after `--`.
    pub test_args: Vec<String>,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// Preserve the directories the run worked in.
    pub keep_temp: bool,
    /// The run's identity.
    pub run_id: RunId,
    /// When it started.
    pub started: Timestamp,
    /// Where the engine records its own stream.
    pub engine_trace: rust_mutants::trace::Recorder,
    /// What this run is, as numbers.
    /// Empty when the tree could not be read, which states a limitation rather than failing the run.
    pub evidence: crate::assure::identity::Evidence,
    /// The configuration file the run read, as a reader would name it.
    /// Empty when there was none.
    pub configuration: String,
    /// The change set to mutate within, when the run was asked for one.
    pub changed: Option<git::Change>,
    /// Where scheduling state for an interrupted run is kept.
    /// `None` keeps none, which is what a run told to establish everything afresh does.
    pub checkpoints: Option<PathBuf>,
    /// Where what earlier runs established about individual mutants is kept.
    /// `None` establishes everything afresh.
    pub evidence_store: Option<PathBuf>,
    /// Which part of the catalog this run judges.
    /// `None` judges every one of them.
    pub shard: Option<rust_mutants::run::Shard>,
}

/// What one run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The report, audited and ready to persist.
    pub report: BuildReport,
    /// What the run preserved, for a person to look at.
    pub kept: Vec<PathBuf>,
    /// Immutable survivor inputs for the one post-lattice model phase.
    pub(crate) model: model::Preparation,
}

/// Runs one verification.
///
/// # Errors
/// Whatever stopped a phase from observing anything: no scratch directory,
/// no toolchain, a build that could not be started, a coverage tool that failed.
/// A workspace that does not compile and a test that fails are not errors — they are findings in the report.
pub fn run(
    request: &Request,
    environment: &Environment,
    notes: &mut Notes<'_>,
    watch: Watch<'_>,
) -> Result<Outcome, RunnerError> {
    let mut report = identity(request)?;
    notes.phase("open")?;
    watch.trace.stage("open");
    let scratch = Scratch::create(
        &environment.temp_directory,
        &request.run_id,
        request.started,
    )?;
    opened(&mut report, request, environment, (&scratch, watch))?;

    let (toolchain, metadata) = locate(request, environment, watch)?;
    let narrowing = surveyed(&mut report, request, (&toolchain, &metadata))?;
    let mut model = model::Preparation::for_contract(request.config.contract, toolchain.host());

    notes.phase("soundness")?;
    watch.trace.stage("soundness");
    let unsafe_packages = take_inventory(&mut report, request, &metadata, &narrowing)?;
    deepened(&mut report, request, (&toolchain, environment), watch)?;

    let mut resources = holding(request, environment, &mut report, (notes, watch))?;
    let seams = super::wire::watched(&resources.leases(), &request.config.resources);
    state_unwatched(&mut report, &seams);
    let mut held = with_seams(environment, &seams);
    if request.config.contract == crate::config::Contract::VerifiedV1 {
        set_environment(&mut held, "CARGO_BUILD_TARGET", toolchain.host());
    }
    let environment = &held;

    notes.phase("baseline")?;
    watch.trace.stage("baseline");
    let restore = resume_state(request, &mut report)?;
    let mut journal = Journal::of(request, restore.as_ref())?;
    let prepared = prepared(request, environment, &mut report, watch)?;
    let mut asked = false;
    if let Some(session) = prepared {
        let baseline = baseline::observe(&session, baseline::Reporting { notes, watch })?;
        absorb(&mut report, &baseline)?;
        if measurable(&baseline) {
            run_mutation(
                &mut Mutating {
                    unsafe_packages: &unsafe_packages,
                    report: &mut report,
                    request,
                    environment,
                    baseline: &baseline,
                    metadata: &metadata,
                    restore: restore.as_ref(),
                    journal: &mut journal,
                    session: &session,
                    scratch: &scratch,
                },
                &mut model,
                notes,
                watch,
            )?;
        }
        asked = wired(&mut report, &seams, &session, (notes, watch))?;
        for path in session.close()? {
            notes.note("kept", &path.display().to_string())?;
        }
    }
    if !asked {
        licensed(&mut report, seams)?;
    }
    afterwards(
        &mut report,
        (request, environment, &toolchain),
        (notes, watch),
    )?;
    released(&mut resources, &mut report);
    finish(&mut report, request.started)?;
    journal.finished(watch.cancel)?;
    let kept = if request.keep_temp {
        scratch.keep()?
    } else {
        scratch.close()?
    };
    notes.finish()?;
    Ok(Outcome {
        report,
        kept,
        model,
    })
}

fn prepared(
    request: &Request,
    environment: &Environment,
    report: &mut BuildReport,
    watch: Watch<'_>,
) -> Result<Option<rust_mutants::session::Session>, RunnerError> {
    match prepare(request, environment, watch) {
        Ok(session) => Ok(Some(session)),
        Err(error) => {
            let Some(refused) = baseline::refused(&error) else {
                return Err(error);
            };
            absorb(report, &refused)?;
            Ok(None)
        }
    }
}

/// Runs the suite one target at a time, telling the seams which one is running.
///
/// The catalogue is derived from what this sees, and an exchange that says which target caused it is what lets a later run told to measure only what changed skip a question whose target did not.
/// Running the targets one at a time is the same work in a different order — `control` with no target named already loops them — so the attribution costs the loop and nothing else.
fn attributed(
    seams: &super::wire::Seams,
    session: &rust_mutants::session::Session,
    timeout: std::time::Duration,
    watch: Watch<'_>,
) -> Result<Vec<crate::wire::settle::Answered>, RunnerError> {
    let mut answered = Vec::new();
    for target in session.targets() {
        seams.during(Some(target.id.as_str()));
        let asked = rust_mutants::session::Request::new(String::new())
            .with_target(target.id.as_str())
            .with_timeout(Some(timeout));
        let ran = session
            .control(
                &asked,
                watch.cancel,
                rust_mutants::session::Observing::Nothing,
            )?
            .result;
        if ran.target != target.id {
            return Err(RunInvariantError::ControlTargetMismatch {
                requested: target.id.clone(),
                observed: ran.target,
            }
            .into());
        }
        answered.push(crate::wire::settle::Answered {
            passed: ran.outcome() == rust_mutants::outcome::Outcome::Survived,
            target: ran.target,
        });
    }
    seams.during(None);
    Ok(answered)
}

/// Puts every question the seams recorded back to the suite, and says whether it put any.
///
/// The suite is run again with one fault in place and nothing mutated, which is what `control` is, so what a failure says is that a test noticed the seam answering differently rather than that the code changed.
fn wired(
    report: &mut BuildReport,
    seams: &super::wire::Seams,
    session: &rust_mutants::session::Session,
    (notes, watch): (&mut Notes<'_>, Watch<'_>),
) -> Result<bool, RunnerError> {
    if seams.watching.is_empty() {
        return Ok(false);
    }
    notes.phase("wire")?;
    watch.trace.stage("wire");
    let held_up = seams
        .watching
        .iter()
        .map(|one| one.held_up)
        .max()
        .unwrap_or_default();
    let timeout = session
        .slowest_baseline()
        .checked_mul(2)
        .and_then(|doubled| doubled.checked_add(held_up))
        .ok_or(RunInvariantError::TimeoutOverflow { phase: "wire" })?;
    let mut answer_error = None;
    let mut once = || {
        let asked = rust_mutants::session::Request::new(String::new()).with_timeout(Some(timeout));
        match session.control_observing(&asked, watch.cancel) {
            Ok(ran) => match ran.outcome() {
                rust_mutants::outcome::Outcome::Survived
                | rust_mutants::outcome::Outcome::Killed => {
                    crate::wire::settle::Asked::Answered(vec![crate::wire::settle::Answered {
                        passed: ran.outcome() == rust_mutants::outcome::Outcome::Survived,
                        target: ran.target,
                    }])
                }
                outcome @ (rust_mutants::outcome::Outcome::NotRun
                | rust_mutants::outcome::Outcome::StepLimitReached
                | rust_mutants::outcome::Outcome::Waited
                | rust_mutants::outcome::Outcome::Inconclusive
                | rust_mutants::outcome::Outcome::Errored) => {
                    crate::wire::settle::Asked::NotMeasured(outcome)
                }
            },
            Err(error) => {
                if answer_error.is_none() {
                    answer_error = Some(RunnerError::from(error));
                }
                crate::wire::settle::Asked::NotMeasured(rust_mutants::outcome::Outcome::Errored)
            }
        }
    };
    let mut observation_error = None;
    let went_past = seams.observing(|| match attributed(seams, session, timeout, watch) {
        Ok(answered) => answered,
        Err(error) => {
            observation_error = Some(error);
            Vec::new()
        }
    });
    if let Some(error) = observation_error {
        return Err(error);
    }
    let measured = super::wire::asking(seams, &went_past, &mut once, watch);
    if let Some(error) = answer_error {
        return Err(error);
    }
    let measured = measured?;
    report.findings.extend(measured.findings);
    report.limitations.extend(measured.limitations);
    report.seams.extend(measured.seams);
    Ok(measured.executed)
}

/// States what the seams the run watched licensed it to ask, where it asked none of it.
fn licensed(
    report: &mut BuildReport,
    seams: super::wire::Seams,
) -> Result<(), crate::wire::derive::DeriveError> {
    if seams.watching.is_empty() {
        return Ok(());
    }
    report
        .limitations
        .extend(super::wire::licensing(&seams.recorded())?);
    Ok(())
}

/// Says, for every seam the configuration named and this run did not watch, which of the ways it could not.
fn state_unwatched(report: &mut BuildReport, seams: &super::wire::Seams) {
    for (capability, why) in &seams.unwatched {
        report.limitations.push(Limitation::new(
            crate::limitation::SEAM_NOT_WATCHED,
            &format!("{capability}: {}", why.why()),
        ));
    }
}

/// What the toolchain and the workspace are, before anything is built.
fn surveyed(
    report: &mut BuildReport,
    request: &Request,
    found: (&rust_mutants::cargo::Toolchain, &Metadata),
) -> Result<Narrowing, RunnerError> {
    let (toolchain, metadata) = found;
    report.toolchain = describe(toolchain);
    report.repository.packages = metadata
        .packages
        .iter()
        .map(|package| package.name.clone())
        .collect();
    if let Some(name) = unknown_package(request, &report.repository.packages) {
        return Err(RunnerError::Engine(
            rust_mutants::discover::DiscoverError::UnknownPackage { name }.into(),
        ));
    }
    let narrowing = resolved(request, &report.repository.packages);
    report.scope.resolved_packages = narrowing.names().to_vec();
    Ok(narrowing)
}

/// What the `deep-v1` contract adds to a run: the suite interpreted under Miri, and run again under every sanitizer the configuration asks for.
///
/// # Errors
/// A toolchain with no Miri, which a contract that promises interpretation cannot answer for.
/// A sanitizer that will not run is a limitation, because it is asked for by configuration rather than promised by the contract.
fn deepened(
    report: &mut BuildReport,
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
            locked: request.cargo.locked,
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
            locked: request.cargo.locked,
        },
        watch,
    )?;
    report.findings.extend(checked.findings);
    report.limitations.extend(checked.limitations);
    Ok(())
}

/// What a run reads before it builds anything: the tree, the identity, and the repository.
/// What the run can say before it has compiled anything, and what it already knows it cannot claim.
///
/// # Errors
/// Returns [`RunnerError`] when the tree, repository identity, or opening trace cannot be established exactly.
pub fn opened(
    report: &mut BuildReport,
    request: &Request,
    environment: &Environment,
    within: (&Scratch, Watch<'_>),
) -> Result<(), RunnerError> {
    let (scratch, watch) = within;
    open_phase(
        report,
        &Opening {
            request,
            environment,
            scratch,
        },
        watch,
    )?;
    if report.repository.git.said().is_none() {
        report.limitations.push(Limitation::new(
            crate::limitation::GIT_METADATA_UNAVAILABLE,
            "git could not be asked, so the run cannot name the commit it verified",
        ));
    }
    Ok(())
}

/// What a run does once it has measured: drive the fuzz targets, and ask for repairs for what it found.
fn afterwards(
    report: &mut BuildReport,
    within: (&Request, &Environment, &rust_mutants::cargo::Toolchain),
    telling: (&mut Notes<'_>, Watch<'_>),
) -> Result<(), RunnerError> {
    let (request, environment, toolchain) = within;
    let (notes, watch) = telling;
    driven(report, request, toolchain, (notes, watch))?;
    proposed(report, request, environment, (notes, watch))?;
    Ok(())
}

/// Drives the fuzz targets the tree holds, when the configuration asks for it, and keeps what crashed one as a candidate for the corpus.
fn driven(
    report: &mut BuildReport,
    request: &Request,
    toolchain: &rust_mutants::cargo::Toolchain,
    telling: (&mut Notes<'_>, Watch<'_>),
) -> Result<(), RunnerError> {
    let (notes, watch) = telling;
    let held = match super::fuzz::targets_of(&request.root) {
        Ok(held) => held,
        Err(error) => {
            report.limitations.push(Limitation::new(
                crate::limitation::CARGO_FUZZ_UNAVAILABLE,
                &format!("fuzz targets could not be read completely: {error}"),
            ));
            report.findings.push(Finding {
                kind: FindingKind::NotMeasured,
                subject: "fuzz:targets".to_owned(),
                detail: format!("filesystem traversal failed: {error}"),
                origin: crate::report::FindingOrigin::Global,
                path: None,
                position: None,
            });
            return Ok(());
        }
    };
    if held.is_empty() {
        return Ok(());
    }
    if !request.config.fuzz.run {
        report.limitations.push(super::fuzz::found(&held));
        return Ok(());
    }
    notes.phase("fuzz")?;
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
                    .checked_add(request.config.execution.timeout)
                    .ok_or(RunInvariantError::TimeoutOverflow { phase: "fuzz" })?,
            ),
        },
        watch,
    )?;
    report.findings.extend(done.findings);
    report.limitations.extend(done.limitations);
    for crash in &done.crashes {
        kept(report, request, crash);
    }
    Ok(())
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
fn kept(report: &mut BuildReport, request: &Request, crash: &super::fuzz::Crash) {
    let proposal = crate::repair::Proposal {
        kind: crate::repair::Kind::Corpus,
        path: crash.corpus.clone(),
        preimage: crate::repair::preimage_of(&request.root, &crash.corpus),
        digest: hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&crash.content)),
        content: crash.content.clone(),
    };
    if crate::repair::keep(&request.root, &proposal).is_err() {
        report.limitations.push(Limitation::new(
            crate::limitation::CARGO_FUZZ_UNAVAILABLE,
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

/// The limitation a run states when a candidate held up and could not be stored.
fn proposed(
    report: &mut BuildReport,
    request: &Request,
    environment: &Environment,
    telling: (&mut Notes<'_>, Watch<'_>),
) -> Result<(), RunnerError> {
    let (notes, watch) = telling;
    if request.config.generation.is_none() {
        return Ok(());
    }
    notes.phase("generation")?;
    watch.trace.stage("generation");
    propose(report, request, environment, watch);
    Ok(())
}

/// Asks the generation provider to close what the run found, and puts every candidate to the tests before keeping it.
fn propose(
    report: &mut BuildReport,
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) {
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
                    crate::limitation::GENERATION_PROVIDER_UNAVAILABLE,
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
                crate::limitation::GENERATION_PROVIDER_UNAVAILABLE,
                &format!("a candidate for {mutant} was not read: {refusal}"),
            )),
        }
    }
}

/// What the provider is told about one finding.
fn ask(
    report: &BuildReport,
    request: &Request,
    about: (&str, &str),
    allowed: &[String],
) -> crate::repair::Ask {
    let (mutant, detail) = about;
    let found = report
        .mutants
        .iter()
        .find(|one| one.display_id == mutant || one.id == mutant);
    crate::repair::Ask {
        version: crate::repair::VERSION,
        finding: crate::repair::AskedFinding {
            id: mutant.to_owned(),
            kind: "surviving-mutant".to_owned(),
            path: found.map(|one| one.path.clone()).unwrap_or_default(),
            line: found.map_or(0, |one| one.position.line),
            summary: detail.to_owned(),
            replay: format!("njutest replay {mutant}"),
            mutant: found.map(|one| one.rule.clone()).unwrap_or_default(),
            mutant_id: mutant.to_owned(),
        },
        allowed_paths: allowed.to_vec(),
        workspace: crate::repair::AskedWorkspace {
            workspace_digest: report.repository.workspace_digest.clone(),
            run_id: request.run_id.to_string(),
        },
    }
}

/// Puts one candidate to the tests and records what that established.
fn considered(
    report: &mut BuildReport,
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
            build: request.build.clone(),
            harness_args: request.test_args.clone(),
            skip_targets: request.config.execution.skip_targets.clone(),
            timeout: request.config.execution.timeout,
            steps: request.config.execution.steps,
            build_timeout: request.config.execution.build_timeout,
            reports: request.config.reports.directory.clone(),
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
            crate::limitation::GENERATION_CANDIDATE_NOT_KEPT,
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

/// Stops everything the run held, and says what would not stop.
fn released(resources: &mut crate::resource::Manager, report: &mut BuildReport) {
    for refusal in resources.release() {
        report.limitations.push(Limitation::new(
            crate::limitation::RESOURCE_NOT_STOPPED,
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
    report: &mut BuildReport,
    telling: (&mut Notes<'_>, Watch<'_>),
) -> Result<crate::resource::Manager, RunnerError> {
    let (notes, watch) = telling;
    let mut resources = crate::resource::Manager::new(crate::resource::Where {
        dir: request.root.clone(),
        env: environment.vars.clone(),
    });
    if !request.config.resources.is_empty() {
        notes.phase("resources")?;
        watch.trace.stage("resources");
        hold(&mut resources, request, report, watch)?;
    }
    Ok(resources)
}

/// Starts every resource the configuration declares, in name order, and records what the run holds.
fn hold(
    resources: &mut crate::resource::Manager,
    request: &Request,
    report: &mut BuildReport,
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
                let cleanup = resources.release();
                return if cleanup.is_empty() {
                    Err(refusal.into())
                } else {
                    Err(crate::resource::ResourceError::ReleaseAfterFailure {
                        primary: Box::new(refusal),
                        cleanup,
                    }
                    .into())
                };
            }
        };
        report.resources.push(crate::report::ResourceRecord {
            capability: lease.capability.clone(),
            instance: lease.instance.to_string(),
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
fn with_seams(environment: &Environment, seams: &super::wire::Seams) -> Environment {
    let mut held = environment.clone();
    held.vars = held
        .vars
        .into_iter()
        .chain(seams.environment.iter().map(|(name, value)| {
            (
                std::ffi::OsString::from(name),
                std::ffi::OsString::from(value),
            )
        }))
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect();
    held
}

fn set_environment(environment: &mut Environment, name: &str, value: &str) {
    environment.vars.retain(|(held, _value)| {
        !held
            .to_str()
            .is_some_and(|held| held.eq_ignore_ascii_case(name))
    });
    environment
        .vars
        .push((name.into(), std::ffi::OsString::from(value)));
}

/// The report as it is before anything has run: what the run is, what it was asked to verify, and what it already knows it will not claim.
///
/// # Errors
/// Returns [`RunnerError`] when a path or configuration digest cannot be represented exactly in the report identity.
pub fn identity(request: &Request) -> Result<BuildReport, RunnerError> {
    let mut report = BuildReport::new(
        request.run_id.as_str(),
        kind_of(request),
        request.config.contract,
    );
    report.timing.started = request.started.to_string();
    report.repository.root_name = root_name(&request.root)?;
    report.repository.configuration_digest = request.config.digest()?;
    if request.evidence.is_known() {
        report
            .repository
            .workspace_digest
            .clone_from(&request.evidence.tree);
        report
            .provenance
            .identity
            .clone_from(&request.evidence.identity);
    }
    report.scope.requested_packages = requested(request);
    report.scope.shard = request.shard.map(|shard| shard.to_string());
    report
        .scope
        .included
        .clone_from(&request.config.project.include);
    report
        .scope
        .excluded
        .clone_from(&request.config.project.exclude);
    report
        .scope
        .configuration
        .clone_from(&request.configuration);
    report.scope.configured_builds =
        std::iter::once(crate::config::DEFAULT_CONFIGURATION.to_owned())
            .chain(
                request
                    .config
                    .configuration
                    .iter()
                    .map(|configuration| configuration.name.clone()),
            )
            .collect();
    if !request.config.execution.skip_targets.is_empty() {
        report.limitations.push(Limitation::new(
            rust_mutants::limitation::TARGET_SKIPPED_BY_CONFIGURATION,
            &format!(
                "{} ({})",
                limitation_detail(rust_mutants::limitation::TARGET_SKIPPED_BY_CONFIGURATION),
                request.config.execution.skip_targets.join(", ")
            ),
        ));
    }
    if !request.evidence.is_known() {
        report.limitations.push(Limitation::new(
            crate::limitation::WORKSPACE_DIGEST_NOT_COMPUTED,
            "the tree could not be read as one number, so no result of this run can be \
             reused by another",
        ));
    }
    Ok(report)
}

fn count(field: &'static str, value: usize) -> Result<u32, crate::report::CountError> {
    u32::try_from(value).map_err(|_outside_wire_range| crate::report::CountError::Width {
        ledger: field,
        count: value,
    })
}

/// What an interrupted run left for this one, or nothing.
/// A state this release cannot continue from is a state it does not continue from: the run starts cold and says so.
fn resume_state(
    request: &Request,
    report: &mut BuildReport,
) -> Result<Option<crate::checkpoint::State>, crate::checkpoint::CheckpointError> {
    let Some(directory) = request.checkpoints.as_ref() else {
        return Ok(None);
    };
    if !request.evidence.is_known() {
        return Ok(None);
    }
    let identity = request.evidence.continuation_identity();
    let Some(state) = crate::checkpoint::read(directory, &identity)? else {
        return Ok(None);
    };
    if state.is_empty() {
        return Ok(None);
    }
    report.limitations.push(Limitation::new(
        crate::limitation::RESUMED_FROM_CHECKPOINT,
        &format!(
            "an interrupted run had already measured {} targets and established {} mutants; \
             a restored target carries the files it reached and not the regions inside them, \
             so it keeps reaching its whole file",
            state.targets.len(),
            state.mutants.len()
        ),
    ));
    Ok(Some(state))
}

/// What this run has established so far, written where an interrupted run's successor will find it.
struct Journal {
    directory: Option<PathBuf>,
    state: crate::checkpoint::State,
}

impl Journal {
    fn of(
        request: &Request,
        restore: Option<&crate::checkpoint::State>,
    ) -> Result<Self, RunInvariantError> {
        let identity = request.evidence.continuation_identity();
        let mut state = restore
            .cloned()
            .unwrap_or_else(|| crate::checkpoint::State::new(&identity));
        state.attempts = state
            .attempts
            .checked_add(1)
            .ok_or(RunInvariantError::AttemptOverflow)?;
        Ok(Self {
            directory: request
                .checkpoints
                .clone()
                .filter(|_directory| request.evidence.is_known()),
            state,
        })
    }

    fn keep_mutant(
        &mut self,
        judged: &mutation::Judged,
    ) -> Result<(), crate::checkpoint::CheckpointError> {
        let disposition = match &judged.disposition {
            mutation::Disposition::Killed { by } => {
                crate::checkpoint::SavedDisposition::Killed { by: by.clone() }
            }
            mutation::Disposition::StepLimitReached { .. }
            | mutation::Disposition::Waited { .. }
            | mutation::Disposition::Rejected { .. }
            | mutation::Disposition::Survived { .. }
            | mutation::Disposition::Unreached
            | mutation::Disposition::Equivalent { .. }
            | mutation::Disposition::Unconfirmed { .. }
            | mutation::Disposition::Errored { .. } => return Ok(()),
        };
        self.state.record_mutant(crate::checkpoint::SavedMutant {
            id: judged.id.clone(),
            disposition,
            duration_ms: 0,
        });
        self.write()
    }

    /// Preserves the current state before the next mutation can begin.
    fn write(&self) -> Result<(), crate::checkpoint::CheckpointError> {
        if let Some(directory) = &self.directory {
            crate::checkpoint::write(directory, &self.state)?;
        }
        Ok(())
    }

    /// Clears what this run established, because a run that reached its end has nothing left to continue.
    fn finished(
        &self,
        cancel: &rust_mutants::runner::Cancel,
    ) -> Result<(), crate::checkpoint::CheckpointError> {
        if cancel.is_cancelled() {
            return Ok(());
        }
        if let Some(directory) = &self.directory {
            crate::checkpoint::clear(directory, &self.state.identity)?;
        }
        Ok(())
    }
}

/// Where a run works and what it is about, before it has compiled anything.
#[derive(Debug)]
pub struct Opening<'a> {
    /// What the run was asked to do.
    pub request: &'a Request,
    /// The process this run is inside.
    pub environment: &'a Environment,
    /// Where the run works.
    pub scratch: &'a Scratch,
}

/// What the run can say before it has compiled anything: where it works, and what the repository was.
fn open_phase(
    report: &mut BuildReport,
    opening: &Opening<'_>,
    watch: Watch<'_>,
) -> Result<(), RunnerError> {
    let Opening {
        request,
        environment,
        scratch,
    } = *opening;
    if !scratch.is_claimed() {
        report.limitations.push(Limitation::new(
            crate::limitation::TEMP_DIRECTORY_UNCLAIMED,
            "the run works in a directory it could not claim, so a sweep may remove it \
             while the run is still using it",
        ));
    }
    let excluded =
        crate::evidence::tree::Excluded::beside(request.config.reports.directory.as_path())?;
    report.repository.git = git::describe(&git::Asked {
        root: &request.root,
        env: &environment.vars,
        excluded: &excluded,
        watch,
    });
    let Some(change) = &request.changed else {
        return Ok(());
    };
    let crate::report::Git::Said(said) = &mut report.repository.git else {
        return Ok(());
    };
    said.against = change
        .merge_base
        .clone()
        .map(|merge_base| crate::report::Against {
            merge_base,
            changed_files: change.files.clone(),
        });
    Ok(())
}

/// Counts every place a selected package steps outside what the compiler guarantees.
fn take_inventory(
    report: &mut BuildReport,
    request: &Request,
    metadata: &Metadata,
    narrowing: &Narrowing,
) -> Result<BTreeSet<String>, crate::report::CountError> {
    let selected = selected(narrowing, metadata);
    let Ok(taken) = soundness::inventory(&request.root, &selected) else {
        report.limitations.push(Limitation::new(
            crate::limitation::SOUNDNESS_SOURCE_UNREADABLE,
            "the tree could not be walked for the places the compiler stops vouching for, so \
             the run makes no claim about them",
        ));
        return Ok(BTreeSet::new());
    };
    report.accounting.soundness = SoundnessAccounting {
        unsafe_items: count("unsafe items", taken.items.len())?,
        packages_with_unsafe: count("packages containing unsafe", taken.packages.len())?,
        executed: false,
    };
    report.limitations.extend(stated(&taken));
    Ok(taken.packages.iter().cloned().collect())
}

/// The packages a run's scope names, each with the directory its manifest is in.
#[must_use]
pub fn selected(narrowing: &Narrowing, metadata: &Metadata) -> Vec<(String, PathBuf)> {
    metadata
        .packages
        .iter()
        .filter(|package| narrowing.holds(&package.name))
        .filter_map(|package| {
            let directory = package.manifest_path.parent()?;
            if directory.as_os_str().is_empty() {
                return None;
            }
            Some((package.name.clone(), directory.to_path_buf()))
        })
        .collect()
}

/// What a report says about an inventory, which is nothing at all when there was nothing to say.
#[must_use]
pub fn stated(taken: &soundness::Inventory) -> Vec<Limitation> {
    let mut stated = Vec::new();
    if !taken.unreadable.is_empty() {
        stated.push(Limitation::new(
            crate::limitation::SOUNDNESS_SOURCE_UNREADABLE,
            &format!(
                "{} files could not be read as Rust this release understands, so what they \
                 hold is not in the inventory: {}",
                taken.unreadable.len(),
                taken.unreadable.join(", ")
            ),
        ));
    }
    if !taken.is_empty() {
        stated.push(Limitation::new(
            crate::limitation::SOUNDNESS_NOT_EXECUTED,
            &format!(
                "{} places in {} packages step outside what the compiler guarantees, and this \
                 contract counts them rather than executing them; `contract = \"deep-v1\"` \
                 interprets them under miri",
                taken.items.len(),
                taken.packages.len()
            ),
        ));
    }
    stated
}

/// Whether there is anything to measure mutations against.
#[must_use]
pub fn measurable(baseline: &baseline::Baseline) -> bool {
    baseline.failure.is_none()
        && !baseline
            .targets
            .iter()
            .any(|measured| measured.status == TargetStatus::Failed)
}

/// How much of the workspace this run looked at.
#[must_use]
pub fn kind_of(request: &Request) -> RunKind {
    if request.changed.is_some() {
        RunKind::Changed
    } else if requested(request).is_empty() {
        RunKind::Full
    } else {
        RunKind::Scoped
    }
}

/// The toolchain that will build the tree, and what it says the workspace holds.
/// Located inside the workspace, so a `rust-toolchain.toml` there is what answers.
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

/// Whether a resource only one test may hold at a time forces the run to measure one target at a time.
#[must_use]
pub fn alone(config: &Config) -> bool {
    config.resources.values().any(|resource| resource.exclusive)
}

/// The verdict, the canonical order, and how long it all took.
fn finish(report: &mut BuildReport, started: Timestamp) -> Result<(), RunInvariantError> {
    report.verdict = report.concluded();
    report.sort_targets();
    let finished = Timestamp::now();
    report.timing.finished = finished.to_string();
    let milliseconds = finished.duration_since(started).as_millis();
    report.timing.duration_ms = u64::try_from(milliseconds)
        .map_err(|_outside_wire_range| RunInvariantError::ElapsedOutsideWire { milliseconds })?;
    Ok(())
}

/// Puts what the baseline observed into the report.
struct Mutating<'a> {
    report: &'a mut BuildReport,
    request: &'a Request,
    environment: &'a Environment,
    baseline: &'a baseline::Baseline,
    metadata: &'a Metadata,
    restore: Option<&'a crate::checkpoint::State>,
    journal: &'a mut Journal,
    /// Every package the soundness inventory found `unsafe` in, which is where "the same instructions" stops meaning "the same behaviour".
    unsafe_packages: &'a BTreeSet<String>,
    /// The prepared workspace: what built the trees, ran every target once, and decides which tests could notice a mutation.
    session: &'a rust_mutants::session::Session,
    /// This run's own directory, which the knobs make the directories they start controls in under.
    scratch: &'a Scratch,
}

/// The acceptance entries that one catalog can honour, and the entries it cannot match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAcceptances {
    /// Full mutant identities, after resolving every accepted prefix.
    pub ids: BTreeSet<String>,
    /// Entries that did not name exactly one mutant in the catalog.
    pub findings: Vec<Finding>,
}

#[derive(Debug, thiserror::Error)]
enum AcceptanceResolutionError {
    #[error(transparent)]
    Prefix(#[from] rust_mutants::catalog::PrefixError),
    #[error(transparent)]
    Locator(#[from] rust_mutants::session::LocateError),
}

/// Resolves every unexpired acceptance against the complete catalog for a session.
#[must_use]
pub fn resolve_acceptances(
    catalog: &rust_mutants::catalog::Catalog,
    locate: &dyn Fn(
        &rust_mutants::session::Locator,
    ) -> Result<String, rust_mutants::session::LocateError>,
    acceptances: &[Acceptance],
    now: Timestamp,
) -> ResolvedAcceptances {
    let mut ids = Vec::new();
    let mut findings = Vec::new();
    for acceptance in acceptances
        .iter()
        .filter(|acceptance| acceptance.holds(now))
    {
        let found = acceptance.locator().map_or_else(
            || {
                catalog
                    .resolve_prefix(&acceptance.id)
                    .map(|mutant| mutant.id.to_string())
                    .map_err(AcceptanceResolutionError::from)
            },
            |locator| locate(&locator).map_err(AcceptanceResolutionError::from),
        );
        match found {
            Ok(id) => ids.push(id),
            Err(error) => findings.push(Finding::new(
                FindingKind::UnmatchedAcceptance,
                &acceptance.named(),
                &format!(
                    "the acceptance names no single mutant in this catalog: {error}; review or remove it"
                ),
            )),
        }
    }
    let unique_ids = ids.into_iter().collect();
    ResolvedAcceptances {
        ids: unique_ids,
        findings,
    }
}

fn run_mutation(
    mutating: &mut Mutating<'_>,
    model: &mut model::Preparation,
    notes: &mut Notes<'_>,
    watch: Watch<'_>,
) -> Result<(), RunnerError> {
    mutating.report.knobs = repeated(
        mutating.request,
        (mutating.session, mutating.baseline, mutating.scratch),
        mutating.environment,
        (notes, watch),
    )?;
    notes.phase("mutation")?;
    watch.trace.stage("mutation");
    let session = mutating.session;
    let accepted = resolve_acceptances(
        session.catalog(),
        &|locator| session.locate(locator).map(|mutant| mutant.id.to_string()),
        &mutating.request.config.acceptance,
        mutating.request.started,
    );
    let mutation = mutation::run_resuming(
        Subject {
            session,
            baseline: &mutating.baseline.targets,
        },
        &MutationOptions {
            test_args: mutating.request.test_args.clone(),
            evidence: evidence_of(mutating)?,
            jobs: mutating.request.config.execution.jobs,
            exclusive: alone(&mutating.request.config),
            shard: mutating.request.shard,
        },
        &mut mutation::Resume {
            state: mutating.restore,
            record: &mut |judged| {
                mutating
                    .journal
                    .keep_mutant(judged)
                    .map_err(RunnerError::from)
            },
        },
        baseline::Reporting { notes, watch },
    )?;
    let tree_written = !session.changes()?.is_empty();
    if tree_written {
        mutating.report.limitations.push(Limitation::new(
            crate::limitation::TREE_WRITTEN_DURING_MEASUREMENT,
            "a test wrote into the tree while it was being measured, so every later \
             mutation was measured against what it wrote",
        ));
    }
    let mut mutation = mutation;
    if mutating.request.config.mutation.equivalence {
        prove_equivalence(
            &Proving {
                mutating,
                session,
                tree_written,
            },
            &mut mutation,
            (notes, watch),
        )?;
    }
    model.capture(session, &mutation.judged, tree_written)?;
    record(mutating.report, &mutation, &accepted.ids)?;
    mutating.report.findings.extend(accepted.findings);
    Ok(())
}

/// What one pass of the equivalence layer is about, as one argument.
struct Proving<'a> {
    mutating: &'a Mutating<'a>,
    session: &'a rust_mutants::session::Session,
    tree_written: bool,
}

/// Asks the compiler, about every mutation nothing noticed, whether it renders it identically to the code it mutates.
fn prove_equivalence(
    proving: &Proving<'_>,
    mutation: &mut mutation::Mutation,
    reporting: (&mut Notes<'_>, Watch<'_>),
) -> Result<(), RunnerError> {
    let Proving {
        mutating,
        session,
        tree_written,
    } = *proving;
    let (notes, watch) = reporting;
    let asked = equivalence::asked(session, &mutation.judged);
    if asked.is_empty() {
        return Ok(());
    }
    notes.phase("equivalence")?;
    watch.trace.stage("equivalence");
    let phase = watch.trace.phase("equivalence-prove");
    let request = mutating.request;
    let decided = equivalence::prove(
        &equivalence::Proving {
            root: &request.root,
            open: rust_mutants::workspace::OpenOptions {
                allow_outside: Vec::new(),
                cargo: None,
                search_path: mutating
                    .environment
                    .var("PATH")
                    .map(std::ffi::OsStr::to_owned),
                env: mutating.environment.vars.clone(),
                temp_directory: mutating.environment.temp_directory.clone(),
                report_directory: Some(request.config.reports.directory.as_str().to_owned()),
                exclude: Vec::new(),
                keep_temp: false,
                offline: request.cargo.offline,
                locked: request.cargo.locked,
                trace: rust_mutants::trace::Recorder::disabled(),
            },
            build: request.build.clone(),
            timeout: Some(request.config.execution.timeout),
            unsafe_packages: mutating.unsafe_packages.clone(),
            tree_written,
        },
        &asked,
        watch.cancel,
        &rust_mutants::trace::Recorder::disabled(),
    )?;
    equivalence::settle(&mut mutation.judged, &decided, watch)?;
    phase.end();
    Ok(())
}

/// What each knob the configuration asks for establishes about each target whose baseline passed, or nothing where it asks for none.
fn repeated(
    request: &Request,
    (session, baseline, scratch): (
        &rust_mutants::session::Session,
        &baseline::Baseline,
        &Scratch,
    ),
    environment: &Environment,
    (notes, watch): (&mut Notes<'_>, Watch<'_>),
) -> Result<Vec<crate::report::knobs::KnobRecord>, RunnerError> {
    let asked = &request.config.repeatable.knobs;
    if asked.is_empty() {
        return Ok(Vec::new());
    }
    notes.phase("repeatable")?;
    watch.trace.stage("repeatable");
    let place = super::knobs::Place::probed(scratch.dir(), &environment.vars, watch.cancel)?;
    super::knobs::measured(
        session,
        asked,
        (&super::knobs::passing(baseline), &place),
        watch,
    )
}

/// The prepared workspace: the trees, the one run of every target with nothing active, and what its guards recorded.
fn prepare(
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<rust_mutants::session::Session, RunnerError> {
    let include = narrowing(request).map_err(rust_mutants::EngineError::from)?;
    let workspace = rust_mutants::workspace::Workspace::open(
        &request.root,
        rust_mutants::workspace::OpenOptions {
            allow_outside: Vec::new(),
            cargo: None,
            search_path: environment.var("PATH").map(std::ffi::OsStr::to_owned),
            env: environment.vars.clone(),
            temp_directory: environment.temp_directory.clone(),
            report_directory: Some(request.config.reports.directory.as_str().to_owned()),
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
            include,
            exclude: request.config.project.excluded(),
            build: request.build.clone(),
            harness_args: request.test_args.clone(),
            verify: true,
            failing: rust_mutants::session::Failing::Exclude,
            build_timeout: request.config.execution.build_timeout,
            mutant_timeout: rust_mutants::session::Timeout::Fixed(request.config.execution.timeout),
            mutant_steps: (request.config.execution.steps > 0)
                .then_some(request.config.execution.steps),
            skip_targets: request.config.execution.skip_targets.clone(),
            coverage: request.config.execution.coverage,
            ..crate::assure::engine::switches()
        },
        watch.cancel,
    )?)
}

/// Where this run reads and writes what is established about individual mutants, or nothing when it may not.
fn evidence_of(mutating: &Mutating<'_>) -> Result<Option<mutation::Evidence>, RunnerError> {
    let request = mutating.request;
    let Some(root) = request.evidence_store.as_ref() else {
        return Ok(None);
    };
    if !reusable(mutating.report.run_kind, &request.config.resources) {
        return Ok(None);
    }
    let Some(keying) = request.evidence.keying.as_ref() else {
        return Ok(None);
    };
    let mut standing = crate::evidence::store::Standing::default();
    let mut names = BTreeMap::new();
    for measured in &mutating.baseline.targets {
        if measured.status != TargetStatus::Passed {
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
        )?;
        standing.passing.insert(
            measured.target.id.to_string(),
            crate::evidence::key::behaviour(&linked, &keying.common),
        );
        names.insert(measured.target.id.to_string(), measured.target.name());
    }
    Ok(Some(mutation::Evidence {
        root: root.clone(),
        run_id: request.run_id.to_string(),
        standing,
        names,
    }))
}

/// Whether a run of this shape may read and write what earlier runs established about individual mutants.
#[must_use]
pub fn reusable(run_kind: RunKind, resources: &BTreeMap<String, crate::config::Resource>) -> bool {
    run_kind == RunKind::Full && resources.is_empty()
}

/// The package id cargo gave the package called `name`.
fn package_id(metadata: &Metadata, name: &str) -> Option<String> {
    metadata
        .packages
        .iter()
        .find(|package| package.name == name)
        .map(|package| package.id.clone())
}

/// Which files anything may be mutated in: what the configuration allows, narrowed to what changed.
fn narrowing(
    request: &Request,
) -> Result<Vec<rust_mutants::glob::Pattern>, rust_mutants::glob::GlobError> {
    let configured = request.config.project.included();
    let Some(change) = request.changed.as_ref() else {
        return Ok(configured);
    };
    rust_mutants::git::within(change, &configured)
}

/// Puts what the mutation phase judged into the report: the counts, one row per mutation, the findings, and what was not mutated.
///
/// # Errors
/// Returns [`crate::report::CountError`] when exact report counters cannot represent the mutation ledger.
pub fn record(
    report: &mut BuildReport,
    mutation: &mutation::Mutation,
    accepted: &BTreeSet<String>,
) -> Result<(), crate::report::CountError> {
    report.accounting.mutants = mutation.accounting(accepted)?;
    report.mutants = mutation
        .judged
        .iter()
        .map(|judged| MutantRecord {
            catalog_index: crate::report::CatalogIndex::new(judged.catalog_index),
            id: judged.id.clone(),
            display_id: judged.display_id.clone(),
            path: judged.path.clone(),
            position: judged.position.unwrap_or(crate::report::Position {
                line: 1,
                column: 1,
                character_column: 1,
            }),
            rule: judged.rule.clone(),
            item: judged.item.clone(),
            original: judged.original.clone(),
            replacement: judged.replacement.clone(),
            outcome: judged.disposition.decided(),
            accepted: mutation::answered_by(judged, accepted),
            reuse: crate::report::Reuse(judged.source_run_id.clone().map_or(
                crate::report::Established::Here,
                crate::report::Established::ReadBackFrom,
            )),
            blind_in: Vec::new(),
            routing: judged.routing.clone(),
        })
        .collect();
    report.findings.extend(mutation.findings(accepted));
    report.drift.clone_from(&mutation.drift);
    report.sources.clone_from(&mutation.sources);
    if report.scope.shard.is_none() {
        report
            .findings
            .extend(crate::report::hollow::found(&report.mutants));
        report
            .findings
            .extend(crate::report::drift::found(&report.drift, &report.mutants));
        report
            .limitations
            .extend(crate::report::drift::unmeasured(&report.drift));
        report
            .findings
            .extend(crate::report::knobs::found(&report.knobs, &report.mutants));
        report
            .limitations
            .extend(crate::report::knobs::limited(&report.knobs));
    }
    for (reason, count) in &mutation.skips {
        report.limitations.push(Limitation::new(
            &format!("skipped-{reason}"),
            &format!(
                "{count} {} not mutated: {reason}",
                if *count == 1 {
                    "place was"
                } else {
                    "places were"
                }
            ),
        ));
    }
    Ok(())
}

/// Each limitation the baseline stated, once, beside the targets it was stated about.
#[must_use]
pub fn about(limitations: &[String]) -> Vec<(String, Vec<String>)> {
    let mut named: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for limitation in limitations {
        let (name, target) = limitation
            .split_once(':')
            .map_or((limitation.as_str(), None), |(head, tail)| {
                (head, Some(tail))
            });
        let targets = named.entry(name.to_owned()).or_default();
        if let Some(target) = target {
            targets.push(target.to_owned());
        }
    }
    named.into_iter().collect()
}

/// Puts what the baseline observed into the report.
///
/// # Errors
/// Returns [`crate::report::CountError`] when exact report counters cannot represent the baseline ledger.
pub fn absorb(
    report: &mut BuildReport,
    baseline: &baseline::Baseline,
) -> Result<(), crate::report::CountError> {
    for (name, targets) in about(&baseline.limitations) {
        let detail = limitation_detail(&name);
        let detail = if targets.is_empty() {
            detail
        } else {
            format!("{detail} ({})", targets.join(", "))
        };
        report.limitations.push(Limitation::new(&name, &detail));
    }
    if let Some(failure) = &baseline.failure {
        report.findings.push(Finding::new(
            FindingKind::BuildFailure,
            &report.repository.root_name,
            &first_line(failure),
        ));
        return Ok(());
    }
    for measured in &baseline.targets {
        let subject = measured.target.name();
        if let Some(kind) = measured.finding {
            report.findings.push(Finding::new(
                kind,
                &subject,
                measured
                    .message
                    .as_deref()
                    .unwrap_or("the target ended without saying why"),
            ));
        }
        report.targets.push(TargetRecord {
            id: measured.target.id.to_string(),
            name: subject,
            package: measured.target.package.clone(),
            status: measured.status,
            duration_ms: measured.duration_ms,
            message: measured.message.clone(),
        });
    }

    report.count_targets()?;
    Ok(())
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
#[must_use]
pub fn requested(request: &Request) -> Vec<String> {
    asked_for(&request.packages, &request.config)
}

/// The packages a run is about: the ones a reader named, or the ones the configuration names when they named none.
#[must_use]
pub fn asked_for(named: &[String], config: &Config) -> Vec<String> {
    if named.is_empty() {
        config.project.packages.clone()
    } else {
        named.to_vec()
    }
}

/// How much of a workspace one run is about.
///
/// An empty list of names meant both `nothing narrowed this run` and `what narrowed it is not here`, and every reader of it answered the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Narrowing {
    /// Every member, because nothing named any.
    Whole,
    /// Exactly these members, each one the workspace holds.
    Named(Vec<String>),
}

impl Narrowing {
    /// The members this names, which is none when it names the workspace.
    #[must_use]
    pub fn names(&self) -> &[String] {
        match self {
            Self::Whole => &[],
            Self::Named(named) => named,
        }
    }

    /// Whether `name` is one of the packages the run is about.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        match self {
            Self::Whole => true,
            Self::Named(named) => named.iter().any(|held| held == name),
        }
    }
}

/// The packages the run settled on: what was asked for, or every member.
#[must_use]
pub fn resolved(request: &Request, members: &[String]) -> Narrowing {
    let asked = requested(request);
    if asked.is_empty() {
        Narrowing::Whole
    } else {
        Narrowing::Named(
            asked
                .into_iter()
                .filter(|name| members.contains(name))
                .collect(),
        )
    }
}

/// The first package a run was narrowed to that the workspace does not hold, if one is.
#[must_use]
pub fn unknown_package(request: &Request, members: &[String]) -> Option<String> {
    requested(request)
        .into_iter()
        .find(|name| !members.contains(name))
}

/// The name a person calls the workspace.
fn root_name(root: &std::path::Path) -> Result<String, crate::evidence::tree::ScanError> {
    let Some(name) = root.file_name() else {
        return Ok(UNAVAILABLE.to_owned());
    };
    name.to_str()
        .map(str::to_owned)
        .ok_or_else(|| crate::evidence::tree::ScanError::PathNotUtf8 {
            path: root.to_path_buf(),
        })
}

/// One sentence of a compiler's several, and something to say when it said nothing.
#[must_use]
pub fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("the workspace does not compile")
        .to_owned()
}

/// What a named limitation means, for the ones a phase reports by name.
#[must_use]
pub fn limitation_detail(name: &str) -> String {
    let named = name.split_once(':').map_or(name, |(head, _target)| head);
    match named {
        crate::limitation::TARGET_RUSTFLAGS_NOT_MERGED
        | rust_mutants::limitation::COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS => {
            "the project configures compiler flags for a target, and the instrumented build \
             does not merge them: which of them apply is cargo's decision"
        }
        rust_mutants::limitation::CARGO_CONFIGURATION_UNREADABLE => {
            "a cargo configuration file could not be read, so the flags it asks for are not \
             in the instrumented build"
        }
        rust_mutants::limitation::DOCTESTS_ROUTED_BY_FILE => {
            "rustdoc compiles a documented example into a binary this run never sees, so no \
             measurement names it: it reaches every mutation in the files its library is \
             made of and narrows none of them"
        }
        rust_mutants::limitation::DOCTESTS_NONE => {
            "the library documents no example, so its documentation target has nothing to \
             run and no mutation is routed to it"
        }
        crate::limitation::PROC_MACRO_EXPANSION_NOT_MEASURED => {
            "a procedural macro decides what it expands to during the build, and a mutation \
             is activated for a test process: the two never meet, and cargo does not rebuild \
             for an environment variable, so what the macro emits is not measured and is not \
             claimed. Its own unit tests are measured like any others"
        }
        rust_mutants::limitation::CUSTOM_HARNESS => {
            "a test binary brings its own harness, so it cannot be asked for one of its \
             tests and is measured whole"
        }
        rust_mutants::limitation::BASELINE_NOT_PASSING => {
            "the target's own tests do not pass with nothing active, so every mutation put \
             to it would come back killed and not one of those kills would be about a \
             mutation"
        }
        rust_mutants::limitation::BASELINE_PASSED_ON_RETRY => {
            "the target's own tests did not pass the first time they were run with nothing \
             active and passed the second time, so something outside the code decided an \
             answer once and every result against this target is worth that much less"
        }
        rust_mutants::limitation::BASELINE_PASSED_UNPARSED => {
            "the tests the target's baseline was read as passing do not come to the count its \
             own summary gave, so a line the suite wrote past the harness read as a result; \
             which tests passed is not known, and its reach is not compared with a control's"
        }
        rust_mutants::limitation::TOUCH_NOT_RECORDED => {
            "the target's guards recorded nothing this run can route by, so every test of \
             it reaches every mutation in it and none of them is narrowed"
        }
        rust_mutants::limitation::TOUCH_LOG_UNREADABLE => {
            "the target recorded what its guards reached and the record did not read back, \
             so nothing of it is believed and every test of it runs"
        }
        rust_mutants::limitation::TARGET_SKIPPED_BY_CONFIGURATION => {
            "the configuration named this target as one never to start, so no mutation was \
             measured against it"
        }
        rust_mutants::limitation::COVERAGE_BUILD_FAILED => {
            "the tree could not be built with coverage instrumentation, so nothing narrows \
             a route and every test of every target runs"
        }
        rust_mutants::limitation::COVERAGE_TOOLS_MISSING => {
            "the LLVM tools this toolchain ships are not installed, so no profile can be \
             read and every test of every target runs"
        }
        rust_mutants::limitation::COVERAGE_NOT_MEASURED => {
            "the coverage tools ran and said nothing a route can rest on, so every test of \
             every target runs"
        }
        _ => "stated by a phase of the run",
    }
    .to_owned()
}
