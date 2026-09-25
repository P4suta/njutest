// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands: what each one opens, what it establishes, and what it writes.

pub mod ci;
pub mod trace;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::EngineError;
use rust_mutants::id::RunId;
use rust_mutants::report::explain;
use rust_mutants::run::Expectation;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{self, Request, Session};
use rust_mutants::workspace::{self, Workspace};

use crate::error::CliError;
use crate::filesystem::{EntryKind, entry_kind};
use crate::report::html;
use crate::report::run as run_report;
use crate::report::stryker;
use crate::settings::Settings;
use crate::{Environment, cli, report, run};

mod bundle;
pub mod doctor;
pub mod estimate;
pub mod stored;
mod sweep;

use bundle::{Gathering, bundle};
use doctor::{Asked, doctor};
use stored::{named, newest, prune, store};

/// Does what the command asks and returns the exit code it earns.
///
/// # Errors
/// Returns what the command could not do.
pub fn dispatch(
    command: &cli::Command,
    composition: crate::Composition<'_>,
    streams: crate::Streams<'_>,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    let environment = composition.environment;
    let crate::Streams {
        out: stdout,
        err: stderr,
    } = streams;
    if !diagnoses(command) {
        reserved(environment, composition.compiled_catalog)?;
    }
    match command {
        cli::Command::Init { root, force } => init(root.as_deref(), *force, environment, stdout),
        cli::Command::Doctor {
            root,
            packages,
            json,
        } => doctor(
            &Asked {
                root: root.as_deref(),
                packages,
                json: *json,
            },
            environment,
            stdout,
            cancel,
        ),
        cli::Command::Explain {
            scope,
            mutant,
            run,
            fresh: false,
            json,
        } => stored_explain((scope, mutant, run.as_deref(), *json), environment, stdout),
        cli::Command::Report {
            root,
            run,
            format,
            output,
            tui,
        } => report_back(
            Wanted {
                root: root.as_deref(),
                run: run.as_deref(),
                format: *format,
                output: output.as_deref(),
                tui: *tui,
            },
            environment,
            stdout,
        ),
        cli::Command::List { .. }
        | cli::Command::Equivalence { .. }
        | cli::Command::Catalog { .. }
        | cli::Command::Run { .. }
        | cli::Command::Explain { .. }
        | cli::Command::Instrument { .. }
        | cli::Command::WhySkipped { .. }
        | cli::Command::Replay { .. }
        | cli::Command::Merge { .. }
        | cli::Command::Trace { .. }
        | cli::Command::Rules { .. }
        | cli::Command::Diagnostics { .. }
        | cli::Command::Cache { .. }
        | cli::Command::Ci { .. } => kept_command(
            command,
            environment,
            crate::Streams {
                out: stdout,
                err: stderr,
            },
            cancel,
        ),
    }
}

/// Every command that reads what earlier runs left rather than opening a tree of its own.
fn kept_command(
    command: &cli::Command,
    environment: &Environment,
    streams: crate::Streams<'_>,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    let crate::Streams {
        out: stdout,
        err: stderr,
    } = streams;
    match command {
        cli::Command::Cache {
            root,
            gc,
            all,
            kept,
            clear_outcomes,
            cache_dir,
        } => sweep::cache(
            &sweep::Sweeping {
                root: root.as_deref(),
                gc: *gc,
                all: *all,
                kept: *kept,
                clear_outcomes: *clear_outcomes,
                cache_dir: cache_dir.as_deref(),
            },
            environment,
            stdout,
        ),
        cli::Command::Merge {
            reports,
            root,
            runs,
            output,
        } => merge(
            &parts(reports, root.as_deref(), runs, environment)?,
            output.as_deref(),
            stdout,
        ),
        cli::Command::Trace { command } => trace::read(command, environment, stdout),
        cli::Command::Ci { command } => ci::dispatch(command, environment, stdout, cancel),
        cli::Command::Rules { tier, json } => rules(tier.as_deref(), *json, stdout),
        cli::Command::Diagnostics { run, root, output } => bundle(
            &Gathering {
                root: root.as_deref(),
                run: run.as_deref(),
                output: output.as_deref(),
            },
            environment,
            stdout,
            cancel,
        ),
        cli::Command::List { .. }
        | cli::Command::Equivalence { .. }
        | cli::Command::Catalog { .. }
        | cli::Command::Run { .. }
        | cli::Command::Explain { .. }
        | cli::Command::Instrument { .. }
        | cli::Command::WhySkipped { .. }
        | cli::Command::Init { .. }
        | cli::Command::Replay { .. }
        | cli::Command::Doctor { .. }
        | cli::Command::Report { .. } => workspace_command(
            command,
            environment,
            crate::Streams {
                out: stdout,
                err: stderr,
            },
            cancel,
        ),
    }
}

/// A run composes its own activation.
/// An inherited one would silently decide what every test process measures.
/// Whether the command is one whose whole job is to say what is wrong here.
const fn diagnoses(command: &cli::Command) -> bool {
    matches!(
        command,
        cli::Command::Doctor { .. } | cli::Command::Diagnostics { .. }
    )
}

/// The reserved variables this environment already names, in the order they are set.
#[must_use]
pub fn reserved_names(environment: &Environment) -> Vec<String> {
    environment
        .vars
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .filter_map(|(name, _)| {
            rust_mutants::execute::RESERVED_ENV
                .iter()
                .find(|reserved| name.as_os_str() == std::ffi::OsStr::new(reserved))
                .map(|reserved| (*reserved).to_owned())
        })
        .collect()
}

fn reserved(environment: &Environment, compiled_catalog: Option<&str>) -> Result<(), CliError> {
    if is_self_measurement(environment, compiled_catalog) {
        return Ok(());
    }
    reserved_names(environment).into_iter().next().map_or_else(
        || Ok(()),
        |name| Err(CliError::EnvironmentReserved { name }),
    )
}

/// Whether this binary belongs to exactly the catalog the inherited activation or touch run names.
#[must_use]
pub fn is_self_measurement(environment: &Environment, compiled_catalog: Option<&str>) -> bool {
    let value = |name: &str| {
        environment
            .vars
            .iter()
            .find(|(candidate, value)| candidate == name && !value.is_empty())
            .map(|(_, value)| value.as_os_str())
    };
    let Some(compiled) = compiled_catalog.filter(|catalog| !catalog.is_empty()) else {
        return false;
    };
    let Some(catalog) = value(rust_mutants::instrument::CATALOG_ENV) else {
        return false;
    };
    if catalog != std::ffi::OsStr::new(compiled) {
        return false;
    }
    let active = value(rust_mutants::instrument::ACTIVE_ENV).is_some();
    let touch = value(rust_mutants::instrument::TOUCH_ENV).is_some();
    active ^ touch
}

fn workspace_command(
    command: &cli::Command,
    environment: &Environment,
    streams: crate::Streams<'_>,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    const PROGRESS_EVENT_CAPACITY: usize = 4096;

    let crate::Streams {
        out: stdout,
        err: _stderr,
    } = streams;
    let Some(scope) = command.scope() else {
        return Ok(0);
    };
    let settings = Settings::resolve(scope, environment)?;
    let started = Timestamp::now();
    let id = named(command, started)?;
    let (sender, phases) = std::sync::mpsc::sync_channel(PROGRESS_EVENT_CAPACITY);
    let recorder = trace::recorder(
        &trace::Recording {
            scope,
            settings: &settings,
            id: &id,
            command,
        },
        watching(command).then_some(sender),
    )?;
    let outcome = measured(
        command,
        &Running {
            scope,
            settings: &settings,
            environment,
            id: &id,
            started,
            recorder: &recorder,
            phases: &phases,
        },
        stdout,
        cancel,
    );
    trace::ended(&recorder, &outcome, cancel)?;
    let pruned = if scope.trace.is_some() && !whole_run(command) {
        prune(&settings.report_directory(), settings.config.reports.keep)
    } else {
        Ok(())
    };
    match outcome {
        Err(error) => Err(error),
        Ok(code) => pruned.map(|()| code),
    }
}

/// Where a run may remember what measuring this tree established.
fn remembered_measurements(command: &cli::Command, environment: &Environment) -> Option<PathBuf> {
    if matches!(command, cli::Command::Run { no_cache: true, .. }) {
        return None;
    }
    Some(
        environment
            .cache_directory
            .join(rust_mutants::reach::remembered::LAYOUT),
    )
}

/// What a command says while it is preparing, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Displayed {
    /// Nothing: the command has no display, so preparing runs on the calling thread as before.
    Nothing,
    /// The lines a person reads.
    Lines,
    /// One object per line, for a program.
    Stream,
}

impl Displayed {
    /// What this command says while it prepares.
    const fn of(command: &cli::Command) -> Self {
        if streaming(command) {
            return Self::Stream;
        }
        if watching(command) {
            return Self::Lines;
        }
        Self::Nothing
    }
}

/// The sole owner of the scoped preparation worker.
struct PreparationThread<'scope>(
    std::thread::ScopedJoinHandle<'scope, Result<Session, EngineError>>,
);

impl<'scope> PreparationThread<'scope> {
    fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        work: impl FnOnce() -> Result<Session, EngineError> + Send + 'scope,
    ) -> Result<Self, CliError> {
        std::thread::Builder::new()
            .name("rust-mutants-prepare".to_owned())
            .spawn_scoped(scope, work)
            .map(Self)
            .map_err(|source| CliError::PreparationStartFailed { source })
    }

    fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    fn join(self) -> Result<Session, CliError> {
        let prepared = self
            .0
            .join()
            .map_err(|_panic| CliError::PreparationPanicked)?;
        prepared.map_err(CliError::from)
    }
}

/// Prepares the tree, saying what it is doing while it does it.
fn preparing(
    workspace: Workspace,
    options: &session::PrepareOptions,
    displayed: Displayed,
    watching: (
        &std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
        &mut dyn Write,
        &Cancel,
    ),
) -> Result<Session, CliError> {
    let (phases, stdout, cancel) = watching;
    if displayed == Displayed::Nothing {
        return Ok(workspace.prepare(options, cancel)?);
    }
    std::thread::scope(|scope| {
        let working = PreparationThread::launch(scope, move || workspace.prepare(options, cancel))?;
        let alive = || !working.is_finished();
        let watched = match displayed {
            Displayed::Lines => crate::ui::watch(phases, stdout, &alive),
            Displayed::Stream => crate::stream::watch(phases, stdout, &alive),
            Displayed::Nothing => Ok(()),
        };
        let prepared = working.join();
        watched?;
        prepared
    })
}

/// Whether this command writes the run as a stream, which opens before anything is prepared.
const fn streaming(command: &cli::Command) -> bool {
    matches!(
        command,
        cli::Command::Run {
            json: true,
            dry_run: false,
            mutant: None,
            ..
        }
    )
}

/// Whether the command's successful result is a whole-run document whose pending terminal state already closes, prunes, and then publishes itself.
const fn whole_run(command: &cli::Command) -> bool {
    matches!(
        command,
        cli::Command::Run {
            dry_run: false,
            mutant: None,
            ..
        }
    )
}

/// Whether this command has a progress display that wants the phases as they end.
const fn watching(command: &cli::Command) -> bool {
    matches!(
        command,
        cli::Command::Run {
            ui: crate::ui::Ui::Auto | crate::ui::Ui::Plain,
            ..
        } | cli::Command::Run { json: true, .. }
    )
}

/// What every test binary of this run is started with: what a person typed after `--`, or what the file holds when they typed nothing.
fn harness(command: &cli::Command, options: &mut session::PrepareOptions) {
    if let cli::Command::Run { args, .. } = command
        && !args.is_empty()
    {
        options.harness_args.clone_from(args);
    }
}

/// Everything a workspace command needs beyond what it prints.
struct Running<'a> {
    scope: &'a cli::Scope,
    settings: &'a Settings,
    environment: &'a Environment,
    id: &'a RunId,
    started: Timestamp,
    recorder: &'a rust_mutants::trace::Recorder,
    /// What the recorder has said about the phases it has finished, for a display to write.
    phases: &'a std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
}

fn preparation_options(
    command: &cli::Command,
    running: &Running<'_>,
    changed: Option<Vec<rust_mutants::glob::Pattern>>,
) -> Result<(session::PrepareOptions, Option<run::Filter>), CliError> {
    let mut options = running.settings.prepare_options()?;
    options.measurements = remembered_measurements(command, running.environment);
    harness(command, &mut options);
    if let Some(changed) = changed {
        options.include = changed;
    }
    let validation_filter = validation_filter(command, running.settings)?;
    options.validation_filter.clone_from(&validation_filter);
    Ok((options, validation_filter))
}

#[expect(
    clippy::too_many_lines,
    reason = "the length is the size of a closed set named in full, not a function doing \
              several things: a command added to the engine is a question the compiler puts \
              to these arms rather than a line that silently inherits somebody else's answer"
)]
fn measured(
    command: &cli::Command,
    running: &Running<'_>,
    stdout: &mut dyn Write,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    let Running {
        scope,
        settings,
        environment,
        id,
        started,
        recorder,
        phases,
    } = *running;
    let changed = match base_of(scope) {
        None => None,
        Some(base) => match selected(running, base, cancel)? {
            rust_mutants::git::Within::Changed(patterns) => Some(patterns),
            rust_mutants::git::Within::Nothing { changed } => {
                return nothing_changed(base, &changed, stdout);
            }
        },
    };
    let open = settings.open_options(scope, environment, recorder.clone())?;
    let workspace = Workspace::open(&settings.root, open.clone(), cancel)?;
    let (options, validation_filter) = preparation_options(command, running, changed)?;
    match command {
        cli::Command::Equivalence { limit, .. } => {
            let discovery = session::preview(&workspace, &options, cancel)?;
            let root = settings.root.clone();
            let open = settings.open_options(scope, environment, recorder.clone())?;
            workspace.close()?;
            let cataloged = discovery.catalog.mutants().len();
            let said = equivalence(
                &Asking {
                    root: &root,
                    open,
                    discovery: &discovery,
                    limit: *limit,
                    trace: recorder,
                },
                cancel,
            )?;
            write(stdout, &rendered(&said, cataloged)?)?;
            Ok(0)
        }
        cli::Command::List { .. }
        | cli::Command::WhySkipped { .. }
        | cli::Command::Instrument { .. } => {
            let discovery = session::preview(&workspace, &options, cancel)?;
            write(stdout, &previewed(command, &workspace, &discovery)?)?;
            workspace.close()?;
            Ok(0)
        }
        cli::Command::Catalog { .. }
        | cli::Command::Run { .. }
        | cli::Command::Explain { .. }
        | cli::Command::Init { .. }
        | cli::Command::Replay { .. }
        | cli::Command::Doctor { .. }
        | cli::Command::Merge { .. }
        | cli::Command::Trace { .. }
        | cli::Command::Report { .. }
        | cli::Command::Rules { .. }
        | cli::Command::Diagnostics { .. }
        | cli::Command::Cache { .. }
        | cli::Command::Ci { .. } => {
            let streams = streaming(command);
            if streams {
                crate::stream::started(
                    stdout,
                    id.as_str(),
                    &root_name(settings)?,
                    report::selection_document(&options),
                )?;
            }
            let session = match preparing(
                workspace,
                &options,
                Displayed::of(command),
                (phases, stdout, cancel),
            ) {
                Ok(session) => session,
                Err(error) => {
                    if streams {
                        crate::stream::failed(stdout, &error)?;
                    }
                    return Err(error);
                }
            };
            let prepared = prepared(
                command,
                &Prepared {
                    session: &session,
                    settings,
                    open: &open,
                    environment,
                    id,
                    started,
                    phases,
                    filter: validation_filter.as_ref(),
                },
                cancel,
                stdout,
            );
            let outcome = match session.close() {
                Ok(kept) => remember(settings, id, &kept, recorder).and(prepared),
                Err(error) => Err(error.into()),
            };
            let outcome = outcome.and_then(|prepared| prepared.finish(settings, stdout));
            if streams && let Err(error) = &outcome {
                crate::stream::failed(stdout, error)?;
            }
            outcome
        }
    }
}

/// Writes down what a run kept, so a later command can find it and a later sweep can leave it alone.
fn remember(
    settings: &Settings,
    run_id: &RunId,
    kept: &[PathBuf],
    recorder: &rust_mutants::trace::Recorder,
) -> Result<(), CliError> {
    if kept.is_empty() {
        return Ok(());
    }
    for path in kept {
        recorder.kept(rust_mutants::trace::KeptRecord {
            path: path.display().to_string(),
            run_id: run_id.to_string(),
        });
    }
    crate::kept::Ledger::record(&settings.report_directory(), run_id.as_str(), kept)
        .map(|_ledger| ())
        .map_err(|source| CliError::writing(&settings.report_directory(), source))
}

/// The revision a change set is computed against, when the command line asked for one at all.
fn base_of(scope: &cli::Scope) -> Option<&str> {
    scope
        .changed_from
        .as_deref()
        .or_else(|| scope.changed.then_some(rust_mutants::git::DEFAULT_BASE))
}

/// What a command answers when the change from `base` touches no file the configuration measures: that, and what did change, with nothing opened or built.
fn nothing_changed(base: &str, changed: &[String], stdout: &mut dyn Write) -> Result<u8, CliError> {
    let mut said = format!(
        "NOTHING   the change from {base} touches no Rust file this configuration measures\n"
    );
    if changed.is_empty() {
        said.push_str("          nothing changed at all\n");
    } else {
        said.push_str("          changed instead: ");
        said.push_str(&changed.join(", "));
        said.push('\n');
    }
    write(stdout, &said)?;
    Ok(run::EXIT_DETECTED)
}

/// What a change set selects, narrowing what the configuration already selected, or that it selects nothing.
fn selected(
    running: &Running<'_>,
    base: &str,
    cancel: &Cancel,
) -> Result<rust_mutants::git::Within, CliError> {
    let Running {
        settings,
        environment,
        recorder,
        ..
    } = *running;
    let report_directory = path_text(
        &settings.config.reports.directory,
        "the report directory has no exact UTF-8 spelling for change selection",
    )?;
    let excluded = [report_directory.as_str(), "target"];
    let watch = rust_mutants::runner::Watched::new(cancel, recorder);
    let asking = rust_mutants::git::Asking {
        root: &settings.root,
        env: &environment.vars,
        excluded: &excluded,
        watch: &watch,
    };
    let change = rust_mutants::git::changed(&asking, base).ok_or_else(|| {
        CliError::ChangeSetUnavailable {
            root: settings.root.clone(),
            base: base.to_owned(),
        }
    })?;
    Ok(
        rust_mutants::git::within(&change, &settings.prepare_options()?.include)
            .map_err(EngineError::from)?,
    )
}

/// What a command that only needs discovery prints.
/// Refuses a `--file` that names no file the walk considered.
///
/// # Errors
/// [`CliError::InvalidValue`] naming the path and how many files there are.
fn narrowed(considered: &[String], named: &[String]) -> Result<(), CliError> {
    let missing: Vec<&String> = named
        .iter()
        .filter(|one| {
            let path = one
                .rsplit_once(':')
                .map_or(one.as_str(), |(head, _lines)| head);
            !considered.iter().any(|held| held == path)
        })
        .collect();
    let Some(first) = missing.first() else {
        return Ok(());
    };
    let suggested = nearest(first, considered)?;
    Err(CliError::InvalidValue {
        flag: "--file".to_owned(),
        value: missing
            .iter()
            .map(|one| one.as_str())
            .collect::<Vec<&str>>()
            .join(", "),
        expected: format!(
            "a workspace-relative path of one of the {} files this run reads. Nearest to \
             {first:?}: {}",
            considered.len(),
            suggested
        ),
    })
}

/// The three names most like `named`, so a typo is answered with what was meant.
///
/// A refusal that says "not one of the four hundred files this run reads" and stops has told somebody they are wrong and left them to find out how.
/// The names are in hand.
fn nearest(named: &str, considered: &[String]) -> Result<String, CliError> {
    let mut ranked = Vec::with_capacity(considered.len());
    for held in considered {
        ranked.push((distance(named, held)?, held));
    }
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    Ok(ranked
        .into_iter()
        .take(3)
        .map(|(_at, held)| held.as_str())
        .collect::<Vec<&str>>()
        .join(", "))
}

/// How far apart two names are, counting the characters they do not share.
fn distance(left: &str, right: &str) -> Result<usize, CliError> {
    let shared = left
        .chars()
        .zip(right.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let left = left
        .chars()
        .count()
        .checked_sub(shared)
        .ok_or(CliError::ProjectionOverflow {
            projection: "file suggestion",
            field: "the unmatched character count",
        })?;
    let right = right
        .chars()
        .count()
        .checked_sub(shared)
        .ok_or(CliError::ProjectionOverflow {
            projection: "file suggestion",
            field: "the unmatched character count",
        })?;
    left.checked_add(right).ok_or(CliError::ProjectionOverflow {
        projection: "file suggestion",
        field: "the total unmatched character count",
    })
}

fn previewed(
    command: &cli::Command,
    workspace: &Workspace,
    discovery: &rust_mutants::discover::Discovery,
) -> Result<String, CliError> {
    let considered: Vec<String> = discovery
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    match command {
        cli::Command::List { file, json, .. } => {
            narrowed(&considered, file.as_slice())?;
            let sources = read_sources(workspace.snapshot_root(), discovery)?;
            if *json {
                return json_line(&report::candidates(discovery, &sources, file.as_deref())?);
            }
            Ok(report::list(discovery, &sources, file.as_deref())?)
        }
        cli::Command::WhySkipped { file, line, .. } => {
            narrowed(&considered, file.as_slice())?;
            match file {
                Some(path) => Ok(report::decisions(discovery, path, *line)),
                None => report::why_skipped(&discovery.skips),
            }
        }
        cli::Command::Instrument { file, mutant, .. } => {
            narrowed(&considered, std::slice::from_ref(file))?;
            instrumented(workspace, discovery, (file, mutant.as_deref()))
        }
        cli::Command::Equivalence { .. }
        | cli::Command::Catalog { .. }
        | cli::Command::Run { .. }
        | cli::Command::Explain { .. }
        | cli::Command::Init { .. }
        | cli::Command::Replay { .. }
        | cli::Command::Doctor { .. }
        | cli::Command::Merge { .. }
        | cli::Command::Trace { .. }
        | cli::Command::Report { .. }
        | cli::Command::Rules { .. }
        | cli::Command::Diagnostics { .. }
        | cli::Command::Cache { .. }
        | cli::Command::Ci { .. } => Ok(String::new()),
    }
}

/// A prepared session and the configuration it was prepared from.
struct Prepared<'a> {
    session: &'a Session,
    settings: &'a Settings,
    /// How the workspace was opened, so a layer that opens a tree of its own does it the same way.
    open: &'a workspace::OpenOptions,
    environment: &'a Environment,
    id: &'a RunId,
    started: Timestamp,
    /// What the recorder has said about the phases it has finished, for a display to write.
    phases: &'a std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
    /// The run selection compiled into the instrumented tree, evaluated once before preparation so `--from-report` cannot move underneath it.
    filter: Option<&'a run::Filter>,
}

/// What remains after a prepared command has stopped using its session.
///
/// A whole run cannot publish its terminal line until closing the session and pruning its store have both succeeded.
/// Keeping that terminal document in a distinct state makes publishing it early impossible at the call site.
#[derive(Debug)]
enum PreparedOutcome {
    /// No terminal run document remains to be published.
    Complete(Box<CompletedCommand>),
    /// A whole run measured everything, but has not yet published its terminal line.
    RunPending(Box<PendingRun>),
}

#[derive(Debug)]
struct CompletedCommand {
    code: u8,
}

#[derive(Debug)]
struct PendingRun {
    document: run_report::RunDocument,
    written: Option<PathBuf>,
    json: bool,
}

impl PreparedOutcome {
    fn complete(code: u8) -> Self {
        Self::Complete(Box::new(CompletedCommand { code }))
    }

    /// Performs the work that must follow a successfully closed session, then publishes the one terminal answer.
    fn finish(self, settings: &Settings, stdout: &mut dyn Write) -> Result<u8, CliError> {
        match self {
            Self::Complete(command) => Ok(command.code),
            Self::RunPending(run) => {
                prune(&settings.report_directory(), settings.config.reports.keep)?;
                concluded(&run.document, (run.json, run.written.as_deref()), stdout)?;
                Ok(run.document.run.exit_code)
            }
        }
    }
}

fn request(
    mutant: &str,
    target: Option<&String>,
    test: Option<&String>,
    args: &[String],
) -> Request {
    let mut request = Request::new(mutant.to_owned())
        .test(test.cloned())
        .with_args(args.to_vec());
    if let Some(target) = target {
        request = request.with_target(target.clone());
    }
    request
}

/// What a command that needs a prepared session does.
#[expect(
    clippy::too_many_lines,
    reason = "the length is the size of a closed set named in full, not a function doing \
              several things: a command added to the engine is a question the compiler puts \
              to these arms rather than a line that silently inherits somebody else's answer"
)]
fn prepared(
    command: &cli::Command,
    prepared: &Prepared<'_>,
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<PreparedOutcome, CliError> {
    let Prepared {
        session,
        settings,
        filter: prepared_filter,
        ..
    } = *prepared;
    match command {
        cli::Command::Catalog {
            json, rejections, ..
        } => {
            let text = if *json {
                json_line(&report::document(session, &settings.prepare_options()?)?)?
            } else if *rejections {
                report::rejections(session)
            } else {
                report::catalog(session)
            };
            write(stdout, &text)?;
            Ok(PreparedOutcome::complete(0))
        }
        cli::Command::Explain { mutant, json, .. } => {
            fresh_explain(prepared, mutant, *json, stdout).map(PreparedOutcome::complete)
        }
        cli::Command::Replay { mutant, run, .. } => {
            replay(prepared, (mutant, run.as_deref()), cancel, stdout)
                .map(PreparedOutcome::complete)
        }
        cli::Command::Run {
            mutant,
            target,
            test,
            shard,
            no_report,
            no_cache,
            ui,
            json,
            fail_fast,
            dry_run,
            args,
            ..
        } => match mutant {
            Some(prefix) => one(
                session,
                &request(prefix, target.as_ref(), test.as_ref(), args),
                cancel,
                stdout,
            )
            .map(PreparedOutcome::complete),
            None => whole(
                session,
                &Whole {
                    settings,
                    open: prepared.open,
                    args,
                    shard: shard.as_deref(),
                    asked: Switches {
                        no_report: *no_report,
                        no_cache: *no_cache,
                        ui: *ui,
                        json: *json,
                        fail_fast: *fail_fast,
                        dry_run: *dry_run,
                    },
                    filter: filter(
                        command,
                        prepared.settings,
                        session,
                        prepared_filter.cloned().unwrap_or_default(),
                    )?,
                    phases: prepared.phases,
                    environment: prepared.environment,
                    id: prepared.id,
                    started: prepared.started,
                },
                cancel,
                stdout,
            ),
        },
        cli::Command::List { .. }
        | cli::Command::Equivalence { .. }
        | cli::Command::Instrument { .. }
        | cli::Command::WhySkipped { .. }
        | cli::Command::Init { .. }
        | cli::Command::Doctor { .. }
        | cli::Command::Merge { .. }
        | cli::Command::Trace { .. }
        | cli::Command::Report { .. }
        | cli::Command::Rules { .. }
        | cli::Command::Diagnostics { .. }
        | cli::Command::Cache { .. }
        | cli::Command::Ci { .. } => Ok(PreparedOutcome::complete(0)),
    }
}

/// One named mutant, run on its own.
fn one(
    session: &Session,
    request: &Request,
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let found = session.resolve(&request.mutant)?.clone();
    let result = session.exec(request, cancel)?;
    write(stdout, &report::outcome(&result, &found))?;
    Ok(report::exit_code(result.outcome()))
}

/// Every accepted mutant, with the expectations verified and a report written.
/// Everything a whole run needs beyond the session.
struct Whole<'a> {
    settings: &'a Settings,
    /// How the workspace was opened, so the equivalence layer can open a tree of its own the same way.
    open: &'a workspace::OpenOptions,
    args: &'a [String],
    shard: Option<&'a str>,
    /// The switches the command line set, which say what the run does rather than what it measures.
    asked: Switches,
    /// Which of the catalog's mutants this run is about.
    filter: run::Filter,
    /// What the recorder has said about the phases it has finished, for a display to write.
    phases: &'a std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
    environment: &'a Environment,
    id: &'a RunId,
    started: Timestamp,
}

/// The switches a run was asked for.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one switch a person sets on the command line, and a switch is a bool \
              wherever it is stored"
)]
struct Switches {
    /// Write no run report under the report directory.
    no_report: bool,
    /// Execute every mutant afresh rather than reading back what an earlier run established.
    no_cache: bool,
    /// How much the run says while it is happening.
    ui: crate::ui::Ui,
    /// Whether the run is written as a stream a program reads rather than as lines a person does.
    json: bool,
    /// Whether the run stops at the first thing a reader has to act on.
    fail_fast: bool,
    /// Whether the run says what it would cost rather than paying it.
    dry_run: bool,
}

fn whole(
    session: &Session,
    whole: &Whole<'_>,
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<PreparedOutcome, CliError> {
    let Whole {
        settings,
        open,
        args,
        shard,
        asked:
            Switches {
                no_report,
                no_cache,
                ui,
                json,
                fail_fast,
                dry_run,
            },
        ref filter,
        phases,
        environment,
        id,
        started,
    } = *whole;
    let shard = shard.map(run::Shard::parse).transpose()?;
    let asking = asking_equivalence(settings, open);
    let outcomes = crate::outcomes::Store::new(&environment.cache_directory);
    let (keyed, expectations) = (keyed(session, whole), expectations(settings));
    let selection = report::selection_document(&settings.prepare_options()?);
    let options = run::Options {
        quiet: &run::Quiet::default(),
        equivalence: asking.as_ref(),
        jobs: settings.config.execution.jobs,
        expectations: &expectations,
        args,
        shard,
        outcomes: (!no_cache).then_some(run::Reusing {
            store: &outcomes,
            keyed: &keyed,
            run_id: id.as_str(),
        }),
        filter: Some(filter),
        fail_fast,
    };
    if dry_run {
        write(stdout, &crate::ui::phases(phases))?;
        write(stdout, &estimate::estimate(session, filter)?)?;
        return Ok(PreparedOutcome::complete(0));
    }
    let mut result = measured_run(
        session,
        &Watched {
            options: &options,
            settings,
            phases,
            json,
            ui,
            paints: environment.paints,
        },
        cancel,
        stdout,
    )?;
    result.expectations =
        run::verify(session, &expectations, &mut result.judged).map_err(EngineError::from)?;
    let document = run_report::document(
        session,
        &result,
        selection,
        &run_report::Meta {
            id: id.as_str(),
            started_at: started,
            finished_at: Timestamp::now(),
        },
    )?;
    let written = if no_report {
        None
    } else {
        Some(stored_with_evidence(session, settings, id, &document)?)
    };
    Ok(PreparedOutcome::RunPending(Box::new(PendingRun {
        document,
        written,
        json,
    })))
}

/// What a finished run says: the stream's last lines, or the summary a person reads.
fn concluded(
    document: &run_report::RunDocument,
    (json, written): (bool, Option<&Path>),
    stdout: &mut dyn Write,
) -> Result<(), CliError> {
    if json {
        let report = written
            .map(|path| path_text(path, "the stored report path has no exact UTF-8 spelling"))
            .transpose()?;
        return crate::stream::ended(stdout, document, report);
    }
    write(stdout, "\n")?;
    write(stdout, &report::lines(document)?)?;
    if let Some(path) = written {
        let mut line = String::new();
        let spelling = path_text(path, "the stored report path has no exact UTF-8 spelling")?;
        let ok = writeln!(line, "REPORT    {spelling}");
        debug_assert!(ok.is_ok(), "writing to a String cannot fail");
        write(stdout, &line)?;
    }
    write(stdout, &onward(document))?;
    Ok(())
}

/// Where a reader goes from a wall of findings, which is the next thing they want and the one thing the run does not say.
///
/// A survivor is a decision to make, not a fact to file: either the tests have a gap or the code has a claim in it somebody should write down.
/// `explain` is where both are answered — it prints what reached the mutation, and the block that records a reason — and nothing on the way there named it.
fn onward(document: &run_report::RunDocument) -> String {
    let Some(first) = document
        .mutants
        .iter()
        .find(|one| one.outcome == rust_mutants::outcome::Outcome::Survived && !one.expected)
    else {
        return String::new();
    };
    format!(
        "NEXT      rust-mutants explain {}:{}:{}@{}\n          says which tests reached one \
         of these, and writes the block that records a reason for it\n",
        first.path, first.item, first.rule, first.line
    )
}

/// Everything the run itself needs beyond the session, so a caller chooses one display and hands it over.
struct Watched<'a> {
    options: &'a run::Options<'a>,
    settings: &'a Settings,
    phases: &'a std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
    json: bool,
    ui: crate::ui::Ui,
    paints: bool,
}

/// The run, watched by whichever display the command line asked for.
fn measured_run(
    session: &Session,
    watched: &Watched<'_>,
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<run::Run, CliError> {
    if watched.json {
        let mut writer = crate::stream::Writer::new(stdout, session);
        writer.phases(watched.phases);
        let result = run::run(session, watched.options, cancel, &mut writer);
        writer.finish()?;
        return Ok(result?);
    }
    write(stdout, &crate::ui::phases(watched.phases))?;
    let mut display = crate::ui::Display::new(
        stdout,
        resolved(watched.ui),
        watched.paints,
        run::jobs(watched.settings.config.execution.jobs),
    );
    let result = run::run(session, watched.options, cancel, &mut display);
    display.finish()?;
    Ok(result?)
}

/// Everything beyond a mutant's own identity that a stored outcome is keyed on.
fn keyed(session: &Session, whole: &Whole<'_>) -> crate::outcomes::Keyed {
    let (settings, args) = (whole.settings, whole.args);
    let engine = match crate::outcomes::engine_of(&whole.environment.program) {
        Ok(engine) => engine,
        Err(_unreadable_so_nothing_is_remembered) => String::new(),
    };
    crate::outcomes::Keyed {
        closure: session.closure().to_owned(),
        manifests: session.manifests().to_owned(),
        toolchain: format!(
            "{} {} {}",
            session.toolchain().cargo_version().summary,
            session.toolchain().rustc_version().summary,
            session.toolchain().host()
        ),
        args: args.to_vec(),
        timeout: crate::config::render_timeout(settings.config.mutation.timeout),
        steps: settings.config.mutation.steps,
        build: settings.config.build.config().arguments(),
        engine,
    }
}

/// Which of the catalog's mutants a run was asked for, from the flags that narrow it.
///
/// # Errors
/// A `--file` whose lines are not a range.
/// Refuses a rule or family name this release does not know.
///
/// A name that names nothing narrows a run to nothing and the run reports that nothing was missed, or widens a skip to nothing and the rule a person meant to pass over runs anyway.
/// Both are answers they cannot tell from the ones they asked for, and the set of names is compiled into the release, so nothing has to be built to say which it is.
///
/// # Errors
/// [`CliError::InvalidValue`] naming the flag, the value, and where the names are.
fn known(flag: &str, named: &[String], rules: bool) -> Result<(), CliError> {
    let registry = rust_mutants::rule::Registry::canonical();
    for one in named {
        let held = if rules {
            registry.lookup(one).is_some()
        } else {
            rust_mutants::rule::Family::ALL
                .iter()
                .any(|family| family.name() == one)
        };
        if !held {
            return Err(CliError::InvalidValue {
                flag: flag.to_owned(),
                value: one.clone(),
                expected: format!(
                    "one of the {} this release knows, which `rust-mutants rules` lists",
                    if rules { "operators" } else { "families" }
                ),
            });
        }
    }
    Ok(())
}

fn filter(
    command: &cli::Command,
    settings: &Settings,
    session: &Session,
    filter: run::Filter,
) -> Result<run::Filter, CliError> {
    let cli::Command::Run { files, ids, .. } = command else {
        return Ok(run::Filter::default());
    };
    let removed_a_file = session
        .files()
        .iter()
        .any(|file| file.whole_file == Some(rust_mutants::syntax::SkipReason::Excluded));
    if removed_a_file && session.catalog().mutants().is_empty() {
        return Err(CliError::InvalidValue {
            flag: "--include/--exclude".to_owned(),
            value: format!(
                "include {:?}, exclude {:?}",
                settings.config.project.include, settings.config.project.exclude
            ),
            expected: "patterns that leave at least one file to read; a selection that \
                       leaves none measures nothing and scores as though nothing was \
                       missed"
                .to_owned(),
        });
    }
    for prefix in ids {
        if !session.catalog().mutants().iter().any(|mutant| {
            mutant.id.as_str().starts_with(prefix) || mutant.display_id.as_str().starts_with(prefix)
        }) {
            return Err(CliError::InvalidValue {
                flag: "--id".to_owned(),
                value: prefix.clone(),
                expected: format!(
                    "a prefix of one of the {} mutations this catalog holds",
                    session.catalog().mutants().len()
                ),
            });
        }
    }
    narrowed(
        &session
            .files()
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<String>>(),
        files,
    )?;
    Ok(filter)
}

/// Compiles the run's syntactic selection before the expensive compiler validation begins.
/// Existence checks still happen against the complete session catalog afterwards; this step only gives preparation the same predicate the run will use.
fn filter_before_preparation(
    command: &cli::Command,
    settings: &Settings,
) -> Result<run::Filter, CliError> {
    let cli::Command::Run {
        rules,
        families,
        skip_rules,
        skip_families,
        files,
        ids,
        from_report,
        outcome,
        ..
    } = command
    else {
        return Ok(run::Filter::default());
    };
    known("--rule", rules, true)?;
    known("--skip-rule", skip_rules, true)?;
    known("--family", families, false)?;
    known("--skip-family", skip_families, false)?;
    let mut named_ids = (!ids.is_empty()).then(|| ids.clone());
    if let Some(named) = from_report {
        named_ids
            .get_or_insert_with(Vec::new)
            .extend(stored_outcomes(settings, named, outcome)?);
    }
    Ok(run::Filter {
        rules: rules.clone(),
        families: families.clone(),
        skip_rules: skip_rules.clone(),
        skip_families: skip_families.clone(),
        files: files
            .iter()
            .map(|one| addressed(one))
            .collect::<Result<Vec<_>, _>>()?,
        ids: named_ids,
    })
}

/// Which candidates a run already knows it can leave out before it compiles the instrumented tree.
/// Shards deliberately stay out of this predicate: each shard report currently carries the shared validation result, so that result must remain identical across all parts until merge records a partitioned validation proof of its own.
fn validation_filter(
    command: &cli::Command,
    settings: &Settings,
) -> Result<Option<run::Filter>, CliError> {
    match command {
        cli::Command::Run {
            mutant: Some(prefix),
            ..
        } => Ok(Some(run::Filter {
            ids: Some(vec![prefix.clone()]),
            ..run::Filter::default()
        })),
        cli::Command::Run { mutant: None, .. } => {
            Ok(Some(filter_before_preparation(command, settings)?))
        }
        cli::Command::List { .. }
        | cli::Command::Equivalence { .. }
        | cli::Command::Catalog { .. }
        | cli::Command::Explain { .. }
        | cli::Command::Instrument { .. }
        | cli::Command::WhySkipped { .. }
        | cli::Command::Init { .. }
        | cli::Command::Replay { .. }
        | cli::Command::Doctor { .. }
        | cli::Command::Merge { .. }
        | cli::Command::Trace { .. }
        | cli::Command::Report { .. }
        | cli::Command::Rules { .. }
        | cli::Command::Diagnostics { .. }
        | cli::Command::Cache { .. }
        | cli::Command::Ci { .. } => Ok(None),
    }
}

/// The mutants a stored run left with `outcome`, by identity.
///
/// # Errors
/// [`CliError::ReportMissing`] when there is no such run to read.
fn stored_outcomes(
    settings: &Settings,
    named: &str,
    outcome: &str,
) -> Result<Vec<String>, CliError> {
    let directory = settings.report_directory();
    let path = if named.is_empty() {
        newest(&directory)?
    } else {
        directory.join(named).join(run_report::FILE_NAME)
    };
    let document = read_run_document(&path)?;
    Ok(document
        .mutants
        .into_iter()
        .filter(|one| one.outcome.as_str() == outcome)
        .map(|one| one.id)
        .collect())
}

/// One `--file` value: a path, and the lines of it the run is about.
///
/// # Errors
/// [`CliError::InvalidValue`] for lines that are not a range.
pub fn addressed(text: &str) -> Result<(String, Option<(u32, u32)>), CliError> {
    let Some((path, lines)) = text.rsplit_once(':') else {
        return Ok((text.to_owned(), None));
    };
    let refuse = || CliError::InvalidValue {
        flag: "--file".to_owned(),
        value: text.to_owned(),
        expected: "PATH, PATH:LINE, or PATH:FROM-TO".to_owned(),
    };
    let (from, to) = lines.split_once('-').unwrap_or((lines, lines));
    let from: u32 = from.parse::<u32>().map_err(|_error| refuse())?;
    let to: u32 = to.parse::<u32>().map_err(|_error| refuse())?;
    if from == 0 || to < from {
        return Err(refuse());
    }
    Ok((path.to_owned(), Some((from, to))))
}

/// One finding, put back to the tests exactly as the run that found it did.
fn replay(
    prepared: &Prepared<'_>,
    asked: (&str, Option<&str>),
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let (prefix, run) = asked;
    let session = prepared.session;
    let found = session.resolve(prefix)?.clone();
    let stored = recorded(prepared.settings, run, &found)?;
    let mut request = Request::new(found.display_id.to_string());
    if let Some(row) = &stored {
        if !row.target.is_empty() {
            request = request.with_target(row.target.clone());
        }
        if let Some(test) = row.killed_by.first() {
            request = request.test(Some(test.clone()));
        }
    }
    let result = session.exec(&request, cancel)?;
    write(
        stdout,
        &format!(
            "REPLAY    {} {}\n",
            found.display_id,
            verdict(stored.as_ref(), result.outcome().name())
        ),
    )?;
    write(stdout, &report::outcome(&result, &found))?;
    Ok(report::exit_code(result.outcome()))
}

/// What the replay establishes about the stored answer.
fn verdict(stored: Option<&run_report::RunMutantDocument>, now: &str) -> String {
    let Some(row) = stored else {
        return format!("was nothing, now {now}");
    };
    let survived = rust_mutants::outcome::Outcome::Survived.name();
    if row.not_run_reason == Some(run::NotRunReason::Discharged) {
        return if now == survived {
            format!("{now}, which is the proof that discharged it holding")
        } else {
            format!("{now}, and a proof discharged it: the proof is wrong")
        };
    }
    if row.outcome.as_str() == now {
        format!("still {now}")
    } else {
        format!("was {}, now {now}", row.outcome)
    }
}

/// What a stored run said about one mutant, when a stored run said anything.
///
/// # Errors
/// [`CliError::ReportMissing`] when `run` names no stored run, or when the report a name resolves to cannot be read as one.
fn recorded(
    settings: &Settings,
    run: Option<&str>,
    found: &rust_mutants::catalog::Mutant,
) -> Result<Option<run_report::RunMutantDocument>, CliError> {
    let directory = settings.report_directory();
    let path = match stored::report_of(&directory, run) {
        Ok(path) => path,
        Err(refusal) if run.is_some() => return Err(refusal),
        Err(_nothing_stored) => return Ok(None),
    };
    let document = read_run_document(&path)?;
    let mut rows = document.mutants;
    if let Some(at) = rows.iter().position(|one| one.id == found.id.as_str()) {
        return Ok(Some(rows.remove(at)));
    }
    Ok(same_place(rows, found))
}

/// The stored row for the same mutation, when the identity no longer matches.
///
/// An identity is a function of the file's bytes, so the edit a reader makes before replaying — adding the test that kills the survivor — re-mints it.
/// Matching on the identity alone then finds nothing, and the replay says "was nothing, now killed" about a mutation the run had measured and called survived.
/// Where the identity has moved, the place has not: one file, one rule, one original text and one replacement is the same mutation.
fn same_place(
    stored: Vec<run_report::RunMutantDocument>,
    found: &rust_mutants::catalog::Mutant,
) -> Option<run_report::RunMutantDocument> {
    let mut matching = stored.into_iter().filter(|one| {
        one.path == found.candidate.path
            && one.rule == found.candidate.rule.name
            && one.original.as_bytes() == found.candidate.original.as_slice()
            && one.replacement.as_bytes() == found.candidate.replacement.as_slice()
    });
    let first = matching.next()?;
    matching.next().is_none().then_some(first)
}

/// One mutant, explained from a tree prepared for the purpose.
fn fresh_explain(
    prepared: &Prepared<'_>,
    prefix: &str,
    json: bool,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let session = prepared.session;
    let catalog =
        rust_mutants::report::catalog::document(session, &prepared.settings.prepare_options()?)?;
    let source = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| one.id.as_str().starts_with(prefix))
        .map(|one| read_source(session, &one.candidate.path))
        .transpose()?;
    said(
        &explain::Asked {
            catalog: &catalog,
            run: None,
            prefix,
            source: source.as_deref(),
        },
        json,
        stdout,
    )
}

/// One mutant, explained from what the last run stored rather than from a tree prepared again.
fn stored_explain(
    asked: (&cli::Scope, &str, Option<&str>, bool),
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let (scope, prefix, run, json) = asked;
    let settings = Settings::resolve(scope, environment)?;
    let directory = settings.report_directory();
    let report = stored::report_of(&directory, run)?;
    let run = report.parent().map(Path::to_path_buf).unwrap_or_default();
    let catalog: rust_mutants::report::catalog::CatalogDocument =
        read_document(&run.join(rust_mutants::report::evidence::CATALOG))?;
    let stored = read_run_document(&report)?;
    let source = catalog
        .mutants
        .iter()
        .find(|one| one.id.starts_with(prefix))
        .map(|one| read_source_at(&settings.root, &one.path))
        .transpose()?;
    said(
        &explain::Asked {
            catalog: &catalog,
            run: Some(&stored),
            prefix,
            source: source.as_deref(),
        },
        json,
        stdout,
    )
}

/// One stored document, or the reason it is not one this release reads.
fn read_document<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, CliError> {
    let text = std::fs::read_to_string(path).map_err(|_error| CliError::ReportMissing {
        message: format!("{} is not there to read", path.display()),
    })?;
    crate::strictjson::decode_str(&text).map_err(|error| CliError::ReportMissing {
        message: format!(
            "{} is not a document this release reads: {error}",
            path.display()
        ),
    })
}

/// One run report whose own verdicts, evidence, and findings agree.
fn read_run_document(path: &Path) -> Result<run_report::RunDocument, CliError> {
    let document: run_report::RunDocument = read_document(path)?;
    document
        .validate()
        .map_err(|error| CliError::ReportMissing {
            message: format!("{} is a contradictory run report: {error}", path.display()),
        })?;
    Ok(document)
}

/// What an explanation says, as a document or as the lines a person reads.
fn said(asked: &explain::Asked<'_>, json: bool, stdout: &mut dyn Write) -> Result<u8, CliError> {
    let document = explain::explain(asked).map_err(|error| CliError::ReportMissing {
        message: error.to_string(),
    })?;
    if json {
        let text = json_line(&document)?;
        write(stdout, &text)?;
    } else {
        write(stdout, &report::explained(&document))?;
    }
    Ok(0)
}

/// The workspace root's own name, which is what a stream calls the tree it measured.
fn root_name(settings: &Settings) -> Result<String, CliError> {
    let root = &settings.root;
    match root.file_name() {
        Some(name) => path_text(
            Path::new(name),
            "the workspace name has no exact UTF-8 spelling for the run stream",
        ),
        None => path_text(
            root,
            "the workspace root has no exact UTF-8 spelling for the run stream",
        ),
    }
}

fn path_text(path: &Path, context: &'static str) -> Result<String, CliError> {
    rust_mutants::id::slashed(path).map_err(|source| CliError::PathNotUtf8 { context, source })
}

fn stored_spelling(path: &Path) -> Result<String, CliError> {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_owned)
        .ok_or_else(|| CliError::StoredRunCorrupt {
            path: path.to_path_buf(),
            message: "the stored entry name has no exact UTF-8 spelling".to_owned(),
        })
}

/// Writes the report and everything an audit re-derives its proofs from, and names the report.
fn stored_with_evidence(
    session: &Session,
    settings: &Settings,
    id: &RunId,
    document: &run_report::RunDocument,
) -> Result<PathBuf, CliError> {
    let written = store(&settings.report_directory(), id, document)?;
    for one in rust_mutants::report::evidence::write(
        session,
        &settings.report_directory().join(id.as_str()),
        &settings.prepare_options()?,
    )? {
        session
            .trace()
            .evidence(rust_mutants::trace::EvidenceRecord {
                file: one.file,
                bytes: one.bytes,
                digest: one.digest,
            });
    }
    Ok(written)
}

/// What the equivalence layer is asked, when a run asks it.
fn asking_equivalence<'a>(
    settings: &'a Settings,
    open: &workspace::OpenOptions,
) -> Option<run::Equivalence<'a>> {
    settings
        .config
        .mutation
        .equivalence
        .then(|| run::Equivalence {
            root: &settings.root,
            options: rust_mutants::equivalence::ProveOptions {
                build: settings.config.build.config(),
                open: workspace::OpenOptions {
                    trace: rust_mutants::trace::Recorder::disabled(),
                    ..open.clone()
                },
                timeout: settings.config.mutation.build_timeout,
            },
        })
}

fn init(
    root: Option<&Path>,
    force: bool,
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let root = environment.rooted(root);
    let path = root.join(crate::config::FILE_NAME);
    let destination = entry_kind(&path).map_err(|source| CliError::writing(&path, source))?;
    if destination != EntryKind::Missing && !force {
        return Err(CliError::FileExists { path });
    }
    if matches!(destination, EntryKind::Directory | EntryKind::Other) {
        return Err(CliError::writing(
            &path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the destination is not a regular file",
            ),
        ));
    }
    std::fs::write(&path, crate::config::skeleton())
        .map_err(|source| CliError::writing(&path, source))?;
    let mut line = String::new();
    let written = writeln!(
        line,
        "wrote {}\nevery key in it is commented out, because every one has a default: the \
         file is a place to disagree rather than a thing a run needs\nnext: `rust-mutants \
         doctor` says whether a run can go ahead here, and `rust-mutants run --dry-run` \
         says what one would cost before spending it",
        path.display()
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    write(stdout, &line)?;
    Ok(0)
}

/// Lists the operators this release knows.
///
/// # Errors
/// [`CliError::InvalidValue`] when the tier named is not one of them.
fn rules(tier: Option<&str>, json: bool, stdout: &mut dyn Write) -> Result<u8, CliError> {
    use rust_mutants::rule::{Registry, Tier};
    let registry = Registry::canonical();
    let selected = match tier {
        None => registry.rules().to_vec(),
        Some(named) => {
            let tier = Tier::parse(named).ok_or_else(|| CliError::InvalidValue {
                flag: "--tier".to_owned(),
                value: named.to_owned(),
                expected: Tier::ALL.map(Tier::name).join(" | "),
            })?;
            registry.select_tier(tier)
        }
    };
    let text = if json {
        json_line(&serde_json::json!({
            "document_type": "rust-mutants/rules",
            "schema_version": 1,
            "tool_version": rust_mutants::VERSION,
            "rules": selected
                .iter()
                .map(|rule| {
                    serde_json::json!({
                        "name": rule.name,
                        "family": rule.family.name(),
                        "tier": rule.tier.name(),
                        "version": rule.version,
                    })
                })
                .collect::<Vec<_>>(),
        }))?
    } else {
        listed(&selected)?
    };
    write(stdout, &text)?;
    Ok(0)
}

/// The rules as the lines a person reads, in canonical table order.
fn listed(selected: &[rust_mutants::rule::Rule]) -> Result<String, CliError> {
    let mut text = format!(
        "{:<20} {:<30} {:<9} {}\n",
        "FAMILY", "RULE", "TIER", "VERSION"
    );
    let mut families: usize = 0;
    let mut last = None;
    for rule in selected {
        if last != Some(rule.family) {
            families = families
                .checked_add(1)
                .ok_or(CliError::ProjectionOverflow {
                    projection: "rules",
                    field: "the rule-family count",
                })?;
            last = Some(rule.family);
        }
        let written = writeln!(
            text,
            "{:<20} {:<30} {:<9} {}",
            rule.family.name(),
            rule.name,
            rule.tier.name(),
            rule.version
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    let written = writeln!(
        text,
        "\n{} rules in {families} families. `operators = [...]` in .rust-mutants.toml pins \
         exactly these; a tier selects them by name.",
        selected.len()
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    Ok(text)
}

/// How every command here looks for a toolchain.
fn locating(environment: &Environment) -> rust_mutants::cargo::LocateOptions {
    rust_mutants::cargo::LocateOptions {
        cargo: None,
        search_path: rust_mutants::vars::search_path(&environment.vars),
        env: Some(environment.vars.clone()),
    }
}

/// Which stored report to read back, and how.
#[derive(Debug, Clone, Copy)]
struct Wanted<'a> {
    root: Option<&'a Path>,
    run: Option<&'a str>,
    format: cli::Format,
    output: Option<&'a Path>,
    tui: bool,
}

fn report_back(
    wanted: Wanted<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let Wanted {
        root,
        run,
        format,
        output,
        tui,
    } = wanted;
    let root = environment.rooted(root);
    let config = crate::config::Config::load(&root)?;
    let directory = stored::Store::of(&root, &config.reports.directory).root();
    let path = stored::report_of(&directory, run)?;
    let document = read_run_document(&path)?;
    if format == cli::Format::Json {
        return written(&json_line(&document)?, output, stdout).map(|()| 0);
    }
    if tui {
        let code = document.run.exit_code;
        let sources = report::sources::read(&document, &root)?;
        let taken = crate::tui::browse(document, sources)
            .map_err(|error| CliError::writing(&path, error))?;
        if let Some(id) = taken {
            write(stdout, &format!("{id}\n"))?;
        }
        return Ok(code);
    }
    let projected = match format {
        cli::Format::Lines | cli::Format::Json => report::lines(&document)?,
        cli::Format::Junit => report::junit::document(&document),
        cli::Format::Sarif => json_line(&report::sarif::log(&document))?,
        cli::Format::Markdown => report::markdown::document(&document),
        cli::Format::Html => html::document(&document, &report::sources::read(&document, &root)?),
        cli::Format::Stryker => {
            let sources = report::sources::read(&document, &root)?;
            let thresholds = stryker::Thresholds {
                high: config.reports.stryker.high,
                low: config.reports.stryker.low,
            };
            json_line(&stryker::project(&document, &root, thresholds, &sources)?)?
        }
    };
    written(&projected, output, stdout)?;
    Ok(document.run.exit_code)
}

/// Writes what a command produced where it was asked to.
fn written(text: &str, output: Option<&Path>, stdout: &mut dyn Write) -> Result<(), CliError> {
    match output {
        Some(path) => {
            std::fs::write(path, text).map_err(|error| CliError::writing(path, error))?;
            write(stdout, &format!("{}\n", path.display()))?;
        }
        None => write(stdout, text)?,
    }
    Ok(())
}

/// One file as the engine rewrites it.
fn instrumented(
    workspace: &Workspace,
    discovery: &rust_mutants::discover::Discovery,
    (path, mutant): (&str, Option<&str>),
) -> Result<String, CliError> {
    use rust_mutants::instrument::{Instrumenting, instrument_file, plan_file};
    use rust_mutants::workspace::SessionError;

    let found: Vec<rust_mutants::syntax::Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let source = std::fs::read(workspace.snapshot_root().join(path)).map_err(|source| {
        EngineError::from(SessionError::WriteFailed {
            path: path.to_owned(),
            source,
        })
    })?;
    let placements = plan_file(&discovery.catalog, path, &found).map_err(EngineError::from)?;
    let file = instrument_file(&Instrumenting {
        path,
        source: &source,
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: discovery.catalog.digest(),
        first_item: 0,
        watched: "<watched>",
    })
    .map_err(EngineError::from)?;
    let Some(prefix) = mutant else {
        return Ok(file.text);
    };
    let Some(guard) = file
        .guards
        .iter()
        .find(|guard| guard.id.starts_with(prefix))
    else {
        return Ok(format!("no mutant of {path} answers to {prefix:?}\n"));
    };
    let landed = file
        .branches
        .iter()
        .find(|branch| branch.index == guard.index)
        .and_then(|branch| line_around(&file.text, branch.span.start));
    Ok(format!(
        "MUTANT    {}\nFORM      {}\nSITE      {}\n\n{}\n",
        guard.id,
        guard.form,
        guard.site,
        landed.unwrap_or_else(|| String::from("the guard left no branch in the rewrite"))
    ))
}

/// The whole line of `text` that `offset` sits on, which is what a reader of one guard wants.
#[must_use]
pub fn line_around(text: &str, offset: u32) -> Option<String> {
    let Ok(at) = usize::try_from(offset) else {
        return None;
    };
    let before = text.get(..at)?;
    let from = match before.rfind('\n') {
        Some(newline) => newline.checked_add(1)?,
        None => 0,
    };
    let rest = text.get(at..)?;
    let width = rest.find('\n').unwrap_or(rest.len());
    let to = at.checked_add(width)?;
    text.get(from..to)
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
}

fn json_line<T: serde::Serialize>(value: &T) -> Result<String, CliError> {
    let mut text = serde_json::to_string_pretty(value).map_err(|source| {
        CliError::writing(Path::new("<stdout>"), std::io::Error::other(source))
    })?;
    text.push('\n');
    Ok(text)
}

/// Puts the reports of the parts of one catalog back together.
/// The reports of the parts of one catalog, named directly or found under a report directory.
///
/// # Errors
/// [`CliError::ReportMissing`] when a name or a glob matches no stored run.
fn parts(
    reports: &[PathBuf],
    root: Option<&Path>,
    runs: &[String],
    environment: &Environment,
) -> Result<Vec<PathBuf>, CliError> {
    if runs.is_empty() {
        return Ok(reports.to_vec());
    }
    let root = environment.rooted(root);
    let directory = stored::Store::read(&root).root();
    let mut found: Vec<PathBuf> = reports.to_vec();
    for named in runs {
        let pattern = rust_mutants::glob::Pattern::compile(named).map_err(|error| {
            CliError::InvalidValue {
                flag: "--runs".to_owned(),
                value: named.clone(),
                expected: format!("a run name or a glob: {error}"),
            }
        })?;
        let mut matched = Vec::new();
        let entries = std::fs::read_dir(&directory).map_err(|_error| CliError::ReportMissing {
            message: format!("no run is stored under {}", directory.display()),
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| CliError::StoredRunsUnreadable {
                path: directory.clone(),
                source,
            })?;
            let entry_path = entry.path();
            let name = stored_spelling(&entry_path)?;
            let report = entry_path.join(run_report::FILE_NAME);
            if pattern.matches(&name) {
                match entry_kind(&report).map_err(|source| CliError::StoredRunsUnreadable {
                    path: report.clone(),
                    source,
                })? {
                    EntryKind::File => matched.push(report),
                    EntryKind::Missing | EntryKind::Directory | EntryKind::Other => {}
                }
            }
        }
        if matched.is_empty() {
            return Err(CliError::ReportMissing {
                message: format!(
                    "{named:?} names no stored run under {}",
                    directory.display()
                ),
            });
        }
        matched.sort();
        found.extend(matched);
    }
    Ok(found)
}

fn merge(
    reports: &[PathBuf],
    output: Option<&Path>,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let mut parts = Vec::with_capacity(reports.len());
    for path in reports {
        parts.push(read_run_document(path)?);
    }
    let merged = run_report::merge(&parts).map_err(|error| CliError::ReportMissing {
        message: error.to_string(),
    })?;
    let text = json_line(&merged)?;
    match output {
        Some(path) => {
            std::fs::write(path, &text).map_err(|source| CliError::writing(path, source))?;
            write(stdout, &report::lines(&merged)?)?;
        }
        None => write(stdout, &text)?,
    }
    Ok(merged.run.exit_code)
}

/// A closed stream is the reader's choice, not a failure of ours.
/// The claims the file wrote, as the engine reads them.
fn expectations(settings: &Settings) -> Vec<Expectation> {
    settings
        .config
        .mutation
        .expect
        .iter()
        .map(crate::config::Expect::expectation)
        .collect()
}

pub(crate) fn write(stream: &mut dyn Write, text: &str) -> Result<(), CliError> {
    match stream
        .write_all(text.as_bytes())
        .and_then(|()| stream.flush())
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(source) => Err(CliError::OutputFailed { source }),
    }
}

/// The pristine text of every file that yielded a candidate, so a position can be counted in the file a person would open.
fn read_sources(
    root: &Path,
    discovery: &rust_mutants::discover::Discovery,
) -> Result<BTreeMap<String, String>, CliError> {
    let mut sources = BTreeMap::new();
    for file in discovery.files.iter().filter(|file| file.candidates > 0) {
        let text = read_source_at(root, &file.path)?;
        sources.insert(file.path.clone(), text);
    }
    Ok(sources)
}

/// The text of one file of a prepared session.
fn read_source(session: &Session, path: &str) -> Result<String, CliError> {
    read_source_at(session.snapshot_root(), path)
}

/// The text of one selected source file, with absence and permissions kept as failures.
fn read_source_at(root: &Path, path: &str) -> Result<String, CliError> {
    std::fs::read_to_string(root.join(path)).map_err(|source| CliError::unreadable(path, &source))
}

/// What one equivalence pass is asked about.
struct Asking<'a> {
    root: &'a Path,
    open: workspace::OpenOptions,
    discovery: &'a rust_mutants::discover::Discovery,
    limit: usize,
    trace: &'a rust_mutants::trace::Recorder,
}

/// What the compiler said about one mutant.
struct Rendered {
    display_id: String,
    path: String,
    rule: String,
    answer: String,
}

/// Asks the compiler about every mutant of the catalog, or the first `limit` of them.
fn equivalence(asking: &Asking<'_>, cancel: &Cancel) -> Result<Vec<Rendered>, CliError> {
    let mut prover = rust_mutants::equivalence::Prover::open(
        asking.root,
        &rust_mutants::equivalence::ProveOptions {
            build: rust_mutants::cargo::BuildConfig::default(),
            open: asking.open.clone(),
            timeout: None,
        },
        cancel,
        asking.trace,
    )?;
    let mutants = asking.discovery.catalog.mutants();
    let wanted = if asking.limit == 0 {
        mutants.len()
    } else {
        asking.limit.min(mutants.len())
    };
    let mut said = Vec::with_capacity(wanted);
    for mutant in mutants.iter().take(wanted) {
        let answer = prover.identical(&mutant.candidate, cancel)?;
        said.push(Rendered {
            display_id: mutant.display_id.to_string(),
            path: mutant.candidate.path.clone(),
            rule: mutant.candidate.rule.to_string(),
            answer: answer.name().to_owned(),
        });
    }
    prover.close()?;
    Ok(said)
}

/// One line per mutant, and a count of each answer against the catalog it was taken from.
fn rendered(said: &[Rendered], cataloged: usize) -> Result<String, CliError> {
    let mut text = String::new();
    let mut identical = 0usize;
    for one in said {
        if one.answer == rust_mutants::equivalence::Identity::Identical.name() {
            identical = identical
                .checked_add(1)
                .ok_or(CliError::ProjectionOverflow {
                    projection: "equivalence",
                    field: "the identical-mutant count",
                })?;
        }
        let written = writeln!(
            text,
            "{}\t{}\t{}\t{}",
            one.display_id, one.answer, one.rule, one.path
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    let written = writeln!(
        text,
        "EQUIVALENCE\tasked={} of {cataloged}\tidentical={}\tidentical is not equivalent: \
         code nothing links comes out identical because the linker dropped it",
        said.len(),
        identical
    );
    if said.len() < cataloged {
        let unasked = cataloged
            .checked_sub(said.len())
            .ok_or(CliError::ProjectionOverflow {
                projection: "equivalence",
                field: "the unasked-mutant count",
            })?;
        let written = writeln!(
            text,
            "             the other {unasked} were never asked, so nothing here is a rate over \
             the catalog"
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    Ok(text)
}

/// What `--ui auto` means in this environment.
const fn resolved(ui: crate::ui::Ui) -> crate::ui::Ui {
    match ui {
        crate::ui::Ui::Auto => crate::ui::Ui::Plain,
        held @ (crate::ui::Ui::Plain | crate::ui::Ui::Quiet) => held,
    }
}
