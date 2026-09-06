// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands: what each one opens, what it establishes, and what it writes.

pub mod trace;

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rust_mutants::EngineError;
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
    let recorder = trace::recorder(
        &trace::Recording {
            scope,
            settings: &settings,
            id: &id,
            command,
        },
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
        },
        stdout,
        cancel,
    );
    trace::ended(&recorder, &outcome, cancel);
    if recorder.is_enabled() {
        prune(&settings.report_directory(), settings.config.reports.keep);
    }
    outcome
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
    } = *running;
    let workspace = Workspace::open(
        &settings.root,
        settings.open_options(scope, environment, recorder.clone())?,
        cancel,
    )?;
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
                    environment,
                    id,
                    started,
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
    environment: &'a Environment,
    id: &'a str,
    started: Timestamp,
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
                json_line(&report::document(session, &settings.config))
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
                    args,
                    shard: shard.as_deref(),
                    no_report: *no_report,
                    no_cache: *no_cache,
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
    args: &'a [String],
    shard: Option<&'a str>,
    no_report: bool,
    no_cache: bool,
    environment: &'a Environment,
    id: &'a str,
    started: Timestamp,
}

fn whole(
    session: &Session,
    whole: &Whole<'_>,
    cancel: &Cancel,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    let Whole {
        settings,
        args,
        shard,
        no_report,
        no_cache,
        environment,
        id,
        started,
    } = *whole;
    let shard = shard.map(run::Shard::parse).transpose()?;
    let outcomes = crate::outcomes::Store::new(&environment.cache_directory);
    let keyed = crate::outcomes::Keyed {
        workspace: session.workspace_digest().to_owned(),
        catalog: session.catalog().digest().to_owned(),
        args: args.to_vec(),
        timeout: crate::config::render_timeout(settings.config.mutation.timeout),
        build: settings.config.build.config().arguments(),
    };
    let mut result = run::run(
        session,
        &run::Options {
            quiet: &rust_mutants::run::Quiet::default(),
            expectations: &settings.config.mutation.expect,
            args,
            shard,
            outcomes: (!no_cache).then_some(run::Reusing {
                store: &outcomes,
                keyed: &keyed,
                run_id: id,
            }),
        },
        cancel,
        &mut |judged, position, total| {
            let mut line = String::new();
            let written = writeln!(
                line,
                "[{position}/{total}] {} {}",
                judged.display_id,
                judged.outcome.name()
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
            write(stdout, &line);
        },
    )?;
    result.expectations = run::verify(
        session,
        &settings.config.mutation.expect,
        &mut result.judged,
    );
    let finished = Timestamp::now();
    let document = run_report::document(
        session,
        &result,
        report::selection_document(&settings.config),
        &run_report::Meta {
            id,
            started_at: started,
            finished_at: finished,
        },
    );
    write(stdout, "\n");
    write(stdout, &run_report::lines(&document));
    if !no_report {
        let written = store(&settings.report_directory(), id, &document)?;
        let mut line = String::new();
        let ok = writeln!(line, "REPORT    {}", written.display());
        debug_assert!(ok.is_ok(), "writing to a String cannot fail");
        write(stdout, &line);
    }
    prune(&settings.report_directory(), settings.config.reports.keep);
    Ok(document.run.exit_code)
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
        cli::Format::Lines | cli::Format::Json => run_report::lines(&document),
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
            write(stdout, &run_report::lines(&merged));
        }
        None => write(stdout, &text),
    }
    Ok(merged.run.exit_code)
}

/// A closed stream is the reader's choice, not a failure of ours.
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
