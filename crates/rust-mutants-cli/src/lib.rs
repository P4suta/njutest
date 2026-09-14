// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command line of the rust-mutants engine.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod config;
pub mod diagnostics;
pub mod error;
pub mod kept;
pub mod report;

/// What earlier runs of a tree established, as the engine keeps it.
pub use rust_mutants::outcomes;
/// Driving a session, as the engine does it.
pub use rust_mutants::run;
pub mod settings;
pub mod stream;
pub mod tui;
pub mod ui;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use rust_mutants::runner::Cancel;

/// The exit code of a usage error or an infrastructure failure.
pub const EXIT_USAGE: u8 = 2;

/// Everything the command line needs from the process it runs in.
#[derive(Debug, Clone)]
pub struct Environment {
    /// The process environment, which the engine hands to every command and test process it starts.
    pub vars: Vec<(OsString, OsString)>,
    /// The directory snapshots and target directories are created in.
    pub temp_directory: PathBuf,
    /// The user's cache directory, which what earlier runs established is kept under.
    pub cache_directory: PathBuf,
    /// The working directory, which a command with no `--root` reads.
    pub working_directory: PathBuf,
    /// Whether the environment asked for no colour, which `NO_COLOR` says.
    pub no_color: bool,
    /// Whether what the command writes goes to a terminal rather than to a file or a pipe.
    pub stdout_is_terminal: bool,
    /// Whether what the command writes is painted, which `--color` settles from the two above.
    pub paints: bool,
}

impl Environment {
    /// The workspace a command was pointed at: what it named, or where it was started.
    ///
    /// A name that is not an absolute path is resolved against the working
    /// directory the command was given rather than against the process's own.
    /// The two are the same for the binary, which composes one from the other,
    /// and they are not the same for anything else that calls the entry point:
    /// a caller that says where it is and then gets an answer about somewhere
    /// else has been told about a tree it did not name.
    #[must_use]
    pub fn rooted(&self, named: Option<&Path>) -> PathBuf {
        named.map_or_else(
            || self.working_directory.clone(),
            |path| self.working_directory.join(path),
        )
    }
}

/// The process inputs a composition root gives one command invocation.
#[derive(Debug, Clone, Copy)]
pub struct Composition<'a> {
    pub(crate) environment: &'a Environment,
    pub(crate) compiled_catalog: Option<&'a str>,
}

impl<'a> Composition<'a> {
    /// Couples the runtime environment to the catalog identity this binary was compiled with.
    #[must_use]
    pub const fn new(environment: &'a Environment, compiled_catalog: Option<&'a str>) -> Self {
        Self {
            environment,
            compiled_catalog,
        }
    }
}

/// The exit code of a run that was interrupted.
pub const EXIT_INTERRUPTED: u8 = 130;

/// The exit code of a run that was terminated, which is `SIGTERM` by the convention every shell reports.
///
/// A continuous integration job that cancels a run sends this one rather than
/// an interrupt, so a person reading a log sees it more often than they see
/// 130, and a table that named only 130 left them to guess.
pub const EXIT_TERMINATED: u8 = 143;

/// What every exit code of this program means, as the lines `--help` ends with.
///
/// The table is rendered from the codes rather than written beside them: a
/// person reads it to decide what their script does next, and one that named a
/// code no run returns has them waiting for an exit that never comes.
#[must_use]
pub fn exit_codes() -> String {
    let mut said = String::from("Exit codes:");
    for (code, meaning) in [
        (
            run::EXIT_DETECTED,
            "every mutant the run decided, the tests noticed",
        ),
        (
            run::EXIT_UNDETECTED,
            "there is a finding: a survivor, a stale claim, something the run could not decide",
        ),
        (
            EXIT_USAGE,
            "the run itself failed, or the command was used wrongly",
        ),
        (EXIT_INTERRUPTED, "interrupted"),
        (EXIT_TERMINATED, "terminated"),
    ] {
        let written = write!(said, "\n  {code:<4} {meaning}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    said
}

/// The two streams a command writes to.
#[expect(
    missing_debug_implementations,
    reason = "a stream is a handle to the outside; there is nothing to print about one"
)]
pub struct Streams<'a> {
    /// What the command established.
    pub out: &'a mut dyn Write,
    /// What went wrong.
    pub err: &'a mut dyn Write,
}

impl Environment {
    /// Where a user's caches belong, from `vars` alone: `XDG_CACHE_HOME`, then `HOME/.cache`, then `LOCALAPPDATA` on Windows.
    #[must_use]
    pub fn cache_directory_of(vars: &[(OsString, OsString)]) -> PathBuf {
        rust_mutants::userdirs::cache_directory(vars, ".rust-mutants-cache")
    }

    /// Whether `vars` asks for no colour, which one variable being set at all says.
    #[must_use]
    pub fn no_color_of(vars: &[(OsString, OsString)]) -> bool {
        vars.iter()
            .any(|(key, value)| key == "NO_COLOR" && !value.is_empty())
    }
}

/// A cancellation flag the process raises on `SIGINT` and `SIGTERM`, and the number of the signal that raised it.
///
/// Both binaries of this crate are composition roots and both want this, and
/// neither of them is where the duplication should live: registering a handler
/// reads nothing of the process, so it belongs beside the code it cancels
/// rather than beside the code that reads `argv`.
#[must_use]
pub fn interruptible() -> (Cancel, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let cancel = Cancel::new();
    let signalled = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        drop(signal_hook::flag::register(signal, cancel.flag()));
        drop(signal_hook::flag::register_usize(
            signal,
            std::sync::Arc::clone(&signalled),
            usize::try_from(signal).unwrap_or(0),
        ));
    }
    (cancel, signalled)
}

/// The status a process ends with: what the run concluded, unless a signal ended it first.
#[must_use]
pub fn ended(code: u8, signalled: &std::sync::atomic::AtomicUsize) -> std::process::ExitCode {
    std::process::ExitCode::from(match signalled.load(std::sync::atomic::Ordering::SeqCst) {
        0 => code,
        signal => u8::try_from(signal)
            .ok()
            .map_or(code, |number| 128u8.saturating_add(number)),
    })
}

/// Runs the command line described by `args` (program name first) and returns its exit code, writing to the two streams it was given. `cancel` is raised by whoever owns the process's signals; every command stops at the first place it can and leaves nothing behind.
pub fn run_from<I>(args: I, environment: &Environment, cancel: &Cancel, streams: Streams<'_>) -> u8
where
    I: IntoIterator<Item = OsString>,
{
    run_from_compiled(args, Composition::new(environment, None), cancel, streams)
}

/// Runs the command line with the catalog identity Cargo embedded in this composition root.
///
/// A normal build passes `None`. The two binaries pass their `option_env!`
/// value, which lets an instrumented copy recognize only the outer run that
/// compiled it from the same catalog.
pub fn run_from_compiled<I>(
    args: I,
    composition: Composition<'_>,
    cancel: &Cancel,
    streams: Streams<'_>,
) -> u8
where
    I: IntoIterator<Item = OsString>,
{
    let environment = composition.environment;
    let Streams {
        out: stdout,
        err: stderr,
    } = streams;
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            let _written = stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush());
            return usage.exit_code;
        }
    };
    let painted = Environment {
        paints: command.color.paints(ui::Stream {
            no_color: environment.no_color,
            is_terminal: environment.stdout_is_terminal,
        }),
        ..environment.clone()
    };
    let dispatched = app::dispatch(
        &command.command,
        Composition::new(&painted, composition.compiled_catalog),
        Streams {
            out: &mut *stdout,
            err: &mut *stderr,
        },
        cancel,
    );
    match dispatched {
        Ok(code) => code,
        Err(error) if cancel.is_cancelled() => {
            complain(stderr, &error);
            EXIT_INTERRUPTED
        }
        Err(error) => {
            complain(stderr, &error);
            EXIT_USAGE
        }
    }
}

/// What went wrong, and what to do about it when the code carries one.
///
/// A remedy is the next step rather than an explanation: a flag to pass, a
/// component to install, a variable to unset. Where the message already names
/// the file and the line there is nothing a remedy could add, and none is
/// printed.
fn complain(stderr: &mut dyn Write, error: &error::CliError) {
    let _written = writeln!(stderr, "rust-mutants: {error}");
    if let Some(remedy) = error.code().remedy {
        let _written = writeln!(stderr, "          try: {remedy}");
    }
}
