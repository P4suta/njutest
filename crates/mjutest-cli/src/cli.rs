// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Argument parsing, the environment a run is given, and the exit codes.
//!
//! This module knows the command tree and the verdict-to-exit-code mapping,
//! and nothing about executing a run. It also names [`Environment`], which
//! is how everything below the command line is told about the machine:
//! [ADR 0001] puts the process environment behind an argument, so only the
//! composition root reads it and every layer under it is driven by a test.
//!
//! [ADR 0001]: https://github.com/P4suta/mjutest/blob/main/docs/adr/0001-seam-policy.md

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// `ASSURED`, `CHANGE_ASSURED`, `SCOPE_ASSURED`, `RESOLVED`, or `COMPLETED`.
pub const EXIT_ASSURED: u8 = 0;
/// `DEFECT` or `REPRODUCED`.
pub const EXIT_DEFECT: u8 = 1;
/// `INSUFFICIENT`.
pub const EXIT_INSUFFICIENT: u8 = 2;
/// `ERROR`, invalid input, or an infrastructure failure.
pub const EXIT_ERROR: u8 = 3;
/// Interrupted (SIGINT).
pub const EXIT_INTERRUPTED: u8 = 130;
/// Terminated (SIGTERM).
pub const EXIT_TERMINATED: u8 = 143;

/// The program name every diagnostic line starts with.
pub const PROGRAM: &str = "mjutest";

/// What the machine is, as an argument.
///
/// Everything a run learns about the process it is running in arrives here.
/// The composition root fills it once; nothing below reads the environment,
/// so a test drives a run with exactly the machine it wants and no other.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// The whole environment, as names and values.
    pub vars: Vec<(OsString, OsString)>,
    /// Where the process was started.
    pub working_directory: PathBuf,
    /// The operating system's temporary directory.
    pub temp_directory: PathBuf,
    /// The user's cache directory, which the build cache lives under.
    pub cache_directory: PathBuf,
}

impl Environment {
    /// The value of `name`, if the environment has one.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .iter()
            .find(|(key, _)| key == OsStr::new(name))
            .map(|(_, value)| value.as_os_str())
    }

    /// Where a user's caches belong, from `vars` alone: `XDG_CACHE_HOME`,
    /// then `HOME/.cache`, then `LOCALAPPDATA` on Windows.
    ///
    /// A pure function of the environment rather than a read of it, so the
    /// composition root stays the only place that asks the process.
    #[must_use]
    pub fn cache_directory_of(vars: &[(OsString, OsString)]) -> PathBuf {
        let of = |name: &str| {
            vars.iter()
                .find(|(key, _)| key == OsStr::new(name))
                .map(|(_, value)| PathBuf::from(value))
                .filter(|path| path.is_absolute())
        };
        of("XDG_CACHE_HOME")
            .or_else(|| of("HOME").map(|home| home.join(".cache")))
            .or_else(|| of("LOCALAPPDATA"))
            .unwrap_or_else(|| PathBuf::from(".mjutest-cache"))
    }
}

/// The parsed command line.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "mjutest",
    version = crate::VERSION,
    about = "An audit-oriented assurance runner for Rust",
    long_about = "An audit-oriented assurance runner for Rust.\n\n\
        mjutest connects ordinary Cargo tests with coverage routing, mutation testing through \
        rust-mutants, paired kill confirmation, a soundness phase, targeted fuzzing, explicit \
        integration resources, and reviewable repair candidates. It reports a verdict — \
        ASSURED, DEFECT, INSUFFICIENT, ERROR — and never a percentage.",
    after_help = "Exit codes:\n  \
        0    ASSURED, CHANGE_ASSURED, SCOPE_ASSURED, RESOLVED, or COMPLETED\n  \
        1    DEFECT or REPRODUCED\n  \
        2    INSUFFICIENT\n  \
        3    ERROR, invalid input, or an infrastructure failure\n  \
        130  interrupted\n  \
        143  terminated",
    term_width = 100,
    color = clap::ColorChoice::Never
)]
pub struct Request {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// What the command line asked for.
#[derive(Debug, Clone, Subcommand)]
#[non_exhaustive]
pub enum Command {
    /// Write an annotated .mjutest.toml.
    ///
    /// The skeleton is the defaults with every other section as commented
    /// guidance, so loading it untouched configures exactly what configuring
    /// nothing would.
    Init(Init),
    /// Report the toolchain and the tools a run needs.
    ///
    /// Exits 0 when a standard-v1 run could go ahead on this machine and 3
    /// when it could not. What only a deep-v1 run or a later phase needs is
    /// reported as optional and costs nothing.
    Doctor(Doctor),
}

/// `mjutest init`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub struct Init {
    /// Replace a configuration that is already there.
    #[arg(long)]
    pub force: bool,
}

/// `mjutest doctor`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub struct Doctor {}

/// How a run writes what it is doing while it does it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[value(rename_all = "lower")]
pub enum Ui {
    /// Lines a person reads.
    #[default]
    Plain,
    /// One JSON object per line, for a program.
    Jsonl,
}

/// A command line that could not be parsed, or a request to print help or the
/// version, rendered for the stream it belongs on.
#[derive(Debug)]
pub struct Usage {
    /// The text to write, newline-terminated.
    pub text: String,
    /// Whether the text belongs on stderr (a diagnostic) or stdout (help).
    pub to_stderr: bool,
    /// The exit code the process ends with.
    pub exit_code: u8,
}

/// Parses `args`, program name first. A bare invocation is the help text, as
/// it is in goatest.
///
/// # Errors
///
/// Returns the rendered usage error, help text, or version text.
pub fn parse<I>(args: I) -> Result<Request, Usage>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().collect();
    if args.len() <= 1 {
        args.push(OsString::from("--help"));
    }
    Request::try_parse_from(args).map_err(|error| {
        if error.use_stderr() {
            Usage {
                text: diagnose(&error.render().to_string()),
                to_stderr: true,
                exit_code: EXIT_ERROR,
            }
        } else {
            Usage {
                text: error.render().to_string(),
                to_stderr: false,
                exit_code: EXIT_ASSURED,
            }
        }
    })
}

/// Renders a diagnostic the way every mjutest diagnostic is rendered: one
/// `mjutest: ` prefix, a lowercase message, and the usage that follows it.
fn diagnose(rendered: &str) -> String {
    let message = rendered.strip_prefix("error: ").unwrap_or(rendered);
    format!("{PROGRAM}: {message}")
}
