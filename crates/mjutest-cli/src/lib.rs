// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The mjutest assurance runner.
//!
//! mjutest is an orchestrator, not a test framework. It connects ordinary
//! Cargo tests with coverage routing, the rust-mutants engine, paired kill
//! confirmation, a soundness phase, targeted fuzzing, explicit integration
//! resources, and reviewable repair candidates, and it reports a verdict —
//! `ASSURED`, `DEFECT`, `INSUFFICIENT`, `ERROR` — instead of a percentage.
//!
//! [`run_from`] is the entry point the binary calls; the crate is a library
//! so that every layer below the command line can be driven by a test.

#![forbid(unsafe_code)]

pub mod app;
pub mod assure;
pub mod build;
pub mod build_cache;
pub mod cli;
pub mod config;
pub mod coverage;
pub mod error;
pub mod git;
pub mod report;
pub mod run_id;
pub mod rustflags;
pub mod scratch;
pub mod targets;
pub mod trace;
pub mod ui;
pub mod watch;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

/// The version of this runner, as recorded in every report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the command line described by `args` (program name first) in
/// `environment` and returns its exit code, writing to the two streams it
/// was given.
///
/// Exit codes: `0` an assured, resolved, or completed operation; `1`
/// `DEFECT` or `REPRODUCED`; `2` `INSUFFICIENT`; `3` `ERROR`, invalid input,
/// or an infrastructure failure; `130` interrupted; `143` terminated.
pub fn run_from<I>(
    args: I,
    environment: &cli::Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    match cli::parse(args) {
        Ok(request) => ExitCode::from(app::run(&request, environment, stdout, stderr)),
        Err(usage) => {
            let stream: &mut dyn Write = if usage.to_stderr { stderr } else { stdout };
            // A closed stream is the reader's choice, not a failure of ours.
            let _written = stream
                .write_all(usage.text.as_bytes())
                .and_then(|()| stream.flush());
            ExitCode::from(usage.exit_code)
        }
    }
}
