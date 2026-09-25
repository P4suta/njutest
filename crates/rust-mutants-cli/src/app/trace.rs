// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `--trace` and the `trace` readings: where a recording goes, and what one says afterwards.

use std::io::Write;
use std::path::{Path, PathBuf};

use rust_mutants::id::RunId;
use rust_mutants::runner::Cancel;
use rust_mutants::trace::summary::{Summary, diff, render, summarize};
use rust_mutants::trace::{
    ChannelSink, DirSink, Event, FILE_NAME, Problem, ReadError, Recorder, Sink, TraceContext,
    check, read_events,
};

use crate::error::CliError;
use crate::filesystem::{EntryKind, entry_kind};
use crate::settings::Settings;
use crate::{Environment, cli};

/// The directory a run keeps its own recording in, beside its report.
pub const RUN_DIRECTORY_NAME: &str = "trace";

/// The directory every recording that is not a run's own is kept under.
pub const TRACES_DIRECTORY_NAME: &str = "traces";

/// The recording a command was asked for, or the one that records nothing.
///
/// # Errors
/// Returns the filesystem failure when a requested recording directory cannot be created.
pub fn recorder(
    wanted: &Recording<'_>,
    progress: Option<std::sync::mpsc::SyncSender<Event>>,
) -> Result<Recorder, CliError> {
    let watching = progress.map(ChannelSink::new);
    let context = TraceContext::Standalone {
        run_id: wanted.id.clone(),
        build_selection: wanted
            .settings
            .config
            .build
            .config()
            .selection()
            .digest()
            .clone(),
    };
    let Some(asked) = wanted.scope.trace.as_deref() else {
        return Ok(match watching {
            Some(watching) => Recorder::wall(Sink::Channel(watching), context),
            None => Recorder::disabled(),
        });
    };
    let directory = if asked.is_empty() {
        default_directory(wanted)
    } else {
        PathBuf::from(asked)
    };
    let kept = created(&directory).map_err(|source| CliError::writing(&directory, source))?;
    Ok(match watching {
        Some(watching) => Recorder::wall(Sink::required_with_channel(kept, watching), context),
        None => Recorder::wall(Sink::required(kept), context),
    })
}

/// The recording's own directory, with the directories above it made first.
fn created(directory: &Path) -> std::io::Result<DirSink> {
    if let Some(parent) = directory.parent() {
        std::fs::create_dir_all(parent)?;
    }
    DirSink::create(directory)
}

/// What a command was asked to record, and under what name.
#[derive(Debug, Clone, Copy)]
pub struct Recording<'a> {
    /// What the command line asked of the workspace, `--trace` included.
    pub scope: &'a cli::Scope,
    /// Where reports go, which is what a default recording is placed against.
    pub settings: &'a Settings,
    /// The name of this run.
    pub id: &'a RunId,
    /// The command, which names its own recording when it is not a run.
    pub command: &'a cli::Command,
}

/// Where a command records when `--trace` named no directory.
fn default_directory(wanted: &Recording<'_>) -> PathBuf {
    let reports = wanted.settings.report_directory();
    if matches!(wanted.command, cli::Command::Run { .. }) {
        reports.join(wanted.id.as_str()).join(RUN_DIRECTORY_NAME)
    } else {
        reports
            .join(TRACES_DIRECTORY_NAME)
            .join(format!("{}-{}", wanted.id, named(wanted.command)))
    }
}

/// What a command is called in the name of the directory it records into.
const fn named(command: &cli::Command) -> &'static str {
    match command {
        cli::Command::Run { .. } => "run",
        cli::Command::List { .. } => "list",
        cli::Command::Catalog { .. } => "catalog",
        cli::Command::Explain { .. } => "explain",
        cli::Command::Replay { .. } => "replay",
        cli::Command::Instrument { .. } => "instrument",
        cli::Command::WhySkipped { .. } => "why-skipped",
        cli::Command::Equivalence { .. } => "equivalence",
        cli::Command::Init { .. }
        | cli::Command::Diagnostics { .. }
        | cli::Command::Rules { .. }
        | cli::Command::Doctor { .. }
        | cli::Command::Report { .. }
        | cli::Command::Merge { .. }
        | cli::Command::Trace { .. }
        | cli::Command::Cache { .. }
        | cli::Command::Ci { .. } => "command",
    }
}

/// Closes the recording with how the command ended, so a reader can tell a run that finished from one that was killed.
///
/// # Errors
/// Returns the trace sink failure when the terminal event cannot be recorded completely.
pub fn ended(
    recorder: &Recorder,
    outcome: &Result<u8, CliError>,
    cancel: &Cancel,
) -> Result<(), CliError> {
    if !recorder.is_enabled() {
        return Ok(());
    }
    let error = match outcome {
        Ok(_) => None,
        Err(error) => Some(error.to_string()),
    };
    let verdict = if cancel.is_cancelled() {
        rust_mutants::trace::RunOutcome::Interrupted
    } else {
        match outcome {
            Ok(code) => verdict_of(*code),
            Err(_command_failure) => rust_mutants::trace::RunOutcome::Failed,
        }
    };
    recorder
        .run_end(verdict, error)
        .map_err(|source| CliError::writing(Path::new("<trace>"), source))
}

/// How a command ended, in the words the exit codes are named after.
const fn verdict_of(code: u8) -> rust_mutants::trace::RunOutcome {
    match code {
        crate::run::EXIT_DETECTED => rust_mutants::trace::RunOutcome::Detected,
        crate::run::EXIT_FOUND => rust_mutants::trace::RunOutcome::Found,
        crate::run::EXIT_INTERRUPTED => rust_mutants::trace::RunOutcome::Interrupted,
        _ => rust_mutants::trace::RunOutcome::Failed,
    }
}

/// Reads a recording back.
///
/// # Errors
/// Returns the recording that is not there, or that is not one.
pub fn read(
    command: &cli::TraceCommand,
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    match command {
        cli::TraceCommand::Summary {
            root,
            run,
            dir,
            slowest,
        } => {
            let found = load(
                &Where {
                    root: root.as_deref(),
                    run: run.as_deref(),
                    dir: dir.as_deref(),
                },
                environment,
            )?;
            say(stdout, &format!("RUN\t{}\n", found.name))?;
            let summary = summarize(&found.events, *slowest)
                .map_err(|source| CliError::TraceSummary { source })?;
            say(stdout, &render(&summary))?;
            Ok(0)
        }
        cli::TraceCommand::Check { root, run, dir } => {
            let found = load(
                &Where {
                    root: root.as_deref(),
                    run: run.as_deref(),
                    dir: dir.as_deref(),
                },
                environment,
            )?;
            say(stdout, &format!("RUN\t{}\n", found.name))?;
            let problems = check(&found.events);
            if problems.is_empty() {
                say(
                    stdout,
                    "COMPLETE\tthe recording begins, ends, and lost nothing\n",
                )?;
                return Ok(0);
            }
            for problem in &problems {
                say(stdout, &format!("PROBLEM\t{}\n", describe(problem)))?;
            }
            Ok(1)
        }
        cli::TraceCommand::Diff { root, a, b } => {
            let before = summary(root.as_deref(), a, environment)?;
            let after = summary(root.as_deref(), b, environment)?;
            say(stdout, &format!("A\t{a}\nB\t{b}\n"))?;
            for change in diff(&before, &after) {
                say(
                    stdout,
                    &format!("CHANGED\t{}\t{}\t{}\n", change.what, change.from, change.to),
                )?;
            }
            Ok(0)
        }
    }
}

/// Which recording to read.
#[derive(Debug, Clone, Copy)]
struct Where<'a> {
    root: Option<&'a Path>,
    run: Option<&'a str>,
    dir: Option<&'a Path>,
}

/// A recording, by the name a reader would call it.
struct Found {
    name: String,
    events: Vec<Event>,
}

/// One recording summarised, by run name.
fn summary(root: Option<&Path>, run: &str, environment: &Environment) -> Result<Summary, CliError> {
    let found = load(
        &Where {
            root,
            run: Some(run),
            dir: None,
        },
        environment,
    )?;
    summarize(&found.events, 0).map_err(|source| CliError::TraceSummary { source })
}

/// The recording a reading names: a directory given outright, a named run, or the newest one stored.
fn load(wanted: &Where<'_>, environment: &Environment) -> Result<Found, CliError> {
    let (name, directory) = if let Some(dir) = wanted.dir {
        (dir.display().to_string(), dir.to_path_buf())
    } else {
        stored(wanted, environment)?
    };
    let path = directory.join(FILE_NAME);
    let file = std::fs::File::open(&path).map_err(|error| CliError::ReportMissing {
        message: format!("{}: {error}", path.display()),
    })?;
    let events =
        read_events(std::io::BufReader::new(file)).map_err(|error| CliError::ReportMissing {
            message: format!("{}: {}", path.display(), said(&error)),
        })?;
    Ok(Found { name, events })
}

/// The recording a name asks for, or the newest one, out of what is stored.
fn stored(wanted: &Where<'_>, environment: &Environment) -> Result<(String, PathBuf), CliError> {
    let reports = reports_directory(wanted.root, environment)?;
    let kept = recordings(&reports)?;
    let found = match wanted.run {
        Some(run) => kept
            .into_iter()
            .find(|(name, _)| name == run || name.starts_with(&format!("{run}-"))),
        None => kept.into_iter().next_back(),
    };
    found.ok_or_else(|| CliError::ReportMissing {
        message: wanted.run.map_or_else(
            || format!("no recording is stored under {}", reports.display()),
            |run| {
                format!(
                    "no recording named {run} is stored under {}",
                    reports.display()
                )
            },
        ),
    })
}

/// Where stored runs and recordings are kept.
fn reports_directory(root: Option<&Path>, environment: &Environment) -> Result<PathBuf, CliError> {
    let root = environment.rooted(root);
    let config = crate::config::Config::load(&root)?;
    Ok(super::stored::Store::of(&root, &config.reports.directory).root())
}

/// Every stored recording, oldest first: a run's own beside its report, and every other command's under `traces/`.
fn recordings(reports: &Path) -> Result<Vec<(String, PathBuf)>, CliError> {
    let mut found = Vec::new();
    for entry in directory_entries(reports)? {
        let path = entry.path();
        let kind = entry_kind(&path).map_err(|source| CliError::StoredRunsUnreadable {
            path: path.clone(),
            source,
        })?;
        if kind != EntryKind::Directory {
            continue;
        }
        let name = super::stored_spelling(&path)?;
        if name == TRACES_DIRECTORY_NAME {
            continue;
        }
        let recording = path.join(RUN_DIRECTORY_NAME);
        if stored_file(&recording.join(FILE_NAME))? {
            found.push((name, recording));
        }
    }
    let traces = reports.join(TRACES_DIRECTORY_NAME);
    for entry in directory_entries(&traces)? {
        let path = entry.path();
        if stored_file(&path.join(FILE_NAME))? {
            let name = super::stored_spelling(&path)?;
            found.push((name, path));
        }
    }
    found.sort();
    Ok(found)
}

fn stored_file(path: &Path) -> Result<bool, CliError> {
    match entry_kind(path).map_err(|source| CliError::StoredRunsUnreadable {
        path: path.to_path_buf(),
        source,
    })? {
        EntryKind::File => Ok(true),
        EntryKind::Missing | EntryKind::Directory | EntryKind::Other => Ok(false),
    }
}

fn directory_entries(directory: &Path) -> Result<Vec<std::fs::DirEntry>, CliError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(CliError::StoredRunsUnreadable {
                path: directory.to_path_buf(),
                source,
            });
        }
    };
    entries
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|source| CliError::StoredRunsUnreadable {
            path: directory.to_path_buf(),
            source,
        })
}

/// What is wrong with a recording, as one line.
fn describe(problem: &Problem) -> String {
    match problem {
        Problem::MissingRunStart => "the recording does not begin with run-start".to_owned(),
        Problem::MissingRunEnd => {
            "the recording does not end with run-end: the run was killed, or the end was lost"
                .to_owned()
        }
        Problem::SequenceGap { expected, found } => {
            format!("sequence {expected} is missing; the next event is {found}")
        }
        Problem::Dropped(count) => format!("the run admits it dropped {count} events"),
        Problem::UnbalancedPhase { name } => {
            format!("the phase {name} began and ended a different number of times")
        }
    }
}

/// Why a recording could not be read.
fn said(error: &ReadError) -> String {
    error.to_string()
}

/// A closed stream is the reader's choice, not a failure of ours.
fn say(stream: &mut dyn Write, text: &str) -> Result<(), CliError> {
    super::write(stream, text)
}
