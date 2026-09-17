// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest verify`: run a verification and say what it concluded.

use std::io::Write;
use std::path::{Path, PathBuf};

use jiff::Timestamp;

use crate::app::reports;
use crate::assure::identity::{self, Evidence};
use crate::assure::run::{self, Request};
use crate::build::Cargo;
use crate::cache::lock::{self, Lease};
use crate::cache::store::Store;
use crate::cli::{EXIT_ERROR, Environment, Verify};
use crate::config::Config;
use crate::evidence::digest::Mode;
use crate::report::lines;
use crate::run_id;
use crate::trace::{DirSink, Recorder, Sink, StartRecord};
use crate::ui;
use crate::watch::Watch;

/// Where scheduling state for interrupted runs lives, beside the answers finished runs left.
pub const CHECKPOINTS: &str = "checkpoints";

/// How long a run waits for another run of the same inputs before doing the work itself.
pub const LEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(30);

/// Runs a verification.
pub fn run(
    arguments: &Verify,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = environment.rooted(arguments.directory.as_deref());
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
    let cancel = environment.cancel.clone();

    let watch = Watch::new(&cancel, &trace);
    let changed = match asked_about(arguments, (&root, &config), environment, watch) {
        Ok(changed) => changed,
        Err(message) => {
            super::diagnose(stderr, &message);
            return EXIT_ERROR;
        }
    };
    let asked = Asked {
        root: &root,
        config: &config,
        environment,
        changed: changed.as_ref(),
    };
    let evidence = evidence_of(arguments, &asked, &cancel);
    let store = store_of(environment, &config);
    let _lease = match already_answered(
        &Asking {
            store: &store,
            identity: &evidence.identity,
            run_id: &identity,
            started,
            root: &root,
        },
        arguments,
        &cancel,
        Streams {
            out: stdout,
            err: stderr,
        },
    ) {
        Settled::Answered(code) => return code,
        Settled::Establish(lease) => lease,
    };
    establish(
        &Establishing {
            arguments,
            environment,
            root: &root,
            config,
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
    )
}

/// Everything one verification needs to establish its own answer. The configuration file this run read, or nothing when it read none.
fn read_from(root: &Path) -> String {
    if root.join(crate::config::FILE_NAME).is_file() {
        return crate::config::FILE_NAME.to_owned();
    }
    String::new()
}

struct Establishing<'a> {
    arguments: &'a Verify,
    environment: &'a Environment,
    root: &'a Path,
    config: Config,
    identity: &'a str,
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
fn part_of(arguments: &Verify) -> Result<Option<rust_mutants::run::Shard>, String> {
    arguments
        .shard
        .as_deref()
        .map(rust_mutants::run::Shard::parse)
        .transpose()
        .map_err(|error| error.to_string())
}

/// Runs the verification, writes what it concluded, and stores it for the next run of the same inputs.
fn establish(establishing: &Establishing<'_>, streams: Streams<'_>) -> u8 {
    let Establishing {
        arguments,
        environment,
        root,
        identity,
        started,
        evidence,
        store,
        trace,
        watch,
        ..
    } = *establishing;
    let Streams {
        out: stdout,
        err: stderr,
    } = streams;
    let shard = match part_of(arguments) {
        Ok(shard) => shard,
        Err(said) => {
            super::diagnose(stderr, &said);
            return EXIT_ERROR;
        }
    };
    let request = Request {
        root: root.to_path_buf(),
        configuration: read_from(root),
        config: establishing.config.clone(),
        packages: packages(arguments, &establishing.config),
        test_args: harness_args(arguments, &establishing.config),
        cargo: Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
        keep_temp: arguments.keep_temp,
        run_id: identity.to_owned(),
        started,
        engine_trace: engine_recorder(arguments, root, identity),
        evidence: evidence.clone(),
        changed: establishing.changed.clone(),
        checkpoints: (!arguments.no_cache).then(|| store.root().join(CHECKPOINTS)),
        evidence_store: (!arguments.no_cache).then(|| store.root().to_path_buf()),
        shard,
    };
    let result = {
        let mut notes = ui::Notes::of(arguments.ui, stderr);
        run::run(&request, environment, &mut notes, watch)
    };
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            trace.run_end("ERROR", None, Some(error.to_string()));
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };

    let report = outcome.report;
    trace.run_end(
        &lines::escape(&format!("{:?}", report.verdict)),
        Some(report.accounting),
        None,
    );
    let document = match persist(
        &Persisting {
            root,
            report: &report,
            request: &request,
            store,
            store_it: !arguments.no_cache
                && evidence.is_known()
                && !environment.cancel.is_cancelled(),
            kept: &outcome.kept,
        },
        arguments,
        stderr,
    ) {
        Ok(document) => document,
        Err(code) => return code,
    };

    let _written = stdout.write_all(said(&report, root, &document, environment).as_bytes());
    report.verdict.exit_code()
}

/// What a run has to say, in the shape the thing reading it wants.
///
/// A terminal gets the drawing and a pipe gets the record stream. They are two
/// projections of one value rather than two accounts of a report (ADR 0020),
/// and the stream keeps every guarantee `docs/report-v1.md` states, because it
/// is a contract with programs.
fn said(
    report: &crate::report::Report,
    root: &Path,
    document: &str,
    environment: &Environment,
) -> String {
    if !environment.terminal.drawing {
        return lines::kept(report, document);
    }
    let sources = crate::presentation::Sources::read(root, report);
    let told = crate::presentation::Told::of(report, &sources, document);
    crate::presentation::human::draw(&told, environment.terminal)
}

/// The change set, asked for with the directories this project writes left out.
fn asked_about(
    arguments: &Verify,
    about: (&Path, &Config),
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<Option<crate::git::Change>, String> {
    let (root, config) = about;
    let excluded = crate::evidence::tree::Excluded::beside(&config.reports.directory);
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
) -> Result<Option<crate::git::Change>, String> {
    if !arguments.changed && arguments.changed_from.is_none() {
        return Ok(None);
    }
    let base = arguments
        .changed_from
        .as_deref()
        .unwrap_or(crate::git::DEFAULT_BASE);
    crate::git::changed(asked, base).map(Some).ok_or_else(|| {
        format!(
            "git could not say what differs from {base:?}, and a run that cannot see what \
                 changed cannot claim to have verified what changed"
        )
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

/// What this run is, as numbers, or nothing when the tree could not be read. A tree that cannot be measured is a limitation the report states, not a reason to refuse to verify it.
fn evidence_of(
    arguments: &Verify,
    asked: &Asked<'_>,
    cancel: &rust_mutants::runner::Cancel,
) -> Evidence {
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
    );
    let Ok(toolchain) = toolchain else {
        return Evidence::default();
    };
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
        )
        .map(|read| read.environment)
        .unwrap_or_default(),
        contract: format!("{:?}", config.contract).to_lowercase(),
        test_args: harness_args(arguments, config),
        features: config.execution.features.clone(),
        timeout_ms: u64::try_from(config.execution.timeout.as_millis()).unwrap_or(u64::MAX),
        versions: vec![
            format!("njutest {}", crate::VERSION),
            format!("rust-mutants {}", rust_mutants::VERSION),
        ],
        corpus: String::new(),
    };
    identity::of(&asked, mode, common, arguments.shard.clone()).unwrap_or_default()
}

/// The store of earlier answers, bounded the way the configuration says.
fn store_of(environment: &Environment, config: &Config) -> Store {
    Store::new(
        &environment.cache_directory,
        config.cache.max_bytes,
        config.cache.ttl,
    )
}

/// Waits for whatever run is already establishing this identity, so the same work is not done twice at once. A claim that cannot be taken is not a reason to refuse: the run does the work again rather than not at all.
fn claim(
    asking: &Asking<'_>,
    arguments: &Verify,
    cancel: &rust_mutants::runner::Cancel,
    stderr: &mut dyn Write,
) -> Option<Lease> {
    let path = asking.store.lease(asking.identity);
    let mut waited = false;
    let taken = lock::claim(&path, LEASE_TIMEOUT, cancel, &mut || waited = true);
    let mut notes = ui::Notes::of(arguments.ui, stderr);
    if waited {
        notes.note("waiting", "another run of the same inputs is under way");
    }
    match taken {
        Ok(lease) => Some(lease),
        Err(error) => {
            notes.note("unclaimed", &error.to_string());
            None
        }
    }
}

/// The two streams a command writes to.
struct Streams<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

/// A finished run and everywhere it goes.
struct Persisting<'a> {
    root: &'a Path,
    report: &'a crate::report::Report,
    request: &'a Request,
    store: &'a Store,
    store_it: bool,
    kept: &'a [PathBuf],
}

/// Writes the report where a reader will look for it, retires what the configuration no longer keeps, and stores the answer for the next run of the same inputs. Returns the exit code only when the report could not be written, which is the one failure that stops the run from having answered at all.
fn persist(
    persisting: &Persisting<'_>,
    arguments: &Verify,
    stderr: &mut dyn Write,
) -> Result<String, u8> {
    let Persisting {
        root,
        report,
        request,
        store,
        store_it,
        kept,
    } = *persisting;
    let written = match reports::keep(root, report) {
        Ok(written) => written,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return Err(EXIT_ERROR);
        }
    };
    let removed = reports::retain(root, request_keep(request));
    let stored = store_it.then(|| store.put(report)).and_then(Result::err);
    let mut notes = ui::Notes::of(arguments.ui, stderr);
    notes.note("report", &written.document.display().to_string());
    for path in &removed {
        notes.note("retired", &path.display().to_string());
    }
    if !kept.is_empty() {
        drop(crate::kept::record(
            root,
            &report.run_id,
            Timestamp::now(),
            kept,
        ));
    }
    for path in kept {
        notes.note("kept", &path.display().to_string());
    }
    if let Some(error) = stored {
        notes.note("not-stored", &error.to_string());
    }
    Ok(reports::Store::read(root).said(&written.document))
}

/// Whether an earlier run of the same inputs has already answered, and the claim this run holds while it establishes its own.
enum Settled {
    /// An earlier run answered, and this is the exit code.
    Answered(u8),
    /// Nothing is stored; the claim this run holds while it establishes one.
    Establish(Option<Lease>),
}

/// Whether the store already answers, unless the run was told to establish everything afresh or the tree could not be measured.
fn already_answered(
    asking: &Asking<'_>,
    arguments: &Verify,
    cancel: &rust_mutants::runner::Cancel,
    streams: Streams<'_>,
) -> Settled {
    if arguments.no_cache || asking.identity.is_empty() {
        return Settled::Establish(None);
    }
    settled(asking, arguments, cancel, streams)
}

/// Asks the store, waits for whoever is already establishing this identity, and asks again.
fn settled(
    asking: &Asking<'_>,
    arguments: &Verify,
    cancel: &rust_mutants::runner::Cancel,
    streams: Streams<'_>,
) -> Settled {
    let Streams {
        out: stdout,
        err: stderr,
    } = streams;
    if let Reuse::Answered(code) = reuse(asking, stdout, stderr) {
        return Settled::Answered(code);
    }
    let lease = claim(asking, arguments, cancel, stderr);
    if lease.is_some()
        && let Reuse::Answered(code) = reuse(asking, stdout, stderr)
    {
        return Settled::Answered(code);
    }
    Settled::Establish(lease)
}

/// What a run needs to ask the store of earlier answers.
struct Asking<'a> {
    store: &'a Store,
    identity: &'a str,
    run_id: &'a str,
    started: Timestamp,
    root: &'a Path,
}

/// Whether this run has to establish anything at all.
enum Reuse {
    /// An earlier run of the same inputs answered, and this is the exit code.
    Answered(u8),
    /// Nothing is stored, or what is stored cannot be believed.
    Establish,
}

/// Reads back what an earlier run of the same inputs established, and writes it as this run's report.
fn reuse(asking: &Asking<'_>, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Reuse {
    let Asking {
        store,
        identity,
        run_id,
        started,
        root,
    } = *asking;
    let stored = match store.get(identity) {
        Ok(Some(stored)) => stored,
        Ok(None) => return Reuse::Establish,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return Reuse::Establish;
        }
    };
    let mut report = stored.clone();
    run_id.clone_into(&mut report.run_id);
    report.provenance.cached = true;
    report.provenance.source_run_id = Some(stored.run_id);
    report.timing.started = started.to_string();
    report.timing.finished = Timestamp::now().to_string();
    report.timing.duration_ms = 0;
    let written = match reports::keep(root, &report) {
        Ok(written) => written,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return Reuse::Establish;
        }
    };
    let _written = stdout.write_all(
        lines::kept(&report, &reports::Store::read(root).said(&written.document)).as_bytes(),
    );
    Reuse::Answered(report.verdict.exit_code())
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
fn engine_recorder(
    arguments: &Verify,
    root: &Path,
    identity: &str,
) -> rust_mutants::trace::Recorder {
    use rust_mutants::trace::{DirSink, Recorder as EngineRecorder, Sink as EngineSink};

    let Some(requested) = &arguments.trace else {
        return EngineRecorder::disabled();
    };
    let directory = if requested.is_empty() {
        root.join(".njutest/trace").join(identity)
    } else {
        PathBuf::from(requested)
    };
    DirSink::create(&directory.join(crate::app::trace::ENGINE_DIRECTORY)).map_or_else(
        |_error| EngineRecorder::disabled(),
        |sink| EngineRecorder::wall(EngineSink::Dir(sink)),
    )
}

fn recorder(arguments: &Verify, run: &Recording<'_>, stderr: &mut dyn Write) -> Recorder {
    let (root, identity, contract) = (run.root, run.identity, run.contract);
    let start = StartRecord::of(identity, crate::report::RunKind::Full, contract);
    let ring = Sink::ring();
    let Some(requested) = &arguments.trace else {
        return Recorder::wall(ring, start);
    };
    let directory = if requested.is_empty() {
        root.join(".njutest/trace").join(identity)
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
