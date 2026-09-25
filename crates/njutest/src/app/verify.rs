// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest verify`: run a verification and say what it concluded.

use std::io::Write;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::id::{HexDigest, HexDigestError, RunId};

use crate::app::reports;
use crate::assure::identity::{self, Evidence};
use crate::assure::run::{self, Request};
use crate::build::Cargo;
use crate::cache::lock::{self, Lease};
use crate::cache::store::Store;
use crate::cli::{EXIT_ERROR, Environment, Format, Verify};
use crate::config::Config;
use crate::evidence::digest::Mode;
use crate::report::{Verdict, lines};
use crate::run_id;
use crate::trace::{DirSink, Recorder, Sink, StartRecord};
use crate::ui;
use crate::watch::Watch;

/// Why the configuration selected for this invocation could not be loaded.
#[derive(Debug, thiserror::Error)]
enum LoadError {
    /// Configuration bytes did not satisfy the typed configuration contract.
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    /// The held workspace could not supply exact configuration bytes.
    #[error(transparent)]
    Store(#[from] reports::StoreError),
    /// The explicit configuration path cannot be retained exactly in evidence.
    #[error("configuration source path is not portable UTF-8: {0}")]
    SourcePath(#[from] rust_mutants::id::SlashedPathError),
}

struct LoadedConfig {
    config: Config,
    source: String,
}

/// Why a change-scoped run cannot establish the requested scope.
#[derive(Debug, thiserror::Error)]
enum ChangeError {
    /// Git did not supply the requested comparison.
    #[error(
        "git could not say what differs from {base:?}, and a run that cannot see what changed cannot claim to have verified what changed"
    )]
    Unavailable { base: String },
    /// The configured report directory cannot be represented exactly in the exclusion protocol shared with git and the evidence walk.
    #[error(transparent)]
    Evidence(#[from] crate::evidence::tree::ScanError),
}

/// Why a requested durable trace could not be established before verification began.
#[derive(Debug, thiserror::Error)]
enum TraceSetupError {
    /// The outer `njutest` recording directory could not be claimed.
    #[error("the requested trace directory {} could not be created: {source}", path.display())]
    Outer {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The common build namespace could not be claimed inside the new outer recording.
    #[error("the requested build trace namespace {} could not be created: {source}", path.display())]
    Builds {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// One ordinal namespace could not be claimed.
    #[error(
        "the requested trace namespace for configured build {ordinal} at {} could not be created: {source}",
        path.display()
    )]
    Build {
        ordinal: u32,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// One engine recording directory could not be claimed.
    #[error(
        "the requested engine trace for configured build {ordinal} at {} could not be created: {source}",
        path.display()
    )]
    Engine {
        ordinal: u32,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The configuration list exceeded the typed ordinal carried on the wire.
    #[error("configured build index {index} does not fit the trace ordinal")]
    Ordinal { index: usize },
    /// The closed nested trace binding rejected inconsistent fields.
    #[error("configured build {ordinal} could not be bound to its trace: {source}")]
    Binding {
        ordinal: u32,
        #[source]
        source: rust_mutants::trace::NjutestBuildError,
    },
    /// A requested engine trace could not be made durable at the end of its configured build.
    #[error("the engine trace for configured build {ordinal} could not be finalized: {source}")]
    Finalize {
        ordinal: u32,
        #[source]
        source: std::io::Error,
    },
}

/// Why a verification could not establish its immutable pre-run inputs.
#[derive(Debug, thiserror::Error)]
enum InitializationFailure {
    /// The requested configuration could not be loaded.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// The workspace report capability could not be established.
    #[error(transparent)]
    Store(#[from] reports::StoreError),
    /// The final run namespace could not be minted canonically.
    #[error("the run could not be named: {0}")]
    RunId(#[from] rust_mutants::id::RunIdError),
    /// The requested durable trace could not be claimed.
    #[error(transparent)]
    Trace(#[from] TraceSetupError),
}

struct Initialized {
    root: PathBuf,
    config: Config,
    configuration: String,
    reports: reports::Store,
    started: Timestamp,
    identity: RunId,
    trace: Recorder,
}

/// Why configured-build evidence could not take its one legal terminal transition into a durable report document.
#[derive(Debug, thiserror::Error)]
enum CompletionFailure {
    /// Progress output failed while the terminal model phase was running.
    #[error("writing model-phase progress: {source}")]
    Output {
        /// The stream failure.
        #[source]
        source: std::io::Error,
    },
    /// The configured-build lattice contradicted itself.
    #[error(transparent)]
    Lattice(#[from] crate::report::across::ConfiguredError),
    /// A report type-state transition rejected mismatched evidence.
    #[error(transparent)]
    Report(#[from] crate::report::CompletionError),
    /// The exact post-lattice model record sequence was incomplete or out of order.
    #[error(transparent)]
    ModelBatch(#[from] crate::report::ModelBatchError),
    /// The final report namespace could not be exclusively reserved.
    #[error(transparent)]
    Store(#[from] reports::StoreError),
    /// The final isolated proof workspace could not be created.
    #[error(transparent)]
    Scratch(#[from] crate::scratch::ScratchError),
    /// The model phase could not construct or execute its closed proof inputs.
    #[error(transparent)]
    Model(#[from] crate::error::RunnerError),
}

/// A completion failure, distinguished by whether abandoning its private publication namespace failed too.
#[derive(Debug, thiserror::Error)]
enum ReconciliationFailure {
    /// The private staging tree was removed after completion failed.
    #[error(transparent)]
    Completion(CompletionFailure),
    /// Neither completion nor removal of its unpublished evidence succeeded.
    #[error("{completion}; abandoning the unpublished run also failed: {abort}")]
    CompletionAndAbort {
        /// Why no complete document could be constructed.
        completion: CompletionFailure,
        /// Why the staging tree could not be removed.
        abort: reports::StoreError,
    },
}

/// A terminal report plus exclusive ownership of the directory into which it and any model artifacts will be persisted.
struct Reconciled {
    report: crate::report::ReportDocument,
    kept: Vec<PathBuf>,
    directory: reports::RunDirectory,
}

type ReportAnswer = (Verdict, Option<crate::report::ConclusionAccounting>);

/// Where scheduling state for interrupted runs lives, beside the answers finished runs left.
pub const CHECKPOINTS: &str = "checkpoints";

/// How long a run waits for another run of the same inputs before doing the work itself.
pub const LEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(30);

/// Runs a verification.
///
/// # Errors
/// Returns an output failure when the command's diagnostic or report stream cannot be written completely.
pub fn run(
    arguments: &Verify,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let initialized = match initialize(arguments, environment) {
        Ok(initialized) => initialized,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    run_initialized(
        arguments,
        environment,
        initialized,
        Streams {
            out: stdout,
            err: stderr,
        },
    )
}

fn run_initialized(
    arguments: &Verify,
    environment: &Environment,
    initialized: Initialized,
    streams: Streams<'_>,
) -> std::io::Result<u8> {
    let (stdout, stderr) = (streams.out, streams.err);
    let Initialized {
        root,
        config,
        configuration,
        reports,
        started,
        identity,
        trace,
    } = initialized;
    let cancel = environment.cancel.clone();

    let watch = Watch::new(&cancel, &trace);
    let changed = match asked_about(arguments, (&root, &config), environment, watch) {
        Ok(changed) => changed,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let asked = Asked {
        root: &root,
        config: &config,
        environment,
        changed: changed.as_ref(),
    };
    let evidence = match evidence_of(arguments, &asked, &cancel) {
        Ok(evidence) => evidence,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let cache_identity = match reported_cache_identity(&evidence, stderr)? {
        ControlFlow::Continue(identity) => identity,
        ControlFlow::Break(code) => return Ok(code),
    };
    let store = store_of(environment, &config);
    let lease = match establishment(
        &Asking {
            store: &store,
            reports: &reports,
            identity: cache_identity.as_ref(),
            run_id: &identity,
            root: &root,
            environment,
        },
        arguments,
        &cancel,
        Streams {
            out: stdout,
            err: stderr,
        },
    )? {
        ControlFlow::Break(code) => return Ok(code),
        ControlFlow::Continue(lease) => lease,
    };
    let code = establish(
        &Establishing {
            arguments,
            environment,
            root: &root,
            config,
            configuration: &configuration,
            reports: &reports,
            identity: &identity,
            started,
            evidence: &evidence,
            changed: &changed,
            store: &store,
            trace: &trace,
            watch,
        },
        Streams {
            out: stdout,
            err: stderr,
        },
    );
    drop(lease);
    code
}

fn canonical_cache_identity(evidence: &Evidence) -> Result<Option<HexDigest>, HexDigestError> {
    if evidence.is_known() {
        HexDigest::try_from(evidence.identity.as_str()).map(Some)
    } else {
        Ok(None)
    }
}

fn reported_cache_identity(
    evidence: &Evidence,
    stderr: &mut dyn Write,
) -> std::io::Result<ControlFlow<u8, Option<HexDigest>>> {
    match canonical_cache_identity(evidence) {
        Ok(identity) => Ok(ControlFlow::Continue(identity)),
        Err(error) => {
            super::diagnose(
                stderr,
                &format!("the measured evidence identity is not canonical: {error}"),
            )?;
            Ok(ControlFlow::Break(EXIT_ERROR))
        }
    }
}

fn initialize(
    arguments: &Verify,
    environment: &Environment,
) -> Result<Initialized, InitializationFailure> {
    let root = environment.rooted(arguments.directory.as_deref());
    let workspace = reports::WorkspaceRoot::open(&root)?;
    let loaded = load(arguments, &workspace)?;
    let reports = workspace.store(&loaded.config.reports.directory)?;
    let started = Timestamp::now();
    let identity = run_id::mint(started, std::process::id())?;
    let trace = recorder(
        arguments,
        &Recording {
            root: &root,
            identity: &identity,
            contract: loaded.config.contract,
        },
    )?;
    Ok(Initialized {
        root,
        config: loaded.config,
        configuration: loaded.source,
        reports,
        started,
        identity,
        trace,
    })
}

struct Establishing<'a> {
    arguments: &'a Verify,
    environment: &'a Environment,
    root: &'a Path,
    config: Config,
    configuration: &'a str,
    reports: &'a reports::Store,
    identity: &'a RunId,
    started: Timestamp,
    evidence: &'a Evidence,
    changed: &'a Option<crate::git::Change>,
    store: &'a Store,
    trace: &'a Recorder,
    watch: Watch<'a>,
}

/// Which part of the catalog the command line asked for, refused before anything is built.
///
/// # Errors
/// What is wrong with the text, as a reader would want it said.
fn part_of(
    arguments: &Verify,
) -> Result<Option<rust_mutants::run::Shard>, rust_mutants::run::ShardError> {
    arguments
        .shard
        .as_deref()
        .map(rust_mutants::run::Shard::parse)
        .transpose()
}

/// Runs the verification, writes what it concluded, and stores it for the next run of the same inputs.
fn establish(establishing: &Establishing<'_>, streams: Streams<'_>) -> std::io::Result<u8> {
    let arguments = establishing.arguments;
    let Streams {
        out: stdout,
        err: stderr,
    } = streams;
    let shard = match part_of(arguments) {
        Ok(shard) => shard,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let request = asking(establishing, shard);
    let reconciled = match reconciled(&request, establishing, stderr)? {
        Ok(reconciled) => reconciled,
        Err(code) => return Ok(code),
    };
    finish_established(
        establishing,
        &request,
        reconciled,
        Streams {
            out: stdout,
            err: stderr,
        },
    )
}

fn finish_established(
    establishing: &Establishing<'_>,
    request: &Request,
    reconciled: Reconciled,
    streams: Streams<'_>,
) -> std::io::Result<u8> {
    let Establishing {
        arguments,
        environment,
        root,
        evidence,
        store,
        reports,
        trace,
        ..
    } = *establishing;
    let Streams {
        out: stdout,
        err: stderr,
    } = streams;
    let Reconciled {
        report,
        kept,
        directory,
    } = reconciled;
    let (verdict, accounting) = match report_answer(&report, stderr)? {
        Ok(answer) => answer,
        Err(code) => return Ok(code),
    };
    let published = match persist_or_report(
        Persisting {
            root,
            report: &report,
            request,
            store,
            reports,
            store_it: !arguments.no_cache
                && evidence.is_known()
                && !environment.cancel.is_cancelled(),
            kept: &kept,
            directory,
        },
        arguments,
        trace,
        stderr,
    )? {
        ControlFlow::Continue(document) => document,
        ControlFlow::Break(code) => return Ok(code),
    };

    if let Err(error) = trace.run_end(verdict, accounting, None) {
        super::diagnose(
            stderr,
            &format!("the trace could not be finalized: {error}"),
        )?;
        return Ok(EXIT_ERROR);
    }

    let rendered = match said(&report, root, &published, (environment, arguments.format)) {
        Ok(rendered) => rendered,
        Err(error) => {
            super::diagnose(
                stderr,
                &format!("the report could not be projected: {error}"),
            )?;
            return Ok(EXIT_ERROR);
        }
    };
    if let Err(error) = stdout.write_all(rendered.as_bytes()) {
        super::diagnose(stderr, &format!("the report could not be written: {error}"))?;
        return Ok(EXIT_ERROR);
    }
    Ok(report.verdict().exit_code())
}

fn persist_or_report(
    persisting: Persisting<'_>,
    arguments: &Verify,
    trace: &Recorder,
    stderr: &mut dyn Write,
) -> std::io::Result<ControlFlow<u8, Published>> {
    match persist(persisting, arguments, stderr) {
        Ok(document) => Ok(ControlFlow::Continue(document)),
        Err(error) => {
            match &error {
                PersistError::Store(source) => super::complain(stderr, source, source.code())?,
                PersistError::Output { .. } => super::diagnose(stderr, &error.to_string())?,
            }
            if let Err(trace_error) = trace.run_end(
                Verdict::Error,
                None,
                Some("the completed report could not be persisted".to_owned()),
            ) {
                super::diagnose(
                    stderr,
                    &format!("the trace could not be finalized: {trace_error}"),
                )?;
            }
            Ok(ControlFlow::Break(EXIT_ERROR))
        }
    }
}

fn report_answer(
    document: &crate::report::ReportDocument,
    stderr: &mut dyn Write,
) -> std::io::Result<Result<ReportAnswer, u8>> {
    match document {
        crate::report::ReportDocument::Complete(report) => match report.conclusion() {
            Ok(conclusion) => Ok(Ok((conclusion.verdict, Some(conclusion.accounting)))),
            Err(error) => {
                super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
                Ok(Err(EXIT_ERROR))
            }
        },
        crate::report::ReportDocument::Shard(report) => Ok(Ok((report.verdict(), None))),
    }
}

/// What every build the configuration named establishes, as one report and what the run kept.
fn reconciled(
    request: &Request,
    establishing: &Establishing<'_>,
    stderr: &mut dyn Write,
) -> std::io::Result<Result<Reconciled, u8>> {
    let measured = match every_build(request, establishing, stderr)? {
        Ok(measured) => measured,
        Err(code) => return Ok(Err(code)),
    };
    match reconcile_measured(request, establishing, measured, stderr) {
        ControlFlow::Continue(completed) => Ok(Ok(completed)),
        ControlFlow::Break(error) => {
            if let Err(trace_error) =
                establishing
                    .trace
                    .run_end(Verdict::Error, None, Some(error.to_string()))
            {
                super::diagnose(
                    stderr,
                    &format!("the trace could not be finalized: {trace_error}"),
                )?;
            }
            super::diagnose(stderr, &error.to_string())?;
            Ok(Err(EXIT_ERROR))
        }
    }
}

fn reconcile_measured(
    request: &Request,
    establishing: &Establishing<'_>,
    measured: Vec<(String, rust_mutants::cargo::BuildSelection, run::Outcome)>,
    stderr: &mut dyn Write,
) -> ControlFlow<ReconciliationFailure, Reconciled> {
    let mut kept = Vec::new();
    let mut parts = Vec::with_capacity(measured.len());
    let mut prepared = Vec::with_capacity(measured.len());
    for (name, selection, outcome) in measured {
        kept.extend(outcome.kept);
        prepared.push((name.clone(), outcome.model));
        parts.push((name, selection, outcome.report));
    }
    let measurements = match crate::report::across::BuildMeasurements::checked(parts) {
        Ok(measurements) => measurements,
        Err(error) => {
            return ControlFlow::Break(ReconciliationFailure::Completion(CompletionFailure::from(
                crate::report::across::ConfiguredError::from(error),
            )));
        }
    };
    let latticed = match crate::report::across::configured(establishing.identity, &measurements) {
        Ok(latticed) => latticed,
        Err(error) => {
            return ControlFlow::Break(ReconciliationFailure::Completion(CompletionFailure::from(
                error,
            )));
        }
    };
    let directory = match establishing
        .reports
        .claim_writable_run(establishing.identity)
    {
        Ok(directory) => directory,
        Err(error) => {
            return ControlFlow::Break(ReconciliationFailure::Completion(CompletionFailure::from(
                error,
            )));
        }
    };
    let completed = complete_lattice(
        latticed,
        Completing {
            request,
            establishing,
            prepared,
            directory: &directory,
            kept: &mut kept,
            stderr,
        },
    );
    let report = match completed {
        Ok(report) => report,
        Err(completion) => {
            return ControlFlow::Break(match directory.abort() {
                Ok(()) => ReconciliationFailure::Completion(completion),
                Err(abort) => ReconciliationFailure::CompletionAndAbort { completion, abort },
            });
        }
    };
    ControlFlow::Continue(Reconciled {
        report,
        kept,
        directory,
    })
}

struct Completing<'request, 'establishing, 'context, 'out> {
    request: &'request Request,
    establishing: &'establishing Establishing<'context>,
    prepared: Vec<(String, crate::assure::model::Preparation)>,
    directory: &'request reports::RunDirectory,
    kept: &'out mut Vec<PathBuf>,
    stderr: &'out mut dyn Write,
}

type ConfiguredBuildOutcome = (String, rust_mutants::cargo::BuildSelection, run::Outcome);
type ConfiguredBuildResults = std::io::Result<Result<Vec<ConfiguredBuildOutcome>, u8>>;

struct Modeling<'request, 'establishing, 'context, 'out> {
    request: &'request Request,
    establishing: &'establishing Establishing<'context>,
    directory: &'request reports::RunDirectory,
    kept: &'out mut Vec<PathBuf>,
    stderr: &'out mut dyn Write,
}

#[cfg_attr(
    not(unix),
    expect(
        clippy::result_large_err,
        reason = "the error carries the runner's own, which this platform lays out past the lint's threshold and unix does not; boxing a shared error type to answer a platform's layout would move the cost to every caller on both"
    )
)]
fn complete_lattice(
    latticed: crate::report::LatticedDocument,
    completing: Completing<'_, '_, '_, '_>,
) -> Result<crate::report::ReportDocument, CompletionFailure> {
    let Completing {
        request,
        establishing,
        prepared,
        directory,
        kept,
        stderr,
    } = completing;
    let latticed = match latticed {
        crate::report::LatticedDocument::Complete(latticed) => latticed,
        crate::report::LatticedDocument::Shard(shard) => {
            return Ok(crate::report::ReportDocument::Shard(shard));
        }
    };
    if latticed.contract() != crate::config::Contract::VerifiedV1 {
        crate::assure::model::confirm_not_required(&prepared)
            .map_err(crate::error::RunnerError::from)?;
        return Ok(crate::report::ReportDocument::Complete(
            latticed.complete_without_models()?,
        ));
    }

    let candidates = latticed.model_candidates()?;
    let plan = crate::assure::model::Plan::checked(&candidates, prepared)
        .map_err(crate::error::RunnerError::from)?;
    let records = if plan.is_empty() {
        Vec::new()
    } else {
        model_records(
            plan,
            Modeling {
                request,
                establishing,
                directory,
                kept,
                stderr,
            },
        )?
    };
    let batch = crate::report::ModelBatch::checked(&candidates, records)?;
    Ok(crate::report::ReportDocument::Complete(
        latticed.attach_models(batch)?,
    ))
}

#[cfg_attr(
    not(unix),
    expect(
        clippy::result_large_err,
        reason = "the error carries the runner's own, which this platform lays out past the lint's threshold and unix does not; boxing a shared error type to answer a platform's layout would move the cost to every caller on both"
    )
)]
fn model_records(
    plan: crate::assure::model::Plan,
    modeling: Modeling<'_, '_, '_, '_>,
) -> Result<Vec<crate::report::ModelRecord>, CompletionFailure> {
    let Modeling {
        request,
        establishing,
        directory,
        kept,
        stderr,
    } = modeling;
    let verified = request
        .config
        .verified()
        .map_err(crate::assure::model::ModelError::from)
        .map_err(crate::error::RunnerError::from)?
        .ok_or(crate::assure::model::ModelError::Configuration(
            crate::config::VerificationError::WrongContract,
        ))
        .map_err(crate::error::RunnerError::from)?;
    let scratch = crate::scratch::Scratch::create(
        &establishing.environment.temp_directory,
        establishing.identity,
        establishing.started,
    )?;
    let (target, tree_written, asked) = plan.into_parts();
    let artifact_dir = directory.path().join("model");
    let mut notes = ui::Notes::of(establishing.arguments.ui, stderr);
    notes
        .phase("model")
        .map_err(|source| CompletionFailure::Output { source })?;
    establishing.trace.stage("model");
    let phase = establishing.trace.phase("model-prove");
    let decided = crate::assure::model::prove(
        &crate::assure::model::Proving {
            scratch_dir: scratch.dir(),
            environment: &establishing.environment.vars,
            target: &target,
            verified,
            artifact_dir: &artifact_dir,
            tree_written,
        },
        asked,
        establishing.watch,
    )?;
    phase.end();
    let preserved = if request.keep_temp {
        scratch.keep()?
    } else {
        scratch.close()?
    };
    for path in &preserved {
        notes
            .note("kept", &path.display().to_string())
            .map_err(|source| CompletionFailure::Output { source })?;
    }
    notes
        .finish()
        .map_err(|source| CompletionFailure::Output { source })?;
    kept.extend(preserved);
    Ok(decided
        .into_iter()
        .map(crate::assure::model::Decided::into_record)
        .collect())
}

/// Every build the configuration named, measured, with what a report calls each.
///
/// The builds are measured in the order the file names them, the one `[execution]` describes first.
/// Each is a program of its own, so each gets a scratch directory and an engine recording of its own, keyed by the name a report will call it.
fn every_build(
    request: &Request,
    establishing: &Establishing<'_>,
    stderr: &mut dyn Write,
) -> ConfiguredBuildResults {
    let Establishing {
        arguments,
        environment,
        trace,
        watch,
        ..
    } = *establishing;
    let configured = match configured_builds(request, establishing) {
        Ok(configured) => configured,
        Err(error) => {
            if let Err(trace_error) = trace.run_end(Verdict::Error, None, Some(error.to_string())) {
                super::diagnose(
                    stderr,
                    &format!("the trace could not be finalized: {trace_error}"),
                )?;
            }
            super::diagnose(stderr, &error.to_string())?;
            return Ok(Err(EXIT_ERROR));
        }
    };
    let mut measured = Vec::new();
    for configured in configured {
        let ConfiguredBuild {
            build,
            selection,
            binding,
            trace: engine_trace,
        } = configured;
        let ordinal = binding.ordinal();
        let name = binding.name().to_owned();
        let mut asked = Request {
            build,
            run_id: binding.internal_run_id().clone(),
            engine_trace: engine_trace.start(),
            ..request.clone()
        };
        asked.evidence = request.evidence.for_build(&asked.build);
        let result = {
            let mut notes = ui::Notes::of(arguments.ui, &mut *stderr);
            run::run(&asked, environment, &mut notes, watch)
        };
        let ended = match &result {
            Ok(_measured) => asked
                .engine_trace
                .run_end(rust_mutants::trace::RunOutcome::Completed, None),
            Err(error) => asked.engine_trace.run_end(
                rust_mutants::trace::RunOutcome::Failed,
                Some(error.to_string()),
            ),
        };
        if let Err(source) = ended {
            let error = TraceSetupError::Finalize { ordinal, source };
            if let Err(trace_error) = trace.run_end(Verdict::Error, None, Some(error.to_string())) {
                super::diagnose(
                    stderr,
                    &format!("the trace could not be finalized: {trace_error}"),
                )?;
            }
            super::diagnose(stderr, &error.to_string())?;
            return Ok(Err(EXIT_ERROR));
        }
        match result {
            Ok(outcome) => measured.push((name, selection, outcome)),
            Err(error) => {
                if let Err(trace_error) =
                    trace.run_end(Verdict::Error, None, Some(error.to_string()))
                {
                    super::diagnose(
                        stderr,
                        &format!("the trace could not be finalized: {trace_error}"),
                    )?;
                }
                super::complain(stderr, &error, error.code())?;
                return Ok(Err(EXIT_ERROR));
            }
        }
    }
    Ok(Ok(measured))
}

/// Everything one run is asking for, gathered from the arguments, the configuration and the store.
fn asking(establishing: &Establishing<'_>, shard: Option<rust_mutants::run::Shard>) -> Request {
    let Establishing {
        arguments,
        root,
        configuration,
        identity,
        started,
        evidence,
        store,
        ..
    } = *establishing;
    Request {
        root: root.to_path_buf(),
        configuration: configuration.to_owned(),
        config: establishing.config.clone(),
        build: establishing.config.execution.build(),
        packages: packages(arguments, &establishing.config),
        test_args: harness_args(arguments, &establishing.config),
        cargo: Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
        keep_temp: arguments.keep_temp,
        run_id: identity.clone(),
        started,
        engine_trace: rust_mutants::trace::Recorder::disabled(),
        evidence: evidence.clone(),
        changed: establishing.changed.clone(),
        checkpoints: (!arguments.no_cache).then(|| store.root().join(CHECKPOINTS)),
        evidence_store: (!arguments.no_cache).then(|| store.root().to_path_buf()),
        shard,
    }
}

/// What a run has to say, in the shape the thing reading it wants.
///
/// Guessed from where the output is going when nobody said, and taken at its word when somebody did.
/// The guess is right for the two readers it was written for — a person at a terminal and a program reading a stream — and wrong for the one that is neither, which runs the same command through a pipe and is handed a stream because of how it was started rather than because of what it is.
/// Every shape is a projection of one value (ADR 0020),
/// so answering a third reader is naming the projection, not writing a report again.
fn said(
    report: &crate::report::ReportDocument,
    root: &Path,
    published: &Published,
    (environment, asked): (&Environment, Option<Format>),
) -> Result<String, SayingError> {
    let shape = asked.unwrap_or(if environment.terminal.drawing {
        Format::Human
    } else {
        Format::Lines
    });
    match report {
        crate::report::ReportDocument::Shard(_) => {
            crate::report::json::document_any(report).map_err(SayingError::from)
        }
        crate::report::ReportDocument::Complete(report) => said_complete(
            report,
            &Saying {
                root,
                said_document: &published.document,
                said: &published.said,
                environment,
                shape,
            },
        ),
    }
}

#[derive(Clone, Copy)]
struct Saying<'a> {
    root: &'a Path,
    said_document: &'a str,
    said: &'a [lines::Said],
    environment: &'a Environment,
    shape: Format,
}

#[derive(Debug, thiserror::Error)]
enum SayingError {
    #[error(transparent)]
    Count(#[from] crate::report::CountError),
    #[error(transparent)]
    Report(#[from] crate::report::json::ReportError),
}

fn said_complete(
    report: &crate::report::Report,
    saying: &Saying<'_>,
) -> Result<String, SayingError> {
    let Saying {
        root,
        said_document,
        said,
        environment,
        shape,
    } = *saying;
    match shape {
        Format::Json => Ok(crate::report::json::document_any(
            &crate::report::ReportDocument::Complete(report.clone()),
        )?),
        Format::Lines => Ok(lines::kept(report, said)?),
        Format::Spec => Ok(crate::report::spec::page(report)?),
        Format::Human | Format::Agent => {
            let sources = crate::presentation::Sources::read(root, report)?;
            let told = crate::presentation::Told::of(report, &sources, said_document)?;
            if shape == Format::Agent {
                Ok(crate::presentation::agent::brief(&told))
            } else {
                Ok(crate::presentation::human::draw(
                    &told,
                    environment.terminal,
                ))
            }
        }
    }
}

/// The change set, asked for with the directories this project writes left out.
fn asked_about(
    arguments: &Verify,
    about: (&Path, &Config),
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<Option<crate::git::Change>, ChangeError> {
    let (root, config) = about;
    let excluded = crate::evidence::tree::Excluded::beside(config.reports.directory.as_path())?;
    change_set(
        arguments,
        &crate::git::Asked {
            root,
            env: &environment.vars,
            excluded: &excluded,
            watch,
        },
    )
}

/// The change set a run was asked to mutate within, or nothing when it was not asked.
fn change_set(
    arguments: &Verify,
    asked: &crate::git::Asked<'_>,
) -> Result<Option<crate::git::Change>, ChangeError> {
    if !arguments.changed && arguments.changed_from.is_none() {
        return Ok(None);
    }
    let base = arguments
        .changed_from
        .as_deref()
        .unwrap_or(crate::git::DEFAULT_BASE);
    crate::git::changed(asked, base)
        .map(Some)
        .ok_or_else(|| ChangeError::Unavailable {
            base: base.to_owned(),
        })
}

/// What a run was asked to verify, before anything has been established about it.
#[derive(Clone, Copy)]
struct Asked<'a> {
    root: &'a Path,
    config: &'a Config,
    environment: &'a Environment,
    changed: Option<&'a crate::git::Change>,
}

/// How much of the workspace the run looked at, which is part of what it is: a run about one package established less than one about everything, and the two must never share a stored answer.
fn mode_of(arguments: &Verify, config: &Config, changed: Option<&crate::git::Change>) -> Mode {
    if let Some(change) = changed {
        return Mode::Changed {
            base: change.base.clone(),
        };
    }
    let named = packages(arguments, config);
    if named.is_empty() {
        return Mode::Full;
    }
    Mode::Scoped { packages: named }
}

/// What this run is, as numbers, or nothing when the tree could not be read.
/// A tree that cannot be measured is a limitation the report states, not a reason to refuse to verify it.
#[derive(Debug, thiserror::Error)]
enum EvidenceError {
    /// The active Cargo/rustc pair could not be identified.
    #[error(transparent)]
    Toolchain(#[from] rust_mutants::cargo::CargoError),
    /// The exact tree/configuration inputs could not be reduced to one identity.
    #[error(transparent)]
    Identity(#[from] identity::IdentityError),
    /// The configured timeout cannot be represented by the fixed report/cache wire.
    #[error("the configured timeout {timeout:?} exceeds the u64 millisecond boundary")]
    Timeout {
        /// The unrepresentable duration.
        timeout: std::time::Duration,
    },
}

fn evidence_of(
    arguments: &Verify,
    asked: &Asked<'_>,
    cancel: &rust_mutants::runner::Cancel,
) -> Result<Evidence, EvidenceError> {
    let Asked {
        root,
        config,
        environment,
        changed,
    } = *asked;
    let toolchain = rust_mutants::cargo::Toolchain::locate(
        &rust_mutants::cargo::LocateOptions {
            cargo: None,
            search_path: rust_mutants::vars::search_path(&environment.vars),
            env: Some(environment.vars.clone()),
        },
        root,
        cancel,
    )?;
    let mode = mode_of(arguments, config, changed);
    let machine = identity::Machine {
        toolchain: &toolchain.to_string(),
        platform: toolchain.host(),
    };
    let asked = identity::Asked {
        root,
        config,
        machine: &machine,
        vars: &environment.vars,
        elsewhere: &[&environment.cache_directory],
    };
    let common = crate::evidence::key::Common {
        toolchain: machine.toolchain.to_owned(),
        platform: machine.platform.to_owned(),
        environment: identity::inputs(
            &asked,
            mode.clone(),
            &harness_args(arguments, config),
            arguments.shard.clone(),
        )?
        .environment,
        contract: format!("{:?}", config.contract).to_lowercase(),
        test_args: harness_args(arguments, config),
        build: config.execution.build().selection(),
        timeout_ms: u64::try_from(config.execution.timeout.as_millis()).map_err(|_overflow| {
            EvidenceError::Timeout {
                timeout: config.execution.timeout,
            }
        })?,
        steps: config.execution.steps,
        versions: vec![
            format!("njutest {}", crate::VERSION),
            format!("rust-mutants {}", rust_mutants::VERSION),
        ],
        corpus: String::new(),
    };
    identity::of(&asked, mode, common, arguments.shard.clone()).map_err(EvidenceError::from)
}

/// The store of earlier answers, bounded the way the configuration says.
fn store_of(environment: &Environment, config: &Config) -> Store {
    Store::new(
        &environment.cache_directory,
        config.cache.max_bytes,
        config.cache.ttl,
    )
}

/// Waits for whatever run is already establishing this identity, so the same work is not done twice at once.
/// A claim that cannot be taken is not a reason to refuse: the run does the work again rather than not at all.
fn claim(
    asking: &Asking<'_>,
    arguments: &Verify,
    cancel: &rust_mutants::runner::Cancel,
    stderr: &mut dyn Write,
) -> Result<Option<Lease>, std::io::Error> {
    let Some(identity) = asking.identity else {
        return Ok(None);
    };
    let path = asking.store.lease(identity);
    let mut waited = false;
    let taken = lock::claim(&path, LEASE_TIMEOUT, cancel, &mut || waited = true);
    let mut notes = ui::Notes::of(arguments.ui, stderr);
    if waited {
        notes.note("waiting", "another run of the same inputs is under way")?;
    }
    let lease = match taken {
        Ok(lease) => Some(lease),
        Err(error) => {
            notes.note("unclaimed", &error.to_string())?;
            None
        }
    };
    notes.finish()?;
    Ok(lease)
}

/// The two streams a command writes to.
struct Streams<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

/// A finished run and everywhere it goes.
struct Persisting<'a> {
    root: &'a Path,
    report: &'a crate::report::ReportDocument,
    request: &'a Request,
    store: &'a Store,
    reports: &'a reports::Store,
    store_it: bool,
    kept: &'a [PathBuf],
    directory: reports::RunDirectory,
}

/// Why a completed document could not finish its durable publication and user-visible accounting.
#[derive(Debug, thiserror::Error)]
enum PersistError {
    /// The report store rejected or could not durably publish the document.
    #[error(transparent)]
    Store(#[from] reports::StoreError),
    /// Progress output failed after the operation it describes.
    #[error("writing report publication progress: {source}")]
    Output {
        /// The stream failure.
        #[source]
        source: std::io::Error,
    },
}

/// Retention is a derived, post-publication maintenance step.
/// Its failure can never turn an already-durable report back into a failed publication.
enum RetentionOutcome {
    /// Index publication did not establish a safe retention boundary.
    NotAttempted,
    /// Old runs were retired after the new authority became durable.
    Complete(Vec<PathBuf>),
    /// The report is durable, but maintenance could not be completed.
    Incomplete(reports::StoreError),
}

/// Writes the report where a reader will look for it, retires what the configuration no longer keeps, and stores the answer for the next run of the same inputs.
/// Returns the exit code only when the report could not be written, which is the one failure that stops the run from having answered at all.
fn persist(
    persisting: Persisting<'_>,
    arguments: &Verify,
    stderr: &mut dyn Write,
) -> Result<Published, PersistError> {
    let Persisting {
        root,
        report,
        request,
        store,
        reports,
        store_it,
        kept,
        directory,
    } = persisting;
    let written = reports::keep_claimed(report, directory)?;
    let retention = if written.indexes.permits_retention() {
        match reports.retain(request_keep(request)) {
            Ok(removed) => RetentionOutcome::Complete(removed),
            Err(error) => RetentionOutcome::Incomplete(error),
        }
    } else {
        RetentionOutcome::NotAttempted
    };
    let stored = match (store_it, report) {
        (true, crate::report::ReportDocument::Complete(report)) => match store.put(report) {
            Ok(()) => None,
            Err(error) => Some(error),
        },
        (false, _) | (true, crate::report::ReportDocument::Shard(_)) => None,
    };
    let mut notes = ui::Notes::of(arguments.ui, stderr);
    notes
        .note("report", &written.said_document)
        .map_err(|source| PersistError::Output { source })?;
    note_publication_status(&mut notes, &written.indexes)?;
    match &retention {
        RetentionOutcome::NotAttempted => {}
        RetentionOutcome::Complete(removed) => {
            for path in removed {
                notes
                    .note("retired", &path.display().to_string())
                    .map_err(|source| PersistError::Output { source })?;
            }
        }
        RetentionOutcome::Incomplete(error) => {
            notes
                .note("report-retention-unavailable", &error.to_string())
                .map_err(|source| PersistError::Output { source })?;
        }
    }
    let kept_record_failure = if kept.is_empty() {
        None
    } else {
        match crate::kept::record(root, report.run_id(), Timestamp::now(), kept) {
            Ok(_ledger) => None,
            Err(error) => Some(error),
        }
    };
    for path in kept {
        notes
            .note("kept", &path.display().to_string())
            .map_err(|source| PersistError::Output { source })?;
    }
    if let Some(error) = stored {
        notes
            .note("not-stored", &error.to_string())
            .map_err(|source| PersistError::Output { source })?;
    }
    if let Some(error) = kept_record_failure {
        notes
            .note("kept-ledger-unavailable", &error.to_string())
            .map_err(|source| PersistError::Output { source })?;
    }
    notes
        .finish()
        .map_err(|source| PersistError::Output { source })?;
    Ok(Published {
        document: written.said_document,
        said: written.said,
    })
}

/// Where a published run's files are, as a reader is told them.
struct Published {
    /// The canonical document, from the project's own root.
    document: String,
    /// Every file the run sealed that a reader is told the path of.
    said: Vec<lines::Said>,
}

fn note_publication_status(
    notes: &mut ui::Notes<'_>,
    publication: &reports::IndexPublication,
) -> Result<(), PersistError> {
    let result = match publication {
        reports::IndexPublication::NotRequired | reports::IndexPublication::Complete => Ok(()),
        reports::IndexPublication::Incomplete { index, error } => notes.note(
            "report-index-unavailable",
            &format!("{}: {error}", index.file()),
        ),
        reports::IndexPublication::RunChanged { error } => {
            notes.note("report-integrity-unavailable", &error.to_string())
        }
    };
    result.map_err(|source| PersistError::Output { source })
}

/// Whether an earlier run of the same inputs has already answered, and the claim this run holds while it establishes its own.
enum Settled {
    /// An earlier run answered, and this is the exit code.
    Answered(u8),
    /// Nothing is stored; the claim this run holds while it establishes one.
    Establish(Option<Lease>),
}

fn establishment(
    asking: &Asking<'_>,
    arguments: &Verify,
    cancel: &rust_mutants::runner::Cancel,
    streams: Streams<'_>,
) -> std::io::Result<ControlFlow<u8, Option<Lease>>> {
    match already_answered(asking, arguments, cancel, streams)? {
        Settled::Answered(code) => Ok(ControlFlow::Break(code)),
        Settled::Establish(lease) => Ok(ControlFlow::Continue(lease)),
    }
}

/// Whether the store already answers, unless the run was told to establish everything afresh or the tree could not be measured.
fn already_answered(
    asking: &Asking<'_>,
    arguments: &Verify,
    cancel: &rust_mutants::runner::Cancel,
    streams: Streams<'_>,
) -> Result<Settled, std::io::Error> {
    if arguments.no_cache || asking.identity.is_none() {
        return Ok(Settled::Establish(None));
    }
    settled(asking, arguments, cancel, streams)
}

/// Asks the store, waits for whoever is already establishing this identity, and asks again.
fn settled(
    asking: &Asking<'_>,
    arguments: &Verify,
    cancel: &rust_mutants::runner::Cancel,
    streams: Streams<'_>,
) -> Result<Settled, std::io::Error> {
    let Streams {
        out: stdout,
        err: stderr,
    } = streams;
    if let Reuse::Answered(code) = reuse(asking, arguments.format, stdout, stderr)? {
        return Ok(Settled::Answered(code));
    }
    let lease = claim(asking, arguments, cancel, stderr)?;
    if lease.is_some()
        && let Reuse::Answered(code) = reuse(asking, arguments.format, stdout, stderr)?
    {
        return Ok(Settled::Answered(code));
    }
    Ok(Settled::Establish(lease))
}

/// What a run needs to ask the store of earlier answers.
struct Asking<'a> {
    store: &'a Store,
    reports: &'a reports::Store,
    identity: Option<&'a HexDigest>,
    run_id: &'a RunId,
    root: &'a Path,
    /// Where the answer is going, so a run that reads one back says it the way a run that established one would.
    environment: &'a Environment,
}

/// Whether this run has to establish anything at all.
enum Reuse {
    /// An earlier run of the same inputs answered, and this is the exit code.
    Answered(u8),
    /// Nothing is stored, or what is stored cannot be believed.
    Establish,
}

/// Reads back what an earlier run of the same inputs established, and writes it as this run's report.
fn reuse(
    asking: &Asking<'_>,
    asked: Option<Format>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<Reuse> {
    let Asking {
        store,
        reports,
        identity,
        run_id,
        root,
        environment,
    } = *asking;
    let Some(identity) = identity else {
        return Ok(Reuse::Establish);
    };
    let stored = match store.get(identity) {
        Ok(Some(stored)) => stored,
        Ok(None) => return Ok(Reuse::Establish),
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(Reuse::Establish);
        }
    };
    let report = match stored.read_back_as(run_id) {
        Ok(report) => report,
        Err(error) => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: cached answer cannot be reissued: {error}",
                    crate::error::CACHE_CORRUPT.code
                ),
            )?;
            return Ok(Reuse::Establish);
        }
    };
    let written = match reports.keep(&report) {
        Ok(written) => written,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(Reuse::Establish);
        }
    };
    diagnose_publication_status(stderr, &written.indexes)?;
    let shape = asked.unwrap_or(if environment.terminal.drawing {
        Format::Human
    } else {
        Format::Lines
    });
    let text = match said_complete(
        &report,
        &Saying {
            root,
            said_document: &written.said_document,
            said: &written.said,
            environment,
            shape,
        },
    ) {
        Ok(text) => text,
        Err(error) => {
            super::diagnose(
                stderr,
                &format!("the cached report could not be projected: {error}"),
            )?;
            return Ok(Reuse::Answered(EXIT_ERROR));
        }
    };
    if let Err(error) = stdout.write_all(text.as_bytes()) {
        super::diagnose(
            stderr,
            &format!("the cached report could not be written: {error}"),
        )?;
        return Ok(Reuse::Answered(EXIT_ERROR));
    }
    Ok(Reuse::Answered(report.verdict().exit_code()))
}

fn diagnose_publication_status(
    stderr: &mut dyn Write,
    publication: &reports::IndexPublication,
) -> std::io::Result<()> {
    match publication {
        reports::IndexPublication::NotRequired | reports::IndexPublication::Complete => Ok(()),
        reports::IndexPublication::Incomplete { index, error } => super::diagnose(
            stderr,
            &format!(
                "the cached report was kept, but {} was not updated: {error}",
                index.file()
            ),
        ),
        reports::IndexPublication::RunChanged { error } => super::diagnose(
            stderr,
            &format!("the cached report was kept, but its sealed bytes changed: {error}"),
        ),
    }
}

/// How many run directories to keep.
const fn request_keep(request: &Request) -> u32 {
    request.config.reports.keep
}

/// The packages this run is about: the ones a reader named, or the ones the configuration names when they named none.
fn packages(arguments: &Verify, config: &Config) -> Vec<String> {
    run::asked_for(&arguments.packages, config)
}

/// The arguments every test binary of this run is started with.
fn harness_args(arguments: &Verify, config: &Config) -> Vec<String> {
    if arguments.test_args.is_empty() {
        config.execution.test_binary_args.clone()
    } else {
        arguments.test_args.clone()
    }
}

/// The configuration this run answers to.
fn load(arguments: &Verify, workspace: &reports::WorkspaceRoot) -> Result<LoadedConfig, LoadError> {
    match &arguments.config {
        Some(path) => {
            let text = reports::read_configuration(path)?;
            let config = Config::parse(&text, path)?;
            Ok(LoadedConfig {
                config,
                source: rust_mutants::id::slashed(path)?,
            })
        }
        None => {
            let loaded = workspace.load_config()?;
            Ok(LoadedConfig {
                config: loaded.config,
                source: match loaded.source {
                    reports::ConfigurationSource::Defaults => String::new(),
                    reports::ConfigurationSource::WorkspaceFile => {
                        crate::config::FILE_NAME.to_owned()
                    }
                },
            })
        }
    }
}

/// What a recording is named after: the run it belongs to.
struct Recording<'a> {
    root: &'a Path,
    identity: &'a RunId,
    contract: crate::config::Contract,
}

/// The directory under a runner trace that owns all configured-build engine traces.
pub use crate::app::trace::BUILDS_DIRECTORY;

/// A configured build ready to execute after every requested trace namespace has already been claimed.
struct ConfiguredBuild {
    build: rust_mutants::cargo::BuildConfig,
    selection: rust_mutants::cargo::BuildSelection,
    binding: rust_mutants::trace::NjutestBuild,
    trace: EngineTrace,
}

/// The only two pre-execution engine trace states.
/// A requested trace cannot degrade into the disabled state.
enum EngineTrace {
    Disabled,
    Requested {
        sink: rust_mutants::trace::DirSink,
        context: rust_mutants::trace::TraceContext,
    },
}

impl EngineTrace {
    fn start(self) -> rust_mutants::trace::Recorder {
        match self {
            Self::Disabled => rust_mutants::trace::Recorder::disabled(),
            Self::Requested { sink, context } => rust_mutants::trace::Recorder::wall(
                rust_mutants::trace::Sink::required(sink),
                context,
            ),
        }
    }
}

/// Captures every configured build and, when requested, exclusively creates every ordinal trace namespace before the first build starts.
fn configured_builds(
    request: &Request,
    establishing: &Establishing<'_>,
) -> Result<Vec<ConfiguredBuild>, TraceSetupError> {
    let mut configured = Vec::new();
    for (index, configuration) in std::iter::once(None)
        .chain(request.config.configuration.iter().map(Some))
        .enumerate()
    {
        let ordinal =
            u32::try_from(index).map_err(|_overflow| TraceSetupError::Ordinal { index })?;
        let name = configuration.map_or_else(
            || crate::config::DEFAULT_CONFIGURATION.to_owned(),
            |one| one.name.clone(),
        );
        let build = configuration.map_or_else(
            || request.build.clone(),
            crate::config::Configuration::build,
        );
        let selection = build.selection();
        let binding = rust_mutants::trace::NjutestBuild::new(
            establishing.identity.clone(),
            ordinal,
            name.clone(),
            &selection,
        )
        .map_err(|source| TraceSetupError::Binding { ordinal, source })?;
        configured.push(ConfiguredBuild {
            build,
            selection,
            binding,
            trace: EngineTrace::Disabled,
        });
    }

    if establishing.arguments.trace.is_none() {
        return Ok(configured);
    }
    let trace_root = trace_directory(
        establishing.arguments,
        establishing.root,
        establishing.identity,
    );
    let builds = trace_root.join(BUILDS_DIRECTORY);
    exclusive_directory(&builds).map_err(|source| TraceSetupError::Builds {
        path: builds.clone(),
        source,
    })?;
    for build in &mut configured {
        let ordinal = build.binding.ordinal();
        let namespace = builds.join(format!("{ordinal:010}"));
        exclusive_directory(&namespace).map_err(|source| TraceSetupError::Build {
            ordinal,
            path: namespace.clone(),
            source,
        })?;
        let engine = namespace.join(crate::app::trace::ENGINE_DIRECTORY);
        let sink = rust_mutants::trace::DirSink::create(&engine).map_err(|source| {
            TraceSetupError::Engine {
                ordinal,
                path: engine,
                source,
            }
        })?;
        build.trace = EngineTrace::Requested {
            sink,
            context: rust_mutants::trace::TraceContext::Njutest {
                build: build.binding.clone(),
            },
        };
    }
    Ok(configured)
}

fn exclusive_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    #[cfg(not(unix))]
    let builder = std::fs::DirBuilder::new();
    builder.create(path)
}

fn trace_directory(arguments: &Verify, root: &Path, identity: &RunId) -> PathBuf {
    match arguments.trace.as_deref() {
        Some("") | None => root.join(".njutest/trace").join(identity.as_str()),
        Some(requested) => PathBuf::from(requested),
    }
}

fn recorder(arguments: &Verify, run: &Recording<'_>) -> Result<Recorder, TraceSetupError> {
    let (root, identity, contract) = (run.root, run.identity, run.contract);
    let start = StartRecord::of(identity.as_str(), crate::report::RunKind::Full, contract);
    if arguments.trace.is_none() {
        return Ok(Recorder::wall(Sink::ring(), start));
    }
    let directory = trace_directory(arguments, root, identity);
    let sink = DirSink::create(&directory).map_err(|source| TraceSetupError::Outer {
        path: directory,
        source,
    })?;
    Ok(Recorder::wall(Sink::required_with_ring(sink), start))
}

#[cfg(test)]
mod tests {
    mod publication {
        use crate::report::across::BuildMeasurements;
        use crate::report::{BuildReport, LatticedDocument, Report, RunKind};
        use rust_mutants::cargo::BuildConfig;
        use rust_mutants::id::RunId;

        pub(super) fn complete_report(run: &str) -> Report {
            let mut draft = BuildReport::new(
                "fixture-evidence",
                RunKind::Full,
                crate::config::Contract::StandardV1,
            );
            draft.repository.root_name = "fixture".to_owned();
            draft.repository.workspace_digest = "a".repeat(64);
            draft.repository.configuration_digest = "b".repeat(64);
            draft.toolchain.rustc = "rustc 1.98.0".to_owned();
            draft.scope.configured_builds = vec![crate::config::DEFAULT_CONFIGURATION.to_owned()];
            draft.timing.started = "2026-01-01T00:00:00Z".to_owned();
            draft.timing.finished = "2026-01-01T00:00:00Z".to_owned();
            draft.limitations.push(crate::report::Limitation::new(
                "git-metadata-unavailable",
                "the fixture has no repository metadata",
            ));
            let measurements = BuildMeasurements::checked(vec![(
                crate::config::DEFAULT_CONFIGURATION.to_owned(),
                BuildConfig::default().selection(),
                draft,
            )])
            .expect("one complete configured-build measurement");
            let run = RunId::try_from(run).expect("canonical final run identity");
            let latticed = crate::report::across::configured(&run, &measurements)
                .expect("the fixture evidence forms a lattice");
            let LatticedDocument::Complete(latticed) = latticed else {
                panic!("a whole-catalog fixture cannot become a shard")
            };
            latticed
                .complete_without_models()
                .expect("the standard contract needs no model batch")
        }

        #[test]
        fn a_kept_report_says_the_directory_it_was_written_to() {
            let project = tempfile::tempdir().expect("temporary project");
            let store =
                crate::app::reports::Store::read(project.path()).expect("held report store");
            let report = complete_report("20260101t000000z-abacac");
            match store.keep(&report) {
                Ok(kept) => {
                    let document =
                        std::fs::read(kept.directory.join(crate::app::reports::DOCUMENT_NAME))
                            .expect(
                                "the directory a publication names holds the document it wrote",
                            );
                    assert!(
                        !document.is_empty(),
                        "a published report is the bytes at the directory it says it wrote to: {}",
                        kept.directory.display()
                    );
                }
                Err(refused) => assert!(
                    matches!(
                        refused,
                        crate::app::reports::StoreError::UnsupportedCapability
                    ),
                    "a platform that cannot publish says so rather than failing some other way: {refused:?}"
                ),
            }
        }

        #[cfg(unix)]
        #[test]
        fn every_path_a_run_says_it_wrote_is_a_file_it_wrote() {
            use crate::app::reports::Surface;

            let project = tempfile::tempdir().expect("temporary project");
            let store =
                crate::app::reports::Store::read(project.path()).expect("held report store");
            let kept = store
                .keep(&complete_report("20260101t000000z-abacac"))
                .expect("durable report publication");
            let records: Vec<&str> = kept.said.iter().map(|one| one.record).collect();
            assert_eq!(
                records,
                Surface::ALL.map(Surface::record).to_vec(),
                "a complete report is published with every surface a reader is pointed at"
            );
            for one in &kept.said {
                let metadata =
                    std::fs::metadata(project.path().join(&one.path)).unwrap_or_else(|error| {
                        panic!(
                            "{} names {}, and a script that uploads what it was told finds nothing \
                             there: {error}",
                            one.record, one.path
                        )
                    });
                assert!(
                    metadata.is_file(),
                    "{} names {}, which is not a file",
                    one.record,
                    one.path
                );
            }
        }

        #[cfg(unix)]
        #[test]
        fn json_output_uses_the_checked_value_after_the_report_path_is_replaced() {
            use super::super::said;
            use crate::cli::{Environment, Format};
            use crate::report::ReportDocument;
            use rust_mutants::id::StoredRunId;

            let project = tempfile::tempdir().expect("temporary project");
            let store =
                crate::app::reports::Store::read(project.path()).expect("held report store");
            let report = complete_report("20260101t000000z-abacac");
            let kept = store.keep(&report).expect("durable report publication");
            let published = kept.directory.clone();
            let report_root = project
                .path()
                .join(crate::config::DEFAULT_REPORTS_DIRECTORY);
            let held_root = project.path().join("held-report-root");
            std::fs::rename(&report_root, &held_root).expect("move the published report root");
            std::fs::create_dir_all(&published).expect("replacement run spelling");
            std::fs::write(
                published.join(crate::app::reports::DOCUMENT_NAME),
                b"forged replacement report\n",
            )
            .expect("replacement document");

            let environment = Environment {
                vars: Vec::new(),
                working_directory: project.path().to_path_buf(),
                temp_directory: project.path().join("tmp"),
                program: std::path::PathBuf::from("unused-test-program"),
                cache_directory: project.path().join("cache"),
                cancel: rust_mutants::runner::Cancel::new(),
                terminal: crate::presentation::Terminal::default(),
            };
            let document = ReportDocument::Complete(report.clone());
            let rendered = said(
                &document,
                project.path(),
                &crate::app::verify::Published {
                    document: kept.said_document.clone(),
                    said: kept.said,
                },
                (&environment, Some(Format::Json)),
            )
            .expect("in-memory JSON projection");
            let stored = store
                .open_run(&StoredRunId::from(
                    &RunId::try_from(report.run_id()).expect("report run identity"),
                ))
                .expect("the original run stays selected through the held root")
                .document()
                .expect("published original bytes");

            assert_eq!(
                rendered, stored,
                "stdout and published authority are one value"
            );
            assert_ne!(
                rendered, "forged replacement report\n",
                "a replacement workspace spelling cannot become stdout authority"
            );
        }
    }
}
