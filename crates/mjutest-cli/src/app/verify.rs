// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest verify`: run a verification and say what it concluded.

use std::io::Write;
use std::path::{Path, PathBuf};

use jiff::Timestamp;

use crate::app::reports;
use crate::assure::run::{self, Request};
use crate::build::Cargo;
use crate::cli::{EXIT_ERROR, Environment, Verify};
use crate::config::Config;
use crate::report::lines;
use crate::run_id;
use crate::trace::{DirSink, Recorder, Sink, StartRecord};
use crate::ui;
use crate::watch::Watch;

/// Runs a verification.
pub fn run(
    arguments: &Verify,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = arguments
        .directory
        .clone()
        .unwrap_or_else(|| environment.working_directory.clone());
    let config = match load(arguments, &root) {
        Ok(config) => config,
        Err(message) => {
            super::diagnose(stderr, &message);
            return EXIT_ERROR;
        }
    };

    let started = Timestamp::now();
    let identity = run_id::mint(started, std::process::id());
    let trace = recorder(
        arguments,
        &Recording {
            root: &root,
            identity: &identity,
            contract: config.contract,
        },
        stderr,
    );
    let cancel = rust_mutants::runner::Cancel::new();
    let watch = Watch::new(&cancel, &trace);
    let request = Request {
        root: root.clone(),
        config,
        packages: arguments.packages.clone(),
        test_args: arguments.test_args.clone(),
        cargo: Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
        keep_temp: arguments.keep_temp,
        run_id: identity,
        started,
    };
    let result = {
        let mut notes = ui::Notes::of(arguments.ui, stderr);
        run::run(&request, environment, &mut notes, watch)
    };
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            trace.run_end("ERROR", None, Some(error.to_string()));
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };

    let report = outcome.report;
    trace.run_end(
        &lines::escape(&format!("{:?}", report.verdict)),
        Some(report.accounting),
        None,
    );
    let kept = match reports::keep(&root, &report) {
        Ok(kept) => kept,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let removed = reports::retain(&root, request_keep(&request));
    {
        let mut notes = ui::Notes::of(arguments.ui, stderr);
        notes.note("report", &kept.document.display().to_string());
        for path in &removed {
            notes.note("retired", &path.display().to_string());
        }
        for path in &outcome.kept {
            notes.note("kept", &path.display().to_string());
        }
    }

    let _written = stdout.write_all(lines::stream(&report).as_bytes());
    report.verdict.exit_code()
}

/// How many run directories to keep.
const fn request_keep(request: &Request) -> u32 {
    request.config.reports.keep
}

/// The configuration this run answers to.
fn load(arguments: &Verify, root: &Path) -> Result<Config, String> {
    match &arguments.config {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("reading {}: {error}", path.display()))?;
            Config::parse(&text, path).map_err(|error| error.to_string())
        }
        None => Config::load(root).map_err(|error| error.to_string()),
    }
}

/// What a recording is named after: the run it belongs to.
struct Recording<'a> {
    root: &'a Path,
    identity: &'a str,
    contract: crate::config::Contract,
}

/// The recording this run keeps.
fn recorder(arguments: &Verify, run: &Recording<'_>, stderr: &mut dyn Write) -> Recorder {
    let (root, identity, contract) = (run.root, run.identity, run.contract);
    let start = StartRecord::of(identity, crate::report::RunKind::Full, contract);
    let ring = Sink::ring();
    let Some(requested) = &arguments.trace else {
        return Recorder::wall(ring, start);
    };
    let directory = if requested.is_empty() {
        root.join(".mjutest/trace").join(identity)
    } else {
        PathBuf::from(requested)
    };
    match DirSink::create(&directory) {
        Ok(sink) => Recorder::wall(Sink::Tee(vec![ring, Sink::Dir(sink)]), start),
        Err(error) => {
            let _written = writeln!(
                stderr,
                "{}: the trace could not be written to {}: {error}",
                crate::cli::PROGRAM,
                directory.display()
            );
            Recorder::wall(ring, start)
        }
    }
}
