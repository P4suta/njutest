// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The njutest assurance runner.

#![forbid(unsafe_code)]

pub mod app;
pub mod assure;
pub mod build;
pub mod cache;
pub mod checkpoint;
pub mod cli;
pub mod concurrency;
pub mod config;
pub mod coverage;
pub mod error;
pub mod evidence;
pub mod git;
pub mod kept;
pub mod limitation;
pub mod naming;
pub mod observe;
pub mod presentation;
pub mod provider;
pub mod repair;
pub mod report;
pub mod resource;
pub mod run_id;
pub mod rustflags;
pub mod scratch;
pub mod soundness;
pub mod spec;
pub(crate) mod strictjson;
pub mod targets;
pub(crate) mod text;
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
/// The gathering, not the deciding: [`presentation::Terminal::of`] holds the rules and is asserted on its own, and this is the one place that asks the operating system (ADR 0001).
/// # Errors
/// Returns an error rather than changing an environment value that is not valid UTF-8 into a different terminal policy.
pub fn asked(
    vars: &[(OsString, OsString)],
) -> Result<presentation::Asked, PresentationEnvironmentError> {
    use std::io::IsTerminal as _;
    let said = |name: &'static str| -> Result<Option<String>, PresentationEnvironmentError> {
        let Some(value) = rust_mutants::vars::var(vars, name) else {
            return Ok(None);
        };
        let value = std::str::from_utf8(value.as_encoded_bytes())
            .map_err(|source| PresentationEnvironmentError { name, source })?;
        Ok(Some(value.to_owned()))
    };
    let locale = match said("LC_ALL")? {
        Some(locale) => locale,
        None => match said("LC_CTYPE")? {
            Some(locale) => locale,
            None => match said("LANG")? {
                Some(locale) => locale,
                None => String::new(),
            },
        },
    }
    .to_ascii_uppercase();
    let forced = said("CLICOLOR_FORCE")?.is_some_and(|value| !value.is_empty() && value != "0");
    let refused = said("NO_COLOR")?.is_some_and(|value| !value.is_empty());
    Ok(presentation::Asked {
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
        term: said("TERM")?,
        glyphs: if locale.contains("UTF-8") || locale.contains("UTF8") || cfg!(windows) {
            presentation::Glyphs::Drawn
        } else {
            presentation::Glyphs::Plain
        },
    })
}

/// A terminal-policy environment value that cannot be interpreted exactly.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("environment variable {name} is not valid UTF-8: {source}")]
pub struct PresentationEnvironmentError {
    /// The variable whose value was read.
    name: &'static str,
    /// Why its bytes are not UTF-8.
    #[source]
    source: std::str::Utf8Error,
}

/// How wide the terminal says it is, where it can be asked.
#[cfg(unix)]
fn columns() -> Option<usize> {
    let size = match rustix::termios::tcgetwinsize(std::io::stdout()) {
        Ok(size) => size,
        Err(_) => return None,
    };
    usize::from(size.ws_col).checked_sub(0).filter(|it| *it > 0)
}

/// How wide the terminal says it is, where this release cannot ask.
#[cfg(not(unix))]
const fn columns() -> Option<usize> {
    None
}

/// The exit status installed signal handlers recorded, if one interrupted the process.
#[derive(Debug)]
pub struct SignalStatus {
    code: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    handlers: SignalHandlers,
}

/// The exact registrations one [`SignalStatus`] owns.
#[derive(Debug, Default)]
struct SignalHandlers(Vec<signal_hook::SigId>);

impl SignalHandlers {
    fn register(&mut self, id: signal_hook::SigId) {
        self.0.push(id);
    }

    const fn is_complete(&self) -> bool {
        self.0.len() == 4
    }
}

impl Drop for SignalHandlers {
    fn drop(&mut self) {
        while let Some(id) = self.0.pop() {
            if !signal_hook::low_level::unregister(id) {
                std::process::abort();
            }
        }
    }
}

/// A cancellation flag the process raises on `SIGINT` and `SIGTERM`, and the exit status that signal means.
///
/// # Errors
/// A signal has no portable exit-status representation or a handler cannot be installed.
pub fn interruptible() -> std::io::Result<(rust_mutants::runner::Cancel, SignalStatus)> {
    let cancel = rust_mutants::runner::Cancel::new();
    let signalled = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handlers = SignalHandlers::default();
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        let number = u8::try_from(signal).map_err(std::io::Error::other)?;
        let exit = 128_u8.checked_add(number).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("signal {signal} has no conventional u8 exit status"),
            )
        })?;
        handlers.register(signal_hook::flag::register(signal, cancel.flag())?);
        handlers.register(signal_hook::flag::register_usize(
            signal,
            std::sync::Arc::clone(&signalled),
            usize::from(exit),
        )?);
    }
    Ok((
        cancel,
        SignalStatus {
            code: signalled,
            handlers,
        },
    ))
}

/// The status a process ends with: what the run concluded, unless a signal ended it first.
#[must_use]
pub fn ended(code: u8, signalled: &SignalStatus) -> std::process::ExitCode {
    if !signalled.handlers.is_complete() {
        std::process::abort();
    }
    std::process::ExitCode::from(
        match signalled.code.load(std::sync::atomic::Ordering::SeqCst) {
            0 => code,
            exit => match u8::try_from(exit) {
                Ok(exit) => exit,
                Err(_) => code,
            },
        },
    )
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
        Ok(request) => match app::run(&request, environment, stdout, stderr) {
            Ok(code) => code,
            Err(_output_failure) => cli::EXIT_ERROR,
        },
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            match stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush())
            {
                Ok(()) => usage.exit_code,
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => usage.exit_code,
                Err(_output_failure) => cli::EXIT_ERROR,
            }
        }
    }
}
