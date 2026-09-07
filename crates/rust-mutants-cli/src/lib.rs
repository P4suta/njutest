// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command line of the rust-mutants engine.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod config;
pub mod error;
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
use std::io::Write;
use std::path::PathBuf;

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

/// The exit code of a run that was interrupted.
pub const EXIT_INTERRUPTED: u8 = 130;

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
        let value = |name: &str| -> Option<PathBuf> {
            vars.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| PathBuf::from(value))
                .filter(|path| path.is_absolute())
        };
        value("XDG_CACHE_HOME")
            .or_else(|| value("HOME").map(|home| home.join(".cache")))
            .or_else(|| value("LOCALAPPDATA"))
            .unwrap_or_else(|| PathBuf::from(".rust-mutants-cache"))
    }

    /// Whether `vars` asks for no colour, which one variable being set at all says.
    #[must_use]
    pub fn no_color_of(vars: &[(OsString, OsString)]) -> bool {
        vars.iter()
            .any(|(key, value)| key == "NO_COLOR" && !value.is_empty())
    }
}

/// Runs the command line described by `args` (program name first) and returns its exit code, writing to the two streams it was given. `cancel` is raised by whoever owns the process's signals; every command stops at the first place it can and leaves nothing behind.
pub fn run_from<I>(args: I, environment: &Environment, cancel: &Cancel, streams: Streams<'_>) -> u8
where
    I: IntoIterator<Item = OsString>,
{
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
        &painted,
        Streams {
            out: &mut *stdout,
            err: &mut *stderr,
        },
        cancel,
    );
    match dispatched {
        Ok(code) => code,
        Err(error) if cancel.is_cancelled() => {
            let _written = writeln!(stderr, "rust-mutants: {error}");
            EXIT_INTERRUPTED
        }
        Err(error) => {
            let _written = writeln!(stderr, "rust-mutants: {error}");
            EXIT_USAGE
        }
    }
}
