// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command line of the rust-mutants engine.
//!
//! [`run_from`] is the whole surface: it takes the argument vector, the
//! environment, and the two output streams as arguments so a test can drive
//! it without a process, and returns the exit code `main` hands to the
//! operating system.

#![forbid(unsafe_code)]

pub mod cli;
pub mod report;

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use rust_mutants::EngineError;
use rust_mutants::glob::Pattern;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{self, PrepareOptions, Request, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

/// The exit code of a usage error or an infrastructure failure.
pub const EXIT_USAGE: u8 = 2;

/// Everything the command line needs from the process it runs in.
///
/// It is an argument because nothing below the composition root may read the
/// process environment ([ADR 0001]): `main.rs` fills this in, and a test
/// fills it in with whatever it wants the run to see.
///
/// [ADR 0001]: https://github.com/P4suta/mjutest/blob/main/docs/adr/0001-seam-policy.md
#[derive(Debug)]
pub struct Environment {
    /// The process environment, which the engine hands to every command and
    /// test process it starts.
    pub vars: Vec<(OsString, OsString)>,
    /// The directory snapshots and target directories are created in.
    pub temp_directory: PathBuf,
    /// The working directory, which a command with no `--root` reads.
    pub working_directory: PathBuf,
}

/// Runs the command line described by `args` (program name first) and
/// returns its exit code, writing to the two streams it was given.
///
/// Exit codes: `0` the command did what it was asked (and, for `run`, the
/// tests noticed the mutant), `1` the mutant survived, `2` a usage error or
/// an infrastructure failure.
pub fn run_from<I>(
    args: I,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            // A closed stream is the reader's choice, not a failure of ours.
            let _written = stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush());
            return ExitCode::from(usage.exit_code);
        }
    };
    match dispatch(&command.command, environment, stdout) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let _written = writeln!(stderr, "rust-mutants: {error}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

/// Opens the workspace a command names and does what it asks.
fn dispatch(
    command: &cli::Command,
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, EngineError> {
    let scope = command.scope();
    let cancel = Cancel::new();
    let root = scope
        .root
        .clone()
        .unwrap_or_else(|| environment.working_directory.clone());
    let workspace = Workspace::open(&root, open_options(scope, environment)?, &cancel)?;
    let options = prepare_options(scope)?;

    match command {
        cli::Command::List { .. } => {
            let discovery = session::preview(&workspace, &options, &cancel)?;
            let sources = read_sources(workspace.snapshot_root(), &discovery);
            write(stdout, &report::list(&discovery, &sources));
            workspace.close()?;
            Ok(0)
        }
        cli::Command::WhySkipped { .. } => {
            let discovery = session::preview(&workspace, &options, &cancel)?;
            write(stdout, &report::why_skipped(&discovery.skips));
            workspace.close()?;
            Ok(0)
        }
        cli::Command::Instrument { file, .. } => {
            let discovery = session::preview(&workspace, &options, &cancel)?;
            let text = instrumented(&workspace, &discovery, file)?;
            write(stdout, &text);
            workspace.close()?;
            Ok(0)
        }
        cli::Command::Catalog { json, .. } => {
            let session = workspace.prepare(&options, &cancel)?;
            let text = if *json {
                let document = report::document(&session, scope);
                let mut text = serde_json::to_string_pretty(&document)
                    .unwrap_or_else(|error| format!("{{\"error\":{error:?}}}"));
                text.push('\n');
                text
            } else {
                report::catalog(&session)
            };
            write(stdout, &text);
            session.close()?;
            Ok(0)
        }
        cli::Command::Explain { mutant, .. } => {
            let session = workspace.prepare(&options, &cancel)?;
            let found = session.resolve(mutant)?.clone();
            let source = read_source(&session, &found.candidate.path);
            write(
                stdout,
                &report::explain(&session, &found, source.as_deref()),
            );
            session.close()?;
            Ok(0)
        }
        cli::Command::Run {
            mutant,
            target,
            test,
            args,
            ..
        } => {
            let session = workspace.prepare(&options, &cancel)?;
            let found = session.resolve(mutant)?.clone();
            let result = session.exec(
                &Request {
                    mutant: mutant.clone(),
                    target: target.clone(),
                    test: test.clone(),
                    args: args.clone(),
                    timeout: None,
                },
                &cancel,
            )?;
            write(stdout, &report::outcome(&result, &found));
            let code = report::exit_code(result.outcome);
            session.close()?;
            Ok(code)
        }
    }
}

/// One file as the engine rewrites it.
fn instrumented(
    workspace: &Workspace,
    discovery: &rust_mutants::discover::Discovery,
    path: &str,
) -> Result<String, EngineError> {
    use rust_mutants::instrument::{instrument_file, plan_file};
    use rust_mutants::workspace::SessionError;

    let found: Vec<rust_mutants::syntax::Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let source = std::fs::read(workspace.snapshot_root().join(path)).map_err(|source| {
        SessionError::WriteFailed {
            path: path.to_owned(),
            source,
        }
    })?;
    let placements = plan_file(&discovery.catalog, path, &found)?;
    let file = instrument_file(path, &source, &placements, discovery.catalog.digest())?;
    Ok(file.text)
}

/// A closed stream is the reader's choice, not a failure of ours.
fn write(stream: &mut dyn Write, text: &str) {
    let _written = stream
        .write_all(text.as_bytes())
        .and_then(|()| stream.flush());
}

fn open_options(scope: &cli::Scope, environment: &Environment) -> Result<OpenOptions, EngineError> {
    Ok(OpenOptions {
        cargo: None,
        search_path: environment
            .vars
            .iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.clone()),
        env: environment.vars.clone(),
        temp_directory: environment.temp_directory.clone(),
        report_directory: None,
        exclude: compile_patterns(&scope.exclude)?,
        keep_temp: scope.switches.keep_temp,
        offline: scope.switches.offline,
        locked: scope.switches.locked,
        trace: rust_mutants::trace::Recorder::disabled(),
    })
}

fn prepare_options(scope: &cli::Scope) -> Result<PrepareOptions, EngineError> {
    Ok(PrepareOptions {
        tier: scope.tier.tier(),
        operators: scope.operators.clone(),
        include: compile_patterns(&scope.include)?,
        exclude: compile_patterns(&scope.exclude)?,
        packages: scope.packages.clone(),
        verify: !scope.switches.no_verify,
        ..PrepareOptions::default()
    })
}

fn compile_patterns(patterns: &[String]) -> Result<Vec<Pattern>, EngineError> {
    patterns
        .iter()
        .map(|pattern| Pattern::compile(pattern).map_err(EngineError::from))
        .collect()
}

/// The pristine text of every file that yielded a candidate, so a position
/// can be counted in the file a person would open.
fn read_sources(
    root: &std::path::Path,
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

/// The text of one file of a prepared session. It is the instrumented text,
/// which is why `explain` reads a position from the catalog's own span
/// rather than trusting it blindly.
fn read_source(session: &Session, path: &str) -> Option<String> {
    std::fs::read_to_string(session.snapshot_root().join(path)).ok()
}
