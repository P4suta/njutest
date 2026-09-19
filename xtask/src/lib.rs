// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Repository gates. Each gate is a pure function over the tree it is given, so a test can hand it a synthetic tree and watch it refuse the right things; the command line only points it at this repository.

#![forbid(unsafe_code)]

pub mod deps;
pub mod devgates;
pub mod engineaudit;
pub mod fixtures;
pub mod gates;
pub mod lints;
pub mod proofaudit;
pub mod release;
pub mod reportdiff;
pub mod route;
pub mod sbom;
pub mod shapes;
pub mod wire;

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
    /// Whether a completed run's verdicts are the ones its own recording supports (ADR 0004).
    Proofaudit {
        /// The directory the run left its report in.
        run: std::path::PathBuf,
        /// The directory the run left its recording in, which is what the proof layers are re-derived from.
        #[arg(long)]
        trace: Option<std::path::PathBuf>,
    },
    /// Whether a completed engine run's report is the one its own rows, recording, and ledger support (ADR 0004).
    EngineAudit {
        /// The directory the run left its report in.
        run: std::path::PathBuf,
        /// The directory the run left its recording in, which is what the trace layer is re-derived from.
        #[arg(long)]
        trace: Option<std::path::PathBuf>,
        /// The reports of the other parts of this catalog, when the run was one part. Repeatable.
        #[arg(long = "shard", value_name = "REPORT")]
        shards: Vec<std::path::PathBuf>,
        /// The configuration file whose accepted survivors the run is held to.
        #[arg(long, value_name = "FILE")]
        ledger: Option<std::path::PathBuf>,
        /// Re-derive the census of the walk's own decisions from the recording.
        #[arg(long)]
        sites: bool,
    },
    /// What changed between two stored assurance reports.
    ReportDiff {
        /// The earlier report.
        before: std::path::PathBuf,
        /// The later one.
        after: std::path::PathBuf,
    },
    /// What a release is made of, as a `CycloneDX` document.
    Sbom {
        /// Write it here rather than to standard output.
        #[arg(long, value_name = "FILE")]
        output: Option<std::path::PathBuf>,
    },
    /// A second opinion, by body shape alone, on every catch-all the ledger waives. Refuses nothing.
    Waivers,
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
        Gate::Proofaudit { run, trace } => {
            return audit_run(&run, trace.as_deref(), stdout, stderr);
        }
        Gate::EngineAudit {
            run,
            trace,
            shards,
            ledger,
            sites,
        } => {
            return audit_engine(
                &gates::EngineRun {
                    run: &run,
                    trace: trace.as_deref(),
                    shards: &shards,
                    ledger: ledger.as_deref(),
                    sites,
                },
                stdout,
                stderr,
            );
        }
        Gate::ReportDiff { before, after } => gates::report_diff(&before, &after),
        Gate::Sbom { output } => gates::sbom(&root, output.as_deref()),
        Gate::Waivers => gates::waivers(&root),
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

/// A run whose report could not be read at all is neither a clean audit nor a failed one, so it leaves by an exit code of its own.
fn audit_engine(
    asked: &gates::EngineRun<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    match gates::engine_audit(asked) {
        Ok(audit) => {
            let _written = writeln!(stdout, "{audit}");
            ExitCode::from(audit.exit_code())
        }
        Err(failure) => {
            let _written = writeln!(stderr, "{failure}");
            ExitCode::from(engineaudit::EXIT_UNREADABLE)
        }
    }
}

/// A recording that could not be read at all is neither a clean audit nor a failed one, so it leaves by an exit code of its own.
fn audit_run(
    run: &std::path::Path,
    trace: Option<&std::path::Path>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    match gates::proofaudit(run, trace) {
        Ok(audit) => {
            let _written = writeln!(stdout, "{audit}");
            ExitCode::from(audit.exit_code())
        }
        Err(failure) => {
            let _written = writeln!(stderr, "{failure}");
            ExitCode::from(proofaudit::EXIT_UNREADABLE)
        }
    }
}
