// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands: what each one opens, what it establishes, and what it writes.

pub mod trace;

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
use rust_mutants::{snapshot, tempowner};

use crate::error::CliError;
use crate::report::doctor as doctor_report;
use crate::report::html;
use crate::report::run as run_report;
use crate::report::stryker;
use crate::settings::Settings;
use crate::{Environment, cli, report, run};

/// The variables a run composes for itself, which it therefore refuses to inherit.
pub const RESERVED_ENV: [&str; 3] = [
    "RUST_MUTANTS_ACTIVE",
    "RUST_MUTANTS_CATALOG",
    "RUST_MUTANTS_PROBE",
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
        } => cache(
            &Sweeping {
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
            let session = workspace.prepare(&options, cancel)?;
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
        write(stdout, &estimate(session, filter));
        return Ok(0);
    }
    let mut result = measured_run(
        session,
        &Watched {
            options: &options,
            selection: &selection,
            settings,
            phases,
            json,
            ui,
            paints: environment.paints,
            id,
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
    selection: &'a rust_mutants::report::catalog::SelectionDocument,
    settings: &'a Settings,
    phases: &'a std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
    json: bool,
    ui: crate::ui::Ui,
    paints: bool,
    id: &'a str,
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
        writer.started(
            watched.id,
            &root_name(watched.settings),
            watched.selection.clone(),
        );
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
        workspace: session.workspace_digest().to_owned(),
        catalog: session.catalog().digest().to_owned(),
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
    let mut ids = ids.clone();
    if let Some(named) = from_report {
        ids.extend(stored_outcomes(settings, named, outcome)?);
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
        ids,
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

/// What a run would cost, from what preparing established and before a mutant is executed.
fn estimate(session: &Session, filter: &run::Filter) -> String {
    let mut selected = 0u32;
    let mut left_out = 0u32;
    let mut unreached = 0u32;
    let mut text = String::new();
    for index in session.accepted() {
        let Some(mutant) = session.catalog().by_index(*index) else {
            continue;
        };
        let at = session.position(mutant);
        let line = at.map_or(0, |one| one.line);
        if !filter.is_empty() && !filter.selects(mutant, line) {
            left_out = left_out.saturating_add(1);
            continue;
        }
        let route = session.route(mutant);
        let targets = route.reaching().len();
        if targets == 0 {
            unreached = unreached.saturating_add(1);
        } else {
            selected = selected.saturating_add(1);
        }
        let written = writeln!(
            text,
            "#{:<5} {}  {:<22} {}:{}  {}  {} targets",
            mutant.index,
            mutant.display_id,
            mutant.candidate.rule.name,
            mutant.candidate.path,
            line,
            route.granularity(),
            targets
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    let each = session.slowest_baseline();
    let seconds = u64::from(selected).saturating_mul(each.as_secs().max(1));
    let written = writeln!(
        text,
        "\nwould run {selected} mutants against up to {} targets, about {}:{:02}:{:02} at ~{}s \
         per target; {unreached} unreached; {left_out} unselected",
        session.targets().len(),
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        each.as_secs().max(1),
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    text
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
    let before = stored
        .as_ref()
        .map_or_else(|| String::from("nothing"), |row| row.outcome.clone());
    let now = result.outcome.name();
    let verdict = if before == now {
        format!("still {now}")
    } else {
        format!("was {before}, now {now}")
    };
    write(
        stdout,
        &format!("REPLAY    {} {verdict}\n", found.display_id),
    );
    write(stdout, &report::outcome(&result, &found));
    Ok(report::exit_code(result.outcome))
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
    settings
        .root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
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

/// The name this run goes by, which is what its report directory is called.
///
/// # Errors
/// [`CliError::InvalidValue`] for a name that is not one a directory can be.
fn named(command: &cli::Command, now: Timestamp) -> Result<String, CliError> {
    let cli::Command::Run {
        run_id: Some(name), ..
    } = command
    else {
        return Ok(run_id(now));
    };
    let shaped = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|one| one.is_ascii_alphanumeric() || matches!(one, '.' | '_' | '-'));
    if shaped {
        Ok(name.clone())
    } else {
        Err(CliError::InvalidValue {
            flag: "--run-id".to_owned(),
            value: name.clone(),
            expected: "1 to 64 of letters, digits, `.`, `_` and `-`".to_owned(),
        })
    }
}

/// The name a run goes by when nobody named it: the instant it started, which sorts chronologically as a directory name.
#[must_use]
pub fn run_id(now: Timestamp) -> String {
    now.strftime("%Y%m%dT%H%M%S%3fZ")
        .to_string()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Writes the report under `directory/<id>/`, and the pointer that names the newest run.
fn store(
    directory: &Path,
    id: &str,
    document: &run_report::RunDocument,
) -> Result<PathBuf, CliError> {
    let dir = directory.join(id);
    std::fs::create_dir_all(&dir).map_err(|source| CliError::writing(&dir, source))?;
    let path = dir.join(run_report::FILE_NAME);
    std::fs::write(&path, json_line(document))
        .map_err(|source| CliError::writing(&path, source))?;
    let latest = directory.join(run_report::LATEST_FILE_NAME);
    let pointer = serde_json::json!({
        "document_type": "rust-mutants/latest-run",
        "schema_version": 1,
        "run": id,
        "document": format!("{id}/{}", run_report::FILE_NAME),
    });
    std::fs::write(&latest, json_line(&pointer))
        .map_err(|source| CliError::writing(&latest, source))?;
    Ok(path)
}

/// Keeps the newest `keep` stored runs and the newest `keep` recordings of the other commands. Both sort chronologically by name, so the oldest are the first.
///
/// A directory that holds no run report is not a run and never costs a run its
/// place: `traces/` sorts after every run name, and counting it would leave
/// `keep - 1` runs stored.
fn prune(directory: &Path, keep: u32) {
    if keep == 0 {
        return;
    }
    let (runs, recordings) = kept(directory);
    oldest(&runs, keep);
    oldest(&recordings, keep);
    oldest(
        &subdirectories(&directory.join(trace::TRACES_DIRECTORY_NAME)),
        keep,
    );
}

/// The stored runs and, apart from them, the directories a run that wrote no report left a recording in.
///
/// A run asked to record and not to report still names itself and still keeps
/// what it recorded, so those directories are bounded by `keep` of their own
/// rather than either counting against the stored runs or growing forever.
fn kept(directory: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut runs = Vec::new();
    let mut recordings = Vec::new();
    for path in subdirectories(directory) {
        if path
            .file_name()
            .is_some_and(|name| name == trace::TRACES_DIRECTORY_NAME)
        {
            continue;
        }
        if path.join(run_report::FILE_NAME).is_file() {
            runs.push(path);
        } else if path.join(trace::RUN_DIRECTORY_NAME).is_dir() {
            recordings.push(path);
        }
    }
    (runs, recordings)
}

/// Removes everything but the newest `keep` of `directories`.
fn oldest(directories: &[PathBuf], keep: u32) {
    let excess = directories
        .len()
        .saturating_sub(usize::try_from(keep).unwrap_or(usize::MAX));
    for old in directories.iter().take(excess) {
        drop(std::fs::remove_dir_all(old));
    }
}

/// Every directory directly under `directory`, in name order.
fn subdirectories(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
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

fn doctor(
    asked: &Asked<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
    cancel: &Cancel,
) -> u8 {
    let document = doctor_document(asked, environment, cancel);
    let text = if asked.json {
        json_line(&document)
    } else {
        doctor_report::lines(&document)
    };
    write(stdout, &text);
    if document.ok { 0 } else { crate::EXIT_USAGE }
}

/// What a run would find in this environment, as the document both the lines and a bundle are made of.
fn doctor_document(
    asked: &Asked<'_>,
    environment: &Environment,
    cancel: &Cancel,
) -> doctor_report::DoctorDocument {
    use doctor_report::Standing::{Fail, Ok as Well};
    let root = asked
        .root
        .map_or_else(|| environment.working_directory.clone(), Path::to_path_buf);
    let mut checks: Vec<doctor_report::Check> = Vec::new();

    let toolchain = rust_mutants::cargo::Toolchain::locate(&locating(environment), &root, cancel);
    match &toolchain {
        Ok(found) => {
            checks.push(noted("cargo", Well, &found.cargo_version().summary));
            checks.push(noted("rustc", Well, &found.rustc_version().summary));
            checks.push(noted("host", Well, found.host()));
        }
        Err(error) => checks.push(doctor_report::Check::new(
            "toolchain",
            Fail,
            &error.to_string(),
            Some("install a toolchain with rustup, or put cargo on PATH"),
        )),
    }

    let manifest = root.join("Cargo.toml");
    checks.push(workspace_check(&root, &manifest));

    let config_path = root.join(crate::config::FILE_NAME);
    if config_path.is_file() {
        match crate::config::Config::load(&root) {
            Ok(_read) => checks.push(noted("config", Well, &config_path.display().to_string())),
            Err(error) => checks.push(doctor_report::Check::new(
                "config",
                Fail,
                &error.to_string(),
                error.code().remedy,
            )),
        }
    } else {
        checks.push(noted(
            "config",
            Well,
            &format!(
                "none; the defaults apply. `rust-mutants init` writes {}",
                crate::config::FILE_NAME
            ),
        ));
    }

    let temp = &environment.temp_directory;
    checks.push(doctor_report::Check::new(
        "temp",
        if temp.is_dir() { Well } else { Fail },
        &format!(
            "{} (snapshots as {}*, target directories as {}*)",
            temp.display(),
            snapshot::DIR_PREFIX,
            workspace::TARGET_DIR_PREFIX
        ),
        (!temp.is_dir()).then_some("set TMPDIR to a directory a run may write in"),
    ));

    checks.push(git_check(environment));
    checks.push(targets_check(
        toolchain.as_ref().ok(),
        &root,
        asked.packages,
        cancel,
    ));
    checks.push(environment_check(environment));
    checks.push(cache_check(environment));
    checks.push(disk_check(&environment.temp_directory));
    checks.push(snapshots_check(&root, environment));
    checks.push(llvm_tools_check(toolchain.as_ref().ok()));
    doctor_report::DoctorDocument::of(checks)
}

/// Which run to gather, and where to put it.
#[derive(Debug, Clone, Copy)]
struct Gathering<'a> {
    /// The workspace root. Defaults to the working directory.
    root: Option<&'a Path>,
    /// The run, by its identity. The newest when none is named.
    run: Option<&'a str>,
    /// Where the bundle goes, when it does not go beside the run.
    output: Option<&'a Path>,
}

/// Gathers everything one run established into one directory.
///
/// # Errors
/// [`CliError::ReportMissing`] when no stored run answers to what was asked
/// for, and [`CliError::WriteFailed`] when the bundle cannot be written.
fn bundle(
    asked: &Gathering<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
    cancel: &Cancel,
) -> Result<u8, CliError> {
    let root = asked
        .root
        .map_or_else(|| environment.working_directory.clone(), Path::to_path_buf);
    let reports = root.join(crate::config::DEFAULT_REPORTS_DIRECTORY);
    let directory = match asked.run {
        Some(named) => {
            let directory = reports.join(named);
            if !directory.join(run_report::FILE_NAME).is_file() {
                return Err(CliError::ReportMissing {
                    message: format!("{named:?} names no stored run under {}", reports.display()),
                });
            }
            directory
        }
        None => newest(&reports)?
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| CliError::ReportMissing {
                message: format!("no run report is stored under {}", reports.display()),
            })?,
    };
    let run_id = directory
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let bundle = asked.output.map_or_else(
        || directory.join(crate::diagnostics::DIRECTORY_NAME),
        Path::to_path_buf,
    );
    std::fs::create_dir_all(&bundle).map_err(|source| CliError::writing(&bundle, source))?;

    let doctor = json_line(&doctor_document(
        &Asked {
            root: Some(&root),
            packages: &[],
            json: true,
        },
        environment,
        cancel,
    ));
    let toolchain = toolchain_text(&root, environment, cancel);
    let names = crate::diagnostics::environment_names(&environment.vars);
    let parts = gathered(
        &directory,
        &root,
        Wrote {
            doctor: &doctor,
            toolchain: &toolchain,
            names: &names,
        },
    );
    let (held, absent) = crate::diagnostics::gather(&bundle, &parts);
    let document = crate::diagnostics::manifest(&run_id, &directory, held, absent.clone());
    let manifest = bundle.join(crate::diagnostics::MANIFEST_NAME);
    std::fs::write(&manifest, json_line(&document))
        .map_err(|source| CliError::writing(&manifest, source))?;

    let mut text = format!("{}\n", bundle.display());
    for name in &absent {
        let written = writeln!(text, "absent\t{name}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    write(stdout, &text);
    Ok(0)
}

/// What a bundle carries that no run left: this command wrote it for the reader.
#[derive(Debug, Clone, Copy)]
struct Wrote<'a> {
    /// The doctor document, as it stands now.
    doctor: &'a str,
    /// What the toolchain says about itself.
    toolchain: &'a str,
    /// The names of the variables that were set, and none of their values.
    names: &'a str,
}

/// What one bundle is gathered from: what the run left, and what this command wrote for it.
fn gathered<'a>(
    directory: &Path,
    root: &Path,
    wrote: Wrote<'a>,
) -> Vec<(&'static str, crate::diagnostics::Part<'a>)> {
    let Wrote {
        doctor,
        toolchain,
        names,
    } = wrote;
    vec![
        (
            run_report::FILE_NAME,
            crate::diagnostics::Part::File(directory.join(run_report::FILE_NAME)),
        ),
        (
            rust_mutants::report::evidence::CATALOG,
            crate::diagnostics::Part::File(directory.join(rust_mutants::report::evidence::CATALOG)),
        ),
        (
            rust_mutants::report::evidence::REACHED,
            crate::diagnostics::Part::File(directory.join(rust_mutants::report::evidence::REACHED)),
        ),
        (
            rust_mutants::report::evidence::PROBE,
            crate::diagnostics::Part::Tree(directory.join(rust_mutants::report::evidence::PROBE)),
        ),
        (
            trace::RUN_DIRECTORY_NAME,
            crate::diagnostics::Part::Tree(directory.join(trace::RUN_DIRECTORY_NAME)),
        ),
        (
            crate::config::FILE_NAME,
            crate::diagnostics::Part::File(root.join(crate::config::FILE_NAME)),
        ),
        (
            crate::diagnostics::DOCTOR_NAME,
            crate::diagnostics::Part::Text(doctor),
        ),
        (
            crate::diagnostics::TOOLCHAIN_NAME,
            crate::diagnostics::Part::Text(toolchain),
        ),
        (
            crate::diagnostics::ENVIRONMENT_NAME,
            crate::diagnostics::Part::Text(names),
        ),
    ]
}

/// What the toolchain says about itself, for a reader who has a different one.
fn toolchain_text(root: &Path, environment: &Environment, cancel: &Cancel) -> String {
    let located = rust_mutants::cargo::Toolchain::locate(&locating(environment), root, cancel);
    match located {
        Ok(found) => format!(
            "cargo: {}\nrustc: {}\nhost: {}\nrust-mutants: {}\n",
            found.cargo_version().summary,
            found.rustc_version().summary,
            found.host(),
            rust_mutants::VERSION
        ),
        Err(error) => format!("{error}\nrust-mutants: {}\n", rust_mutants::VERSION),
    }
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

/// Whether git is installed, which is what `--changed` asks.
fn git_check(environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let printed = std::process::Command::new("git")
        .arg("--version")
        .envs(environment.vars.clone())
        .output();
    match printed {
        Ok(printed) if printed.status.success() => doctor_report::Check::new(
            "git",
            Well,
            String::from_utf8_lossy(&printed.stdout).trim(),
            None,
        ),
        _ => doctor_report::Check::new(
            "git",
            Warn,
            "git is not there, so --changed has nothing to ask",
            Some("install git, or select with --file and --package instead"),
        ),
    }
}

/// Whether anything would run: a package without a test target answers nothing about its mutants.
fn targets_check(
    toolchain: Option<&rust_mutants::cargo::Toolchain>,
    root: &Path,
    packages: &[String],
    cancel: &Cancel,
) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well, Warn};
    let Some(toolchain) = toolchain else {
        return doctor_report::Check::new(
            "targets",
            Warn,
            "there is no cargo to ask what the targets are",
            None,
        );
    };
    let trace = rust_mutants::trace::Recorder::disabled();
    let driver = rust_mutants::cargo::Driver {
        toolchain,
        dir: root,
        cancel,
        trace: &trace,
    };
    let metadata = rust_mutants::cargo::Metadata::load_no_deps(
        &driver,
        rust_mutants::cargo::MetadataOptions::default(),
    );
    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(error) => {
            return doctor_report::Check::new(
                "targets",
                Warn,
                &error.to_string(),
                Some("fix what cargo metadata says before asking about mutants"),
            );
        }
    };
    let selected: Vec<&rust_mutants::cargo::Package> = metadata
        .members()
        .filter(|package| packages.is_empty() || packages.contains(&package.name))
        .collect();
    let barren: Vec<&str> = selected
        .iter()
        .filter(|package| !package.targets.iter().any(tests_something))
        .map(|package| package.name.as_str())
        .collect();
    let tested = selected.len().saturating_sub(barren.len());
    if selected.is_empty() || tested == 0 {
        return doctor_report::Check::new(
            "targets",
            Fail,
            &format!(
                "{} packages selected, none with a test target",
                selected.len()
            ),
            Some("write a test, or select a package that has one with --package"),
        );
    }
    if barren.is_empty() {
        return doctor_report::Check::new(
            "targets",
            Well,
            &format!("{tested} packages, each with a test target"),
            None,
        );
    }
    doctor_report::Check::new(
        "targets",
        Warn,
        &format!("{} has no test target", barren.join(", ")),
        Some("a package without a test target answers nothing; narrow with --package"),
    )
}

/// Whether a target is one a run would execute.
fn tests_something(target: &rust_mutants::cargo::Target) -> bool {
    target.test
        && target
            .kind
            .iter()
            .any(|kind| kind == "test" || kind == "lib" || kind == "bin")
}

/// Whether the temporary directory has room for the snapshots and target directories a run makes.
fn disk_check(temp: &Path) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well, Warn};
    const GIB: u64 = 1024 * 1024 * 1024;
    let Some(free) = free_space(temp) else {
        return doctor_report::Check::new(
            "disk",
            Warn,
            &format!("how much room {} has could not be read", temp.display()),
            None,
        );
    };
    let detail = format!("{} free under {}", rendered_bytes(free), temp.display());
    let standing = if free < GIB / 4 {
        Fail
    } else if free < GIB {
        Warn
    } else {
        Well
    };
    doctor_report::Check::new(
        "disk",
        standing,
        &detail,
        (standing != Well).then_some("free some room, or point TMPDIR at a filesystem that has it"),
    )
}

/// How many bytes the filesystem holding `path` will still take.
#[cfg(unix)]
fn free_space(path: &Path) -> Option<u64> {
    let statistics = rustix::fs::statvfs(path).ok()?;
    let block = if statistics.f_frsize == 0 {
        statistics.f_bsize
    } else {
        statistics.f_frsize
    };
    Some(block.saturating_mul(statistics.f_bavail))
}

/// How many bytes the filesystem holding `path` will still take.
#[cfg(not(unix))]
const fn free_space(_path: &Path) -> Option<u64> {
    None
}

/// Bytes as a person reads them.
fn rendered_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut whole = bytes;
    let mut remainder: u64 = 0;
    let mut unit: usize = 0;
    while whole >= 1024 && unit.saturating_add(1) < UNITS.len() {
        remainder = whole.wrapping_rem(1024);
        whole = whole.wrapping_div(1024);
        unit = unit.saturating_add(1);
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 {
        return format!("{whole} {name}");
    }
    let tenths = remainder.saturating_mul(10).wrapping_div(1024);
    format!("{whole}.{tenths} {name}")
}

/// A check that carries no remedy because nothing is wrong with it.
fn noted(name: &str, standing: doctor_report::Standing, detail: &str) -> doctor_report::Check {
    doctor_report::Check::new(name, standing, detail, None)
}

/// Whether the root is the workspace, which is what a run measures.
///
/// The check reads the manifests rather than asking cargo: a doctor answers
/// about a tree that may not build, and `cargo metadata` on a tree that does
/// not resolve says nothing about where the workspace is.
fn workspace_check(root: &Path, manifest: &Path) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well};
    if !manifest.is_file() {
        return doctor_report::Check::new(
            "workspace",
            Fail,
            &format!("{} is not there", manifest.display()),
            Some("run inside a cargo workspace, or pass --root at one"),
        );
    }
    let own = std::fs::read_to_string(manifest).unwrap_or_default();
    if own
        .lines()
        .any(|line| line.trim_start().starts_with("[workspace"))
    {
        return doctor_report::Check::new("workspace", Well, &manifest.display().to_string(), None);
    }
    let above = root.ancestors().skip(1).find(|directory| {
        std::fs::read_to_string(directory.join("Cargo.toml")).is_ok_and(|text| {
            text.lines()
                .any(|line| line.trim_start().starts_with("[workspace"))
        })
    });
    above.map_or_else(
        || doctor_report::Check::new("workspace", Well, &manifest.display().to_string(), None),
        |found| {
            doctor_report::Check::new(
                "workspace",
                Fail,
                &format!("{} is a member of {}", root.display(), found.display()),
                Some("run with --root at the workspace root, and --package to narrow it"),
            )
        },
    )
}

/// Whether a reserved variable is already set, which would make every answer a run gives an answer about something else.
fn environment_check(environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well};
    let set: Vec<String> = environment
        .vars
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .filter(|name| RESERVED_ENV.contains(&name.as_str()))
        .collect();
    if set.is_empty() {
        return doctor_report::Check::new("environment", Well, "no reserved variable is set", None);
    }
    doctor_report::Check::new(
        "environment",
        Fail,
        &format!("{} is set", set.join(", ")),
        Some("unset it: a run composes the activation itself"),
    )
}

/// Where what earlier runs established is kept, and how much of it there is.
fn cache_check(environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let store = crate::outcomes::Store::new(&environment.cache_directory);
    let (records, bytes) = store.size();
    let writable = std::fs::create_dir_all(store.root()).is_ok();
    if writable {
        doctor_report::Check::new(
            "cache",
            Well,
            &format!(
                "{} ({records} records, {bytes} bytes)",
                store.root().display()
            ),
            None,
        )
    } else {
        doctor_report::Check::new(
            "cache",
            Warn,
            &format!("{} cannot be written", store.root().display()),
            Some("set XDG_CACHE_HOME, or pass --cache-dir, or run with --no-cache"),
        )
    }
}

/// What earlier runs left in the temporary directory, and what a run kept on purpose.
fn snapshots_check(root: &Path, environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let ledger = crate::kept::Ledger::read(&root.join(crate::config::DEFAULT_REPORTS_DIRECTORY));
    let abandoned = std::fs::read_dir(&environment.temp_directory)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.starts_with(snapshot::DIR_PREFIX)
                        || name.starts_with(workspace::TARGET_DIR_PREFIX)
                })
                .count()
        })
        .unwrap_or_default();
    if abandoned == 0 && ledger.kept.is_empty() {
        return doctor_report::Check::new("snapshots", Well, "nothing is left over", None);
    }
    doctor_report::Check::new(
        "snapshots",
        Warn,
        &format!(
            "{abandoned} directories under {}, {} kept on purpose",
            environment.temp_directory.display(),
            ledger.kept.len()
        ),
        Some(
            "`cache --gc` removes what is abandoned, `--gc --all` the build caches, `--gc --kept` what was kept",
        ),
    )
}

/// Whether the LLVM tools the coverage layer needs are installed.
///
/// Coverage routing fails open — a measurement it cannot make routes every
/// mutation everywhere — so a missing component is a warning about how much a
/// run will cost, never a reason not to run.
fn llvm_tools_check(toolchain: Option<&rust_mutants::cargo::Toolchain>) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let advise = Some("rustup component add llvm-tools");
    let Some(toolchain) = toolchain else {
        return doctor_report::Check::new(
            "llvm-tools",
            Warn,
            "there is no toolchain to look in",
            advise,
        );
    };
    let printed = std::process::Command::new(toolchain.rustc())
        .arg("--print")
        .arg("target-libdir")
        .output();
    let Ok(printed) = printed else {
        return doctor_report::Check::new(
            "llvm-tools",
            Warn,
            "rustc could not say where its libraries are",
            advise,
        );
    };
    let libdir = PathBuf::from(String::from_utf8_lossy(&printed.stdout).trim().to_owned());
    let profdata = libdir.parent().map(|parent| {
        parent.join("bin").join(if cfg!(windows) {
            "llvm-profdata.exe"
        } else {
            "llvm-profdata"
        })
    });
    profdata.filter(|path| path.is_file()).map_or_else(
        || {
            doctor_report::Check::new(
                "llvm-tools",
                Warn,
                "llvm-profdata is not in the toolchain's sysroot, so coverage routing falls back \
                 to every target",
                advise,
            )
        },
        |path| doctor_report::Check::new("llvm-tools", Well, &path.display().to_string(), None),
    )
}

/// What `doctor` was asked, and how it answers.
#[derive(Debug, Clone, Copy)]
struct Asked<'a> {
    root: Option<&'a Path>,
    packages: &'a [String],
    json: bool,
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
        crate::tui::browse(document).map_err(|error| CliError::writing(&path, error))?;
        return Ok(code);
    }
    let projected = match format {
        cli::Format::Lines | cli::Format::Json => report::lines(&document),
        cli::Format::Html => html::document(&document),
        cli::Format::Stryker => json_line(&stryker::project(&document, &root)),
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

/// The newest stored run, by the pointer the last run wrote, or by name when there is no pointer.
fn newest(directory: &Path) -> Result<PathBuf, CliError> {
    let missing = || CliError::ReportMissing {
        message: format!("no run report is stored under {}", directory.display()),
    };
    let pointer = directory.join(run_report::LATEST_FILE_NAME);
    if let Ok(text) = std::fs::read_to_string(&pointer)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(relative) = value.get("document").and_then(serde_json::Value::as_str)
    {
        let path = directory.join(relative);
        if path.is_file() {
            return Ok(path);
        }
    }
    let entries = std::fs::read_dir(directory).map_err(|_error| missing())?;
    let mut runs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(run_report::FILE_NAME))
        .filter(|path| path.is_file())
        .collect();
    runs.sort();
    runs.pop().ok_or_else(missing)
}

/// What a `cache` command was asked to do.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one switch a person sets on the command line, and a switch is a bool \
              wherever it is stored"
)]
struct Sweeping<'a> {
    /// The workspace root, whose report directory holds the ledger of what was kept.
    root: Option<&'a Path>,
    /// Remove what is abandoned rather than only saying how much there is.
    gc: bool,
    /// Remove every build cache no live run has locked, not only the unowned ones.
    all: bool,
    /// Remove the directories a run was asked to keep, too.
    kept: bool,
    /// Empty the store of what earlier runs established.
    clear_outcomes: bool,
    /// Where the store is, when it is not under the user's cache directory.
    cache_dir: Option<&'a Path>,
}

fn cache(
    asked: &Sweeping<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let parent = &environment.temp_directory;
    let store = crate::outcomes::Store::new(
        asked
            .cache_dir
            .unwrap_or(environment.cache_directory.as_path()),
    );
    if asked.clear_outcomes {
        let (records, bytes) = store.clear();
        write(
            stdout,
            &format!(
                "outcomes    {} removed, {bytes} bytes, from {}\n",
                records,
                store.root().display()
            ),
        );
        return Ok(0);
    }
    let now = Timestamp::now();
    let scratch = [snapshot::DIR_PREFIX];
    let caches = [workspace::TARGET_DIR_PREFIX];
    let nothing = |_dir: &Path| Ok(());
    let (left, taken) = match (asked.gc, asked.all) {
        (true, true) => (
            tempowner::sweep(parent, &scratch, now),
            tempowner::reclaim(parent, &caches, now),
        ),
        (true, false) => (
            tempowner::sweep(parent, &scratch, now),
            tempowner::reclaim_with(parent, &caches, now, &nothing),
        ),
        (false, _) => (
            tempowner::sweep_with(parent, &scratch, now, &nothing),
            tempowner::reclaim_with(parent, &caches, now, &nothing),
        ),
    };
    let left = left.map_err(|source| CliError::writing(parent, source))?;
    let taken = taken.map_err(|source| CliError::writing(parent, source))?;
    let (records, bytes) = store.size();
    let mut text = String::new();
    let verb = if asked.gc { "removed" } else { "reclaimable" };
    let caches_verb = if asked.gc && asked.all {
        "removed"
    } else {
        "reclaimable"
    };
    let written = write!(
        text,
        "temp        {}\ncaches      {} {}, {} bytes; {} still in use\nsnapshots   {} {}, {} bytes; {} still in use, {} preserved on purpose\noutcomes    {} records, {} bytes, at {}\nfailures    {}\n",
        parent.display(),
        taken.removed.len(),
        caches_verb,
        taken.removed_bytes,
        taken.live,
        left.removed.len(),
        verb,
        left.removed_bytes,
        left.live,
        left.kept,
        records,
        bytes,
        store.root().display(),
        left.failures.len().saturating_add(taken.failures.len()),
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    write(stdout, &text);
    write(stdout, &preserved(asked, environment)?);
    Ok(0)
}

/// The directories runs were asked to keep, listed or removed.
fn preserved(asked: &Sweeping<'_>, environment: &Environment) -> Result<String, CliError> {
    let root = asked
        .root
        .map_or_else(|| environment.working_directory.clone(), Path::to_path_buf);
    let directory = root.join(crate::config::DEFAULT_REPORTS_DIRECTORY);
    if asked.kept {
        let (removed, _empty) = crate::kept::Ledger::clear(&directory)
            .map_err(|source| CliError::writing(&directory, source))?;
        return Ok(format!("kept        {removed} removed\n"));
    }
    let ledger = crate::kept::Ledger::read(&directory);
    let mut text = format!("kept        {}\n", ledger.kept.len());
    for entry in &ledger.kept {
        let written = writeln!(
            text,
            "            {} ({})",
            entry.path.display(),
            entry.run_id
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    Ok(text)
}

/// One file as the engine rewrites it.
fn instrumented(
    workspace: &Workspace,
    discovery: &rust_mutants::discover::Discovery,
    (path, mutant): (&str, Option<&str>),
) -> Result<String, CliError> {
    use rust_mutants::instrument::{instrument_file, plan_file};
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
    let file = instrument_file(path, &source, &placements, discovery.catalog.digest())
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

fn write(stream: &mut dyn Write, text: &str) {
    let _written = stream
        .write_all(text.as_bytes())
        .and_then(|()| stream.flush());
}

/// The pristine text of every file that yielded a candidate, so a position can be counted in the file a person would open.
fn read_sources(
    root: &Path,
    discovery: &rust_mutants::discover::Discovery,
) -> std::collections::BTreeMap<String, String> {
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
/// ([ADR 0013](../../../docs/adr/0013-codegen-identity-is-the-equivalence-proof.md)).
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
