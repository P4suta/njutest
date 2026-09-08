// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The mjutest assurance runner.

#![forbid(unsafe_code)]

pub mod app;
pub mod assure;
pub mod build;
pub mod build_cache;
pub mod cache;
pub mod checkpoint;
pub mod cli;
pub mod config;
pub mod coverage;
pub mod error;
pub mod evidence;
pub mod git;
pub mod kept;
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

use std::ffi::OsString;
use std::io::Write;

/// The version of this runner, as recorded in every report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A cancellation flag the process raises on `SIGINT` and `SIGTERM`, and the number of the signal that raised it.
///
/// Both binaries of this crate are composition roots and both want this, and
/// neither of them is where the duplication should live: registering a handler
/// reads nothing of the process, so it belongs beside the code it cancels
/// rather than beside the code that reads `argv`.
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
