// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands: what each one opens, what it establishes, and what it writes.

pub mod trace;

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::EngineError;
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
    reserved(environment)?;
    match command {
        cli::Command::Init { root, force } => init(root.as_deref(), *force, environment, stdout),
        cli::Command::Doctor { root, json } => Ok(doctor(
            &Asked {
                root: root.as_deref(),
                json: *json,
            },
            environment,
            stdout,
            cancel,
        )),
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
        cli::Command::Cache { gc } => cache(*gc, environment, stdout),
        cli::Command::Merge { reports, output } => merge(reports, output.as_deref(), stdout),
        cli::Command::Trace { command } => trace::read(command, environment, stdout),
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
    let id = run_id(started);
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
            session.close()?;
            code
        }
    }
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
        cli::Command::List { .. } => Ok(report::list(
            discovery,
            &read_sources(workspace.snapshot_root(), discovery),
        )),
        cli::Command::WhySkipped { .. } => Ok(report::why_skipped(&discovery.skips)),
        cli::Command::Instrument { file, .. } => instrumented(workspace, discovery, file),
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
        cli::Command::Catalog { json, .. } => {
            let text = if *json {
                json_line(&report::document(session, &settings.prepare_options()?))
            } else {
                report::catalog(session)
            };
            write(stdout, &text);
            Ok(0)
        }
        cli::Command::Explain { mutant, .. } => {
            let found = session.resolve(mutant)?.clone();
            let source = read_source(session, &found.candidate.path);
            write(stdout, &report::explain(session, &found, source.as_deref()));
            Ok(0)
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

/// The name of a run: the instant it started, which sorts chronologically as a directory name.
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
    let root = asked
        .root
        .map_or_else(|| environment.working_directory.clone(), Path::to_path_buf);
    let mut checks: Vec<doctor_report::Check> = Vec::new();
    let mut say = |label: &str, good: bool, detail: &str| {
        checks.push(doctor_report::Check {
            name: label.to_owned(),
            ok: good,
            detail: detail.to_owned(),
        });
    };

    let toolchain = rust_mutants::cargo::Toolchain::locate(
        &rust_mutants::cargo::LocateOptions {
            cargo: None,
            search_path: environment
                .vars
                .iter()
                .find(|(name, _)| name == "PATH")
                .map(|(_, value)| value.clone()),
            env: Some(environment.vars.clone()),
        },
        &root,
        cancel,
    );
    match &toolchain {
        Ok(found) => {
            say("cargo", true, &found.cargo_version().summary);
            say("rustc", true, &found.rustc_version().summary);
            say("host", true, found.host());
        }
        Err(error) => say("toolchain", false, &error.to_string()),
    }

    let manifest = root.join("Cargo.toml");
    say(
        "workspace",
        manifest.is_file(),
        &manifest.display().to_string(),
    );

    let config_path = root.join(crate::config::FILE_NAME);
    if config_path.is_file() {
        match crate::config::Config::load(&root) {
            Ok(_read) => say("config", true, &config_path.display().to_string()),
            Err(error) => say("config", false, &error.to_string()),
        }
    } else {
        say(
            "config",
            true,
            &format!(
                "none; the defaults apply. `rust-mutants init` writes {}",
                crate::config::FILE_NAME
            ),
        );
    }

    let temp = &environment.temp_directory;
    say(
        "temp",
        temp.is_dir(),
        &format!(
            "{} (snapshots as {}*, target directories as {}*)",
            temp.display(),
            snapshot::DIR_PREFIX,
            workspace::TARGET_DIR_PREFIX
        ),
    );
    let document = doctor_report::DoctorDocument::of(checks);
    let text = if asked.json {
        json_line(&document)
    } else {
        doctor_report::lines(&document)
    };
    write(stdout, &text);
    if document.ok { 0 } else { crate::EXIT_USAGE }
}

/// What `doctor` was asked, and how it answers.
#[derive(Debug, Clone, Copy)]
struct Asked<'a> {
    root: Option<&'a Path>,
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

fn cache(gc: bool, environment: &Environment, stdout: &mut dyn Write) -> Result<u8, CliError> {
    let parent = &environment.temp_directory;
    let now = Timestamp::now();
    let scratch = [snapshot::DIR_PREFIX];
    let caches = [workspace::TARGET_DIR_PREFIX];
    let nothing = |_dir: &Path| Ok(());
    let (left, taken) = if gc {
        (
            tempowner::sweep(parent, &scratch, now),
            tempowner::reclaim(parent, &caches, now),
        )
    } else {
        (
            tempowner::sweep_with(parent, &scratch, now, &nothing),
            tempowner::reclaim_with(parent, &caches, now, &nothing),
        )
    };
    let left = left.map_err(|source| CliError::writing(parent, source))?;
    let taken = taken.map_err(|source| CliError::writing(parent, source))?;
    let mut text = String::new();
    let verb = if gc { "removed" } else { "reclaimable" };
    let written = write!(
        text,
        "temp        {}\ncaches      {} {}, {} bytes; {} still in use\nsnapshots   {} {}, {} bytes; {} still in use, {} preserved on purpose\nfailures    {}\n",
        parent.display(),
        taken.removed.len(),
        verb,
        taken.removed_bytes,
        taken.live,
        left.removed.len(),
        verb,
        left.removed_bytes,
        left.live,
        left.kept,
        left.failures.len().saturating_add(taken.failures.len()),
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    write(stdout, &text);
    Ok(0)
}

/// One file as the engine rewrites it.
fn instrumented(
    workspace: &Workspace,
    discovery: &rust_mutants::discover::Discovery,
    path: &str,
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
    Ok(file.text)
}

fn json_line<T: serde::Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value)
        .unwrap_or_else(|error| format!("{{\"error\":{error:?}}}"));
    text.push('\n');
    text
}

/// Puts the reports of the parts of one catalog back together.
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
