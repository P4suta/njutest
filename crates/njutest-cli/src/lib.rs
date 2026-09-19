// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The njutest assurance runner.

#![forbid(unsafe_code)]

pub mod app;
pub mod askable;
pub mod assure;
pub mod build;
pub mod cache;
pub mod checkpoint;
pub mod cli;
pub mod config;
pub mod coverage;
pub mod error;
pub mod evidence;
pub mod git;
pub mod kept;
pub mod limitation;
pub mod modelled;
pub mod naming;
pub mod presentation;
pub mod provider;
pub mod repair;
pub mod report;
pub mod resource;
pub mod run_id;
pub mod rustflags;
pub mod scratch;
pub mod soundness;
pub mod targets;
pub mod trace;
pub mod ui;
pub mod watch;
pub mod why;
pub mod wire;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

use std::ffi::OsString;
use std::io::Write;

/// The version of this runner, as recorded in every report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the composition root can find out about where the output is going.
///
/// The gathering, not the deciding: [`presentation::Terminal::of`] holds the
/// rules and is asserted on its own, and this is the one place that asks the
/// operating system (ADR 0001).
#[must_use]
pub fn asked(vars: &[(OsString, OsString)]) -> presentation::Asked {
    use std::io::IsTerminal as _;
    let said = |name: &str| {
        rust_mutants::vars::var(vars, name).map(|value| value.to_string_lossy().into_owned())
    };
    let locale = said("LC_ALL")
        .or_else(|| said("LC_CTYPE"))
        .or_else(|| said("LANG"))
        .unwrap_or_default()
        .to_ascii_uppercase();
    let forced = said("CLICOLOR_FORCE").is_some_and(|value| !value.is_empty() && value != "0");
    let refused = said("NO_COLOR").is_some_and(|value| !value.is_empty());
    presentation::Asked {
        reader: if std::io::stdout().is_terminal() {
            presentation::Reader::Person
        } else {
            presentation::Reader::Program
        },
        columns: columns(),
        colour: match (forced, refused) {
            (true, _) => presentation::Wanted::Forced,
            (false, true) => presentation::Wanted::Refused,
            (false, false) => presentation::Wanted::Unsaid,
        },
        term: said("TERM"),
        glyphs: if locale.contains("UTF-8") || locale.contains("UTF8") || cfg!(windows) {
            presentation::Glyphs::Drawn
        } else {
            presentation::Glyphs::Plain
        },
    }
}

/// How wide the terminal says it is, where it can be asked.
#[cfg(unix)]
fn columns() -> Option<usize> {
    let size = rustix::termios::tcgetwinsize(std::io::stdout()).ok()?;
    usize::from(size.ws_col).checked_sub(0).filter(|it| *it > 0)
}

/// How wide the terminal says it is, where this release cannot ask.
#[cfg(not(unix))]
const fn columns() -> Option<usize> {
    None
}

/// A cancellation flag the process raises on `SIGINT` and `SIGTERM`, and the number of the signal that raised it.
#[must_use]
pub fn interruptible() -> (
    rust_mutants::runner::Cancel,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let cancel = rust_mutants::runner::Cancel::new();
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

/// Runs the command line described by `args` (program name first) in `environment` and returns its exit code, writing to the two streams it was given.
pub fn run_from<I>(
    args: I,
    environment: &cli::Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8
where
    I: IntoIterator<Item = OsString>,
{
    match cli::parse(args) {
        Ok(request) => app::run(&request, environment, stdout, stderr),
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            let _written = stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush());
            usage.exit_code
        }
    }
}
