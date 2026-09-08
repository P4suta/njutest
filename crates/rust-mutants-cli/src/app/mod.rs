// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands: what each one opens, what it establishes, and what it writes.

pub mod trace;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::EngineError;
use rust_mutants::report::explain;
use rust_mutants::run::Expectation;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{self, Request, Session};
use rust_mutants::workspace::{self, Workspace};

use crate::error::CliError;
use crate::report::html;
use crate::report::run as run_report;
use crate::report::stryker;
use crate::settings::Settings;
use crate::{Environment, cli, report, run};

mod bundle;
mod doctor;
mod estimate;
mod stored;
mod sweep;

use bundle::{Gathering, bundle};
use doctor::{Asked, doctor};
pub use stored::run_id;
use stored::{named, newest, prune, store};

/// The variables a run composes for itself, which it therefore refuses to inherit.
pub const RESERVED_ENV: [&str; 4] = [
    "RUST_MUTANTS_ACTIVE",
    "RUST_MUTANTS_CATALOG",
    "RUST_MUTANTS_PROBE",
    "RUST_MUTANTS_TOUCH",
];

/// Does what the command asks and returns the exit code it earns.
///
/// # Errors
/// Returns what the command could not do.
pub fn dispatch(
    command: &cli::Command,
    environment: &Environment,
    streams: crate::Streams<'_>,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    let crate::Streams {
        out: stdout,
        err: stderr,
    } = streams;
    if !diagnoses(command) {
        reserved(environment)?;
    }
    match command {
        cli::Command::Init { root, force } => init(root.as_deref(), *force, environment, stdout),
        cli::Command::Doctor {
            root,
            packages,
            json,
        } => Ok(doctor(
            &Asked {
                root: root.as_deref(),
                packages,
                json: *json,
            },
            environment,
            stdout,
            cancel,
        )),
        cli::Command::Explain {
            scope,
            mutant,
            fresh: false,
            json,
        } => stored_explain((scope, mutant, *json), environment, stdout),
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
        _ => kept_command(
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
        _ => workspace_command(
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

/// A run composes its own activation. An inherited one would silently decide what every test process measures.
/// Whether the command is one whose whole job is to say what is wrong here.
///
/// Every other command refuses to inherit a reserved variable, because what a
/// test process said under one is about something else. These two report it
/// instead: a person whose environment is broken runs them to find that out.
const fn diagnoses(command: &cli::Command) -> bool {
    matches!(
        command,
        cli::Command::Doctor { .. } | cli::Command::Diagnostics { .. }
    )
}

fn reserved(environment: &Environment) -> Result<(), CliError> {
    for (name, value) in &environment.vars {
        let name = name.to_string_lossy();
        if RESERVED_ENV.contains(&name.as_ref()) && !value.is_empty() {
            return Err(CliError::EnvironmentReserved {
                name: name.into_owned(),
            });
        }
    }
    Ok(())
}

fn workspace_command(
    command: &cli::Command,
    environment: &Environment,
    streams: crate::Streams<'_>,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    let crate::Streams {
        out: stdout,
        err: stderr,
    } = streams;
    let Some(scope) = command.scope() else {
        return Ok(0);
    };
    let settings = Settings::resolve(scope, environment)?;
    let started = Timestamp::now();
    let id = named(command, started)?;
    let (sender, phases) = std::sync::mpsc::channel();
    let recorder = trace::recorder(
        &trace::Recording {
            scope,
            settings: &settings,
            id: &id,
            command,
        },
        watching(command).then_some(sender),
        stderr,
    );
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
    trace::ended(&recorder, &outcome, cancel);
    if scope.trace.is_some() {
        prune(&settings.report_directory(), settings.config.reports.keep);
    }
    outcome
}

/// Where a run may remember what measuring this tree established.
///
/// The measurement is a function of the tree and not of any mutation, and
/// making it rebuilds every crate in the graph, so a tree nothing has touched
/// since the last run is a whole build a run does not have to do. `--no-cache`
/// asks for the work to be done again, and asking for that has to mean this
/// too, or the flag would only half do what it says.
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

/// Prepares the tree, saying what it is doing while it does it.
///
/// Preparing is most of a long run and a reader who is shown none of it until
/// it is over cannot tell a slow run from a hung one. The work runs on a
/// thread of its own so the display can write each phase as the recorder
/// reaches it; the display is what the calling thread does while it waits, so
/// nothing about who owns the output stream changes.
///
/// A command with no display prepares on the calling thread exactly as before.
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
    let prepared = std::thread::scope(|scope| {
        let working = scope.spawn(|| workspace.prepare(options, cancel));
        let alive = || !working.is_finished();
        match displayed {
            Displayed::Lines => crate::ui::watch(phases, stdout, &alive),
            Displayed::Stream => crate::stream::watch(phases, stdout, &alive),
            Displayed::Nothing => {}
        }
        working.join()
    });
    match prepared {
        Ok(session) => Ok(session?),
        Err(_panicked) => Err(CliError::ReportMissing {
            message: "preparing the tree stopped without saying why".to_owned(),
        }),
    }
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

/// Everything a workspace command needs beyond what it prints.
///
/// The run is named before the workspace is opened, so a recording of the
/// opening itself has somewhere to go.
struct Running<'a> {
    scope: &'a cli::Scope,
    settings: &'a Settings,
    environment: &'a Environment,
    id: &'a str,
    started: Timestamp,
    recorder: &'a rust_mutants::trace::Recorder,
    /// What the recorder has said about the phases it has finished, for a display to write.
    phases: &'a std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
}

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
    let open = settings.open_options(scope, environment, recorder.clone())?;
    let workspace = Workspace::open(&settings.root, open.clone(), cancel)?;
    let mut options = settings.prepare_options()?;
    options.measurements = remembered_measurements(command, environment);
    if let Some(base) = base_of(scope) {
        options.include = selected(running, base, cancel)?;
    }
    match command {
        cli::Command::Equivalence { limit, .. } => {
            let discovery = session::preview(&workspace, &options, cancel)?;
            let root = settings.root.clone();
            let open = settings.open_options(scope, environment, recorder.clone())?;
            workspace.close()?;
            write(
                stdout,
                &rendered(&equivalence(
                    &Asking {
                        root: &root,
                        open,
                        discovery: &discovery,
                        limit: *limit,
                        trace: recorder,
                    },
                    cancel,
                )?),
            );
            Ok(0)
        }
        cli::Command::List { .. }
        | cli::Command::WhySkipped { .. }
        | cli::Command::Instrument { .. } => {
            let discovery = session::preview(&workspace, &options, cancel)?;
            write(stdout, &previewed(command, &workspace, &discovery)?);
            workspace.close()?;
            Ok(0)
        }
        _ => {
            if streaming(command) {
                crate::stream::started(
                    stdout,
                    id,
                    &root_name(settings),
                    report::selection_document(&options),
                );
            }
            let session = preparing(
                workspace,
                &options,
                Displayed::of(command),
                (phases, stdout, cancel),
            )?;
            let code = prepared(
                command,
                &Prepared {
                    session: &session,
                    settings,
                    open: &open,
                    environment,
                    id,
                    started,
                    phases,
                },
                cancel,
                stdout,
            );
            let kept = session.close()?;
            remember(settings, id, &kept, recorder);
            code
        }
    }
}

/// Writes down what a run kept, so a later command can find it and a later sweep can leave it alone.
///
/// A recording never fails a run and neither does this: a ledger that could
/// not be written costs the next `cache` its list, and nothing else.
fn remember(
    settings: &Settings,
    run_id: &str,
    kept: &[PathBuf],
    recorder: &rust_mutants::trace::Recorder,
) {
    if kept.is_empty() {
        return;
    }
    for path in kept {
        recorder.kept(rust_mutants::trace::KeptRecord {
            path: path.display().to_string(),
            run_id: run_id.to_owned(),
        });
    }
    let _written = crate::kept::Ledger::record(&settings.report_directory(), run_id, kept);
}

/// The revision a change set is computed against, when the command line asked for one at all.
fn base_of(scope: &cli::Scope) -> Option<&str> {
    scope
        .changed_from
        .as_deref()
        .or_else(|| scope.changed.then_some(rust_mutants::git::DEFAULT_BASE))
}

/// The patterns a change set selects, narrowing what the configuration already selected.
///
/// A tree git cannot be asked about ends the command: a run that could not see
/// what changed must never look like a run that saw nothing change.
fn selected(
    running: &Running<'_>,
    base: &str,
    cancel: &Cancel,
) -> Result<Vec<rust_mutants::glob::Pattern>, CliError> {
    let Running {
        settings,
        environment,
        recorder,
        ..
    } = *running;
    let report_directory = settings
        .config
        .reports
        .directory
        .to_string_lossy()
        .into_owned();
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
    Ok(rust_mutants::git::within(
        &change,
        &settings.prepare_options()?.include,
    ))
}

/// What a command that only needs discovery prints.
fn previewed(
    command: &cli::Command,
    workspace: &Workspace,
    discovery: &rust_mutants::discover::Discovery,
) -> Result<String, CliError> {
    match command {
        cli::Command::List { file, .. } => Ok(report::list(
            discovery,
            &read_sources(workspace.snapshot_root(), discovery),
            file.as_deref(),
        )),
        cli::Command::WhySkipped { file, line, .. } => Ok(file.as_ref().map_or_else(
            || report::why_skipped(&discovery.skips),
            |path| report::decisions(discovery, path, *line),
        )),
        cli::Command::Instrument { file, mutant, .. } => {
            instrumented(workspace, discovery, (file, mutant.as_deref()))
        }
        _ => Ok(String::new()),
    }
}

/// A prepared session and the configuration it was prepared from.
struct Prepared<'a> {
    session: &'a Session,
    settings: &'a Settings,
    /// How the workspace was opened, so a layer that opens a tree of its own does it the same way.
    open: &'a workspace::OpenOptions,
    environment: &'a Environment,
    id: &'a str,
    started: Timestamp,
    /// What the recorder has said about the phases it has finished, for a display to write.
    phases: &'a std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
}

/// What a command that needs a prepared session does.
fn prepared(
    command: &cli::Command,
    prepared: &Prepared<'_>,
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let Prepared {
        session, settings, ..
    } = *prepared;
    match command {
        cli::Command::Catalog {
            json, rejections, ..
        } => {
            let text = if *json {
                json_line(&report::document(session, &settings.prepare_options()?))
            } else if *rejections {
                report::rejections(session)
            } else {
                report::catalog(session)
            };
            write(stdout, &text);
            Ok(0)
        }
        cli::Command::Explain { mutant, json, .. } => {
            fresh_explain(prepared, mutant, *json, stdout)
        }
        cli::Command::Replay { mutant, run, .. } => {
            replay(prepared, (mutant, run.as_deref()), cancel, stdout)
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
                &{
                    let mut request = Request::new(prefix.clone())
                        .test(test.clone())
                        .with_args(args.clone());
                    if let Some(target) = target {
                        request = request.with_target(target.clone());
                    }
                    request
                },
                cancel,
                stdout,
            ),
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
                    filter: filter(command, prepared.settings)?,
                    phases: prepared.phases,
                    environment: prepared.environment,
                    id: prepared.id,
                    started: prepared.started,
                },
                cancel,
                stdout,
            ),
        },
        _ => Ok(0),
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
    write(stdout, &report::outcome(&result, &found));
    Ok(report::exit_code(result.outcome))
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
    id: &'a str,
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
) -> Result<u8, CliError> {
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
    let keyed = keyed(session, settings, args);
    let expectations = expectations(settings);
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
            run_id: id,
        }),
        filter: Some(filter),
        fail_fast,
    };
    if dry_run {
        write(stdout, &crate::ui::phases(phases));
        write(stdout, &estimate::estimate(session, filter));
        return Ok(0);
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
    result.expectations = run::verify(session, &expectations, &mut result.judged);
    let document = run_report::document(
        session,
        &result,
        selection,
        &run_report::Meta {
            id,
            started_at: started,
            finished_at: Timestamp::now(),
        },
    );
    let written = if no_report {
        None
    } else {
        Some(stored_with_evidence(session, settings, id, &document)?)
    };
    concluded(session, &document, (json, written.as_deref()), stdout);
    prune(&settings.report_directory(), settings.config.reports.keep);
    Ok(document.run.exit_code)
}

/// What a finished run says: the stream's last lines, or the summary a person reads.
fn concluded(
    session: &Session,
    document: &run_report::RunDocument,
    (json, written): (bool, Option<&Path>),
    stdout: &mut dyn Write,
) {
    if json {
        crate::stream::Writer::new(stdout, session)
            .ended(document, written.map(|path| path.display().to_string()));
        return;
    }
    write(stdout, "\n");
    write(stdout, &report::lines(document));
    if let Some(path) = written {
        let mut line = String::new();
        let ok = writeln!(line, "REPORT    {}", path.display());
        debug_assert!(ok.is_ok(), "writing to a String cannot fail");
        write(stdout, &line);
    }
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
        return Ok(run::run(session, watched.options, cancel, &mut writer)?);
    }
    write(stdout, &crate::ui::phases(watched.phases));
    Ok(run::run(
        session,
        watched.options,
        cancel,
        &mut crate::ui::Display::new(
            stdout,
            resolved(watched.ui),
            watched.paints,
            run::jobs(watched.settings.config.execution.jobs),
        ),
    )?)
}

/// Everything beyond a mutant's own identity that a stored outcome is keyed on.
///
/// A record answers for a mutant only when the tree, the catalog, the harness
/// arguments, the budget and the build are the ones it was established under:
/// anything else is an answer to a different question.
fn keyed(session: &Session, settings: &Settings, args: &[String]) -> crate::outcomes::Keyed {
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
        build: settings.config.build.config().arguments(),
    }
}

/// Which of the catalog's mutants a run was asked for, from the flags that narrow it.
///
/// # Errors
/// A `--file` whose lines are not a range.
fn filter(command: &cli::Command, settings: &Settings) -> Result<run::Filter, CliError> {
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

/// The mutants a stored run left with `outcome`, by identity.
///
/// A run named by nothing is the newest one under the report directory. What
/// a report names and this catalog no longer holds selects nothing, which is
/// what an identity minted from a file's digest does when the file changes;
/// the run reports the rest as unselected rather than pretending otherwise.
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
    let text = std::fs::read_to_string(&path).map_err(|_error| CliError::ReportMissing {
        message: format!("{} is not a stored run", path.display()),
    })?;
    let document: run_report::RunDocument =
        serde_json::from_str(&text).map_err(|_error| CliError::ReportMissing {
            message: format!("{} is not a run report", path.display()),
        })?;
    Ok(document
        .mutants
        .into_iter()
        .filter(|one| one.outcome == outcome)
        .map(|one| one.id)
        .collect())
}

/// One `--file` value: a path, and the lines of it the run is about.
///
/// # Errors
/// [`CliError::InvalidValue`] for lines that are not a range.
fn addressed(text: &str) -> Result<(String, Option<(u32, u32)>), CliError> {
    let Some((path, lines)) = text.rsplit_once(':') else {
        return Ok((text.to_owned(), None));
    };
    let refuse = || CliError::InvalidValue {
        flag: "--file".to_owned(),
        value: text.to_owned(),
        expected: "PATH, PATH:LINE, or PATH:FROM-TO".to_owned(),
    };
    let (from, to) = lines.split_once('-').unwrap_or((lines, lines));
    let from: u32 = from.parse().map_err(|_error| refuse())?;
    let to: u32 = to.parse().map_err(|_error| refuse())?;
    if from == 0 || to < from {
        return Err(refuse());
    }
    Ok((path.to_owned(), Some((from, to))))
}

/// One finding, put back to the tests exactly as the run that found it did.
///
/// What a replay adds over `run --mutant` is the run's own answer: the target
/// and the test that noticed it, read out of the stored report rather than
/// guessed at, so a replay asks the question the run asked rather than a
/// wider one. What it establishes is whether the answer is still the same.
fn replay(
    prepared: &Prepared<'_>,
    asked: (&str, Option<&str>),
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let (prefix, run) = asked;
    let session = prepared.session;
    let found = session.resolve(prefix)?.clone();
    let stored = recorded(prepared.settings, run, &found.id);
    let mut request = Request::new(found.display_id.clone());
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
            verdict(stored.as_ref(), result.outcome.name())
        ),
    );
    write(stdout, &report::outcome(&result, &found));
    Ok(report::exit_code(result.outcome))
}

/// What the replay establishes about the stored answer.
///
/// A mutant a proof discharged has no measured outcome to be the same as: the
/// run said running it would establish what it already knew, and the replay is
/// what puts that to the tests. Surviving is the proof holding, and a replay
/// says so rather than reporting the answer as changed. Anything else is the
/// proof contradicted, which is a fact about this engine rather than about the
/// project's tests, and it is said in those words.
fn verdict(stored: Option<&run_report::RunMutantDocument>, now: &str) -> String {
    let Some(row) = stored else {
        return format!("was nothing, now {now}");
    };
    let discharged = run::NotRunReason::Discharged.name();
    let survived = rust_mutants::outcome::Outcome::Survived.name();
    if row.not_run_reason.as_deref() == Some(discharged) {
        return if now == survived {
            format!("{now}, which is the proof that discharged it holding")
        } else {
            format!("{now}, and a proof discharged it: the proof is wrong")
        };
    }
    if row.outcome == now {
        format!("still {now}")
    } else {
        format!("was {}, now {now}", row.outcome)
    }
}

/// What a stored run said about one mutant, when a stored run said anything.
fn recorded(
    settings: &Settings,
    run: Option<&str>,
    id: &str,
) -> Option<run_report::RunMutantDocument> {
    let directory = settings.report_directory();
    let path = match run {
        Some(named) => directory.join(named).join(run_report::FILE_NAME),
        None => newest(&directory).ok()?,
    };
    let text = std::fs::read_to_string(path).ok()?;
    let document: run_report::RunDocument = serde_json::from_str(&text).ok()?;
    document.mutants.into_iter().find(|one| one.id == id)
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
        rust_mutants::report::catalog::document(session, &prepared.settings.prepare_options()?);
    let source = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| one.id.starts_with(prefix))
        .and_then(|one| read_source(session, &one.candidate.path));
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
///
/// An explanation costs two documents to read: the catalog the run kept and
/// the report it wrote. Nothing is copied, nothing is compiled, and nothing is
/// instrumented, which is what makes it a thing a person runs while reading a
/// report rather than a thing they wait for.
fn stored_explain(
    asked: (&cli::Scope, &str, bool),
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let (scope, prefix, json) = asked;
    let settings = Settings::resolve(scope, environment)?;
    let directory = settings.report_directory();
    let report = newest(&directory)?;
    let run = report.parent().map(Path::to_path_buf).unwrap_or_default();
    let catalog: rust_mutants::report::catalog::CatalogDocument =
        read_document(&run.join(rust_mutants::report::evidence::CATALOG))?;
    let stored: run_report::RunDocument = read_document(&report)?;
    let source = catalog
        .mutants
        .iter()
        .find(|one| one.id.starts_with(prefix))
        .and_then(|one| std::fs::read_to_string(settings.root.join(&one.path)).ok());
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
    serde_json::from_str(&text).map_err(|error| CliError::ReportMissing {
        message: format!(
            "{} is not a document this release reads: {error}",
            path.display()
        ),
    })
}

/// What an explanation says, as a document or as the lines a person reads.
fn said(asked: &explain::Asked<'_>, json: bool, stdout: &mut dyn Write) -> Result<u8, CliError> {
    let document = explain::explain(asked).map_err(|error| CliError::ReportMissing {
        message: error.to_string(),
    })?;
    if json {
        let text =
            serde_json::to_string_pretty(&document).unwrap_or_else(|_error| String::from("{}"));
        write(stdout, &text);
        write(stdout, "\n");
    } else {
        write(stdout, &report::explained(&document));
    }
    Ok(0)
}

/// The workspace root's own name, which is what a stream calls the tree it measured.
fn root_name(settings: &Settings) -> String {
    let root = settings
        .root
        .canonicalize()
        .unwrap_or_else(|_error| settings.root.clone());
    root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Writes the report and everything an audit re-derives its proofs from, and names the report.
fn stored_with_evidence(
    session: &Session,
    settings: &Settings,
    id: &str,
    document: &run_report::RunDocument,
) -> Result<PathBuf, CliError> {
    let written = store(&settings.report_directory(), id, document)?;
    for one in rust_mutants::report::evidence::write(
        session,
        &settings.report_directory().join(id),
        &settings.prepare_options()?,
    ) {
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
    let root = root.map_or_else(|| environment.working_directory.clone(), Path::to_path_buf);
    let path = root.join(crate::config::FILE_NAME);
    if path.exists() && !force {
        return Err(CliError::FileExists { path });
    }
    std::fs::write(&path, crate::config::skeleton())
        .map_err(|source| CliError::writing(&path, source))?;
    let mut line = String::new();
    let written = writeln!(line, "wrote {}", path.display());
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    write(stdout, &line);
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
        }))
    } else {
        listed(&selected)
    };
    write(stdout, &text);
    Ok(0)
}

/// The rules as the lines a person reads, in canonical table order.
fn listed(selected: &[rust_mutants::rule::Rule]) -> String {
    let mut text = format!(
        "{:<20} {:<26} {:<9} {}\n",
        "FAMILY", "RULE", "TIER", "VERSION"
    );
    let mut families: usize = 0;
    let mut last = None;
    for rule in selected {
        if last != Some(rule.family) {
            families = families.saturating_add(1);
            last = Some(rule.family);
        }
        let written = writeln!(
            text,
            "{:<20} {:<26} {:<9} {}",
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
    text
}

/// How every command here looks for a toolchain.
fn locating(environment: &Environment) -> rust_mutants::cargo::LocateOptions {
    rust_mutants::cargo::LocateOptions {
        cargo: None,
        search_path: environment
            .vars
            .iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.clone()),
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
    let root = root.map_or_else(|| environment.working_directory.clone(), Path::to_path_buf);
    let config = crate::config::Config::load(&root)?;
    let directory = root.join(&config.reports.directory);
    let path = match run {
        Some(id) => directory.join(id).join(run_report::FILE_NAME),
        None => newest(&directory)?,
    };
    let text = std::fs::read_to_string(&path).map_err(|error| CliError::ReportMissing {
        message: format!("{}: {error}", path.display()),
    })?;
    if format == cli::Format::Json {
        return written(&text, output, stdout).map(|()| 0);
    }
    let document: run_report::RunDocument =
        serde_json::from_str(&text).map_err(|error| CliError::ReportMissing {
            message: format!("{} is not a run report: {error}", path.display()),
        })?;
    if tui {
        let code = document.run.exit_code;
        let sources = report::sources::read(&document, &root).unwrap_or_default();
        let taken = crate::tui::browse(document, sources)
            .map_err(|error| CliError::writing(&path, error))?;
        if let Some(id) = taken {
            write(stdout, &format!("{id}\n"));
        }
        return Ok(code);
    }
    let projected = match format {
        cli::Format::Lines | cli::Format::Json => report::lines(&document),
        cli::Format::Junit => report::junit::document(&document),
        cli::Format::Sarif => json_line(&report::sarif::log(&document)),
        cli::Format::Markdown => report::markdown::document(&document),
        cli::Format::Html => html::document(&document, &report::sources::read(&document, &root)?),
        cli::Format::Stryker => {
            let sources = report::sources::read(&document, &root)?;
            let thresholds = stryker::Thresholds {
                high: config.reports.stryker.high,
                low: config.reports.stryker.low,
            };
            json_line(&stryker::project(&document, &root, thresholds, &sources)?)
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
            write(stdout, &format!("{}\n", path.display()));
        }
        None => write(stdout, text),
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
fn line_around(text: &str, offset: u32) -> Option<String> {
    let at = usize::try_from(offset).ok()?;
    let before = text.get(..at)?;
    let from = before
        .rfind('\n')
        .map_or(0, |newline| newline.saturating_add(1));
    let rest = text.get(at..)?;
    let to = at.saturating_add(rest.find('\n').unwrap_or(rest.len()));
    text.get(from..to).map(ToOwned::to_owned)
}

fn json_line<T: serde::Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value)
        .unwrap_or_else(|error| format!("{{\"error\":{error:?}}}"));
    text.push('\n');
    text
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
    let root = root.map_or_else(|| environment.working_directory.clone(), Path::to_path_buf);
    let directory = root.join(crate::config::DEFAULT_REPORTS_DIRECTORY);
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
        for entry in std::fs::read_dir(&directory)
            .map_err(|_error| CliError::ReportMissing {
                message: format!("no run is stored under {}", directory.display()),
            })?
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            let report = entry.path().join(run_report::FILE_NAME);
            if pattern.matches(&name) && report.is_file() {
                matched.push(report);
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
        let text = std::fs::read_to_string(path).map_err(|error| CliError::ReportMissing {
            message: format!("{}: {error}", path.display()),
        })?;
        parts.push(
            serde_json::from_str::<run_report::RunDocument>(&text).map_err(|error| {
                CliError::ReportMissing {
                    message: format!("{} is not a run report: {error}", path.display()),
                }
            })?,
        );
    }
    let merged = run_report::merge(&parts).map_err(|error| CliError::ReportMissing {
        message: error.to_string(),
    })?;
    let text = json_line(&merged);
    match output {
        Some(path) => {
            std::fs::write(path, &text).map_err(|source| CliError::writing(path, source))?;
            write(stdout, &report::lines(&merged));
        }
        None => write(stdout, &text),
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

pub(super) fn write(stream: &mut dyn Write, text: &str) {
    let _written = stream
        .write_all(text.as_bytes())
        .and_then(|()| stream.flush());
}

/// The pristine text of every file that yielded a candidate, so a position can be counted in the file a person would open.
fn read_sources(
    root: &Path,
    discovery: &rust_mutants::discover::Discovery,
) -> BTreeMap<String, String> {
    discovery
        .files
        .iter()
        .filter(|file| file.candidates > 0)
        .filter_map(|file| {
            std::fs::read_to_string(root.join(&file.path))
                .ok()
                .map(|text| (file.path.clone(), text))
        })
        .collect()
}

/// The text of one file of a prepared session.
fn read_source(session: &Session, path: &str) -> Option<String> {
    std::fs::read_to_string(session.snapshot_root().join(path)).ok()
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
///
/// This says `identical` and never `equivalent`: a mutation of a function
/// nothing calls is dropped by the linker and comes out identical for the
/// opposite of a reassuring reason, and only a run that knows which tests
/// executed the position can tell the two apart
/// ([ADR 0013](../../../../docs/adr/0013-codegen-identity-is-the-equivalence-proof.md)).
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
            display_id: mutant.display_id.clone(),
            path: mutant.candidate.path.clone(),
            rule: mutant.candidate.rule.to_string(),
            answer: answer.name().to_owned(),
        });
    }
    prover.close()?;
    Ok(said)
}

/// One line per mutant, and a count of each answer.
fn rendered(said: &[Rendered]) -> String {
    let mut text = String::new();
    let mut identical = 0usize;
    for one in said {
        if one.answer == "identical" {
            identical = identical.saturating_add(1);
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
        "EQUIVALENCE\tasked={}\tidentical={}\tidentical is not equivalent: code nothing links \
         comes out identical because the linker dropped it",
        said.len(),
        identical
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    text
}

/// What `--ui auto` means in this environment.
///
/// A terminal and a log want the same lines in the same order; what a terminal
/// gets on top is the tally rewritten in place, which a log cannot use. Both
/// are `plain` until there is a renderer that overwrites, and `auto` is where
/// that choice will be made.
const fn resolved(ui: crate::ui::Ui) -> crate::ui::Ui {
    match ui {
        crate::ui::Ui::Auto => crate::ui::Ui::Plain,
        other => other,
    }
}
