// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command line of the rust-mutants engine.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod config;
pub mod diagnostics;
pub mod error;
pub(crate) mod filesystem;
pub mod kept;
pub mod report;

/// What earlier runs of a tree established, as the engine keeps it.
pub use rust_mutants::outcomes;
/// Driving a session, as the engine does it.
pub use rust_mutants::run;
pub mod settings;
pub mod stream;
pub(crate) mod strictjson;
pub(crate) mod text;
pub mod tui;
pub mod ui;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use rust_mutants::runner::Cancel;

/// The exit code of a usage error or an infrastructure failure.
pub const EXIT_USAGE: u8 = run::Exit::Unestablished.code();

/// Everything the command line needs from the process it runs in.
#[derive(Debug, Clone)]
pub struct Environment {
    /// The process environment, which the engine hands to every command and test process it starts.
    pub vars: rust_mutants::vars::Variables,
    /// The directory snapshots and target directories are created in.
    pub temp_directory: PathBuf,
    /// This program's own path, which `doctor` copies to measure what running a newly written file costs.
    ///
    /// An argument for the same reason the temporary directory is: the composition root is where the operating system is asked.
    /// It has to be a program somebody may copy and run — a system binary is signed in place and is killed when it is run from anywhere else — and this one is both to hand and known to run.
    pub program: PathBuf,
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
    /// The continuous integration service the command runs under.
    pub ci: CiHost,
    /// The cargo `--cargo` named, which every toolchain lookup runs rather than the one on the `PATH`.
    pub cargo: Option<PathBuf>,
}

/// A continuous integration service, and where it takes what a step reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiHost {
    /// GitHub Actions.
    GitHub {
        /// The file a step appends its Markdown summary to, which `GITHUB_STEP_SUMMARY` names.
        summary: PathBuf,
        /// The file a step appends its `name=value` outputs to, which `GITHUB_OUTPUT` names.
        output: PathBuf,
        /// The checkout an annotation's path is relative to, which `GITHUB_WORKSPACE` names.
        workspace: PathBuf,
    },
    /// GitLab CI.
    GitLab,
    /// No service this release writes for.
    None,
}

impl Environment {
    /// The workspace a command was pointed at: what it named, or where it was started.
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
pub const EXIT_INTERRUPTED: u8 = run::Exit::Interrupted.code();

/// What every exit code of this program means, as the lines `--help` ends with.
#[must_use]
pub fn exit_codes() -> String {
    let mut said = String::from("Exit codes:");
    for exit in run::Exit::ALL {
        let written = write!(said, "\n  {:<4} {}", exit.code(), exit.meaning());
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    said
}

/// The two streams a command writes to.
pub struct Streams<'a> {
    /// What the command established.
    pub out: &'a mut dyn Write,
    /// What went wrong.
    pub err: &'a mut dyn Write,
}

impl std::fmt::Debug for Streams<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Streams").finish_non_exhaustive()
    }
}

impl Environment {
    /// Where a user's caches belong, from `vars` alone: `XDG_CACHE_HOME`, then `HOME/.cache`, then `LOCALAPPDATA` on Windows.
    #[must_use]
    pub fn cache_directory_of(vars: &rust_mutants::vars::Variables) -> PathBuf {
        rust_mutants::userdirs::cache_directory(vars, ".rust-mutants-cache")
    }

    /// Whether `vars` asks for no colour, which one variable being set at all says.
    #[must_use]
    pub fn no_color_of(vars: &rust_mutants::vars::Variables) -> bool {
        vars.var("NO_COLOR").is_some_and(|value| !value.is_empty())
    }

    /// The service `vars` says the command runs under: GitHub Actions only when it names every file a step reports through.
    #[must_use]
    pub fn ci_host_of(vars: &rust_mutants::vars::Variables) -> CiHost {
        let named = |wanted: &str| vars.var(wanted).filter(|value| !value.is_empty());
        if named("GITHUB_ACTIONS").is_some_and(|value| value == "true")
            && let (Some(summary), Some(output), Some(workspace)) = (
                named("GITHUB_STEP_SUMMARY"),
                named("GITHUB_OUTPUT"),
                named("GITHUB_WORKSPACE"),
            )
        {
            return CiHost::GitHub {
                summary: PathBuf::from(summary),
                output: PathBuf::from(output),
                workspace: PathBuf::from(workspace),
            };
        }
        if named("GITLAB_CI").is_some_and(|value| value == "true") {
            return CiHost::GitLab;
        }
        CiHost::None
    }
}

/// The process's installed cancellation handlers and the state they own.
#[derive(Debug)]
pub struct Interrupt {
    cancel: Cancel,
    signalled: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    handlers: SignalHandlers,
}

#[derive(Debug, Default)]
struct SignalHandlers(Vec<signal_hook::SigId>);

impl SignalHandlers {
    fn unregister(&mut self) {
        for handler in self.0.drain(..) {
            let removed = signal_hook::low_level::unregister(handler);
            if !removed {
                std::process::abort();
            }
        }
    }
}

impl Drop for SignalHandlers {
    fn drop(&mut self) {
        self.unregister();
    }
}

impl Interrupt {
    /// The cancellation flag every fallible operation observes.
    #[must_use]
    pub const fn cancel(&self) -> &Cancel {
        &self.cancel
    }

    /// The signal number captured by the handlers, or zero before interruption.
    #[must_use]
    pub fn signalled(&self) -> &std::sync::atomic::AtomicUsize {
        &self.signalled
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        self.handlers.unregister();
    }
}

/// Installs owned cancellation handlers for `SIGINT` and `SIGTERM`.
///
/// # Errors
/// Returns the registration failure without leaving a signal handler unowned.
pub fn interruptible() -> std::io::Result<Interrupt> {
    let cancel = Cancel::new();
    let signalled = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handlers = SignalHandlers::default();
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        handlers
            .0
            .push(signal_hook::flag::register(signal, cancel.flag())?);
        let number = usize::try_from(signal).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("signal number {signal} cannot be recorded: {error}"),
            )
        })?;
        handlers.0.push(signal_hook::flag::register_usize(
            signal,
            std::sync::Arc::clone(&signalled),
            number,
        )?);
    }
    Ok(Interrupt {
        cancel,
        signalled,
        handlers,
    })
}

/// The status a process ends with: what the run concluded, unless a signal ended it first.
#[must_use]
pub fn ended(code: u8, signalled: &std::sync::atomic::AtomicUsize) -> std::process::ExitCode {
    std::process::ExitCode::from(match signalled.load(std::sync::atomic::Ordering::SeqCst) {
        0 => code,
        signal => match u8::try_from(signal) {
            Ok(number) => match 128u8.checked_add(number) {
                Some(interrupted) => interrupted,
                None => code,
            },
            Err(_signal_does_not_fit_an_exit_code) => code,
        },
    })
}

/// Runs the command line described by `args` (program name first) and returns its exit code, writing to the two streams it was given.
///
/// `cancel` is raised by whoever owns the process's signals; every command stops at the first place it can and leaves nothing behind.
#[cfg(feature = "testkit")]
pub fn run_from<I>(args: I, environment: &Environment, cancel: &Cancel, streams: Streams<'_>) -> u8
where
    I: IntoIterator<Item = OsString>,
{
    run_from_compiled(args, Composition::new(environment, None), cancel, streams)
}

/// Runs the command line with the catalog identity Cargo embedded in this composition root.
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
            if app::write(stream, &usage.text).is_err() {
                return EXIT_USAGE;
            }
            return usage.exit_code;
        }
    };
    let painted = Environment {
        paints: command.color.paints(ui::Stream {
            no_color: environment.no_color,
            is_terminal: environment.stdout_is_terminal,
        }),
        cargo: command.cargo.clone(),
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
            complaint_code(complain(stderr, &error), EXIT_INTERRUPTED)
        }
        Err(error) => complaint_code(complain(stderr, &error), EXIT_USAGE),
    }
}

/// What went wrong, and what to do about it when the code carries one.
fn complain(stderr: &mut dyn Write, error: &error::CliError) -> std::io::Result<()> {
    writeln!(stderr, "rust-mutants: {error}")?;
    if let Some(remedy) = error.code().remedy {
        writeln!(stderr, "          try: {remedy}")?;
    }
    stderr.flush()
}

fn complaint_code(written: std::io::Result<()>, intended: u8) -> u8 {
    match written {
        Ok(()) => intended,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => intended,
        Err(_) => EXIT_USAGE,
    }
}
