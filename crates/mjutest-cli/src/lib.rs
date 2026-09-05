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
