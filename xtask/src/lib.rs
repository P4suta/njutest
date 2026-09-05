// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Repository gates. Each gate is a pure function over the tree it is given,
//! so a test can hand it a synthetic tree and watch it refuse the right
//! things; the command line only points it at this repository.

#![forbid(unsafe_code)]

pub mod deps;
pub mod devgates;
pub mod fixtures;
pub mod gates;
pub mod lints;
pub mod release;
pub mod reportdiff;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cargo xtask", about = "Repository gates", term_width = 100, color = clap::ColorChoice::Never)]
struct Cli {
    #[command(subcommand)]
    gate: Gate,
}

#[derive(Debug, Subcommand)]
enum Gate {
    /// The seam ratchet (ADR 0001): production code against `xtask/seam_allowlist.txt`.
    Devgates,
    /// `#[allow]` and `Box<dyn Trait>`, which this repository does not write.
    Lints,
    /// Dependency direction between the workspace crates.
    Deps,
    /// Conventions of the independent fixture projects under fixtures/.
    Fixtures,
    /// Version consistency between the workspace and the release manifest.
    ReleaseCheck,
    /// What changed between two stored assurance reports.
    ReportDiff {
        /// The earlier report.
        before: std::path::PathBuf,
        /// The later one.
        after: std::path::PathBuf,
    },
    /// Every gate, in order.
    All,
}

/// Runs the gate named by `args` against the workspace and reports.
pub fn run_from<I>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let stream: &mut dyn Write = if error.use_stderr() { stderr } else { stdout };
            let _written = stream.write_all(error.render().to_string().as_bytes());
            return if error.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let root = gates::workspace_root();
    let outcome = match cli.gate {
        Gate::Devgates => gates::devgates(&root),
        Gate::Lints => gates::lints(&root),
        Gate::Deps => gates::deps(&root),
        Gate::Fixtures => gates::fixtures(&root),
        Gate::ReleaseCheck => gates::release_check(&root),
        Gate::ReportDiff { before, after } => gates::report_diff(&before, &after),
        Gate::All => gates::all(&root),
    };
    match outcome {
        Ok(report) => {
            let _written = writeln!(stdout, "{report}");
            ExitCode::SUCCESS
        }
        Err(failure) => {
            let _written = writeln!(stderr, "{failure}");
            ExitCode::FAILURE
        }
    }
}
