// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Repository gates.
//! Each gate is a pure function over the tree it is given, so a test can hand it a synthetic tree and watch it refuse the right things; the command line only points it at this repository.

#![forbid(unsafe_code)]

pub mod deps;
pub mod devgates;
pub mod docflows;
pub mod drift;
pub mod engineaudit;
pub mod fixtures;
pub mod fuzzclippy;
pub mod gates;
pub mod kaniaudit;
pub mod lints;
pub mod milestones;
pub mod modelaudit;
pub mod proofaudit;
pub mod release;
pub mod remote;
pub mod reportdiff;
pub mod route;
pub mod sbom;
pub mod sentinel;
pub mod shapes;
pub mod strictjson;
pub mod surface;
pub mod wire;

use std::ffi::{OsStr, OsString};
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
    /// Lossy Rust shapes this repository does not write.
    Lints,
    /// Dependency direction between the workspace crates.
    Deps,
    /// Conventions of the independent fixture projects under fixtures/.
    Fixtures,
    /// Nothing a build writes is committed: no tracked path lies under a directory named `target`.
    Tracked,
    /// Clippy every independent fuzz target under the root workspace lint policy.
    FuzzClippy {
        /// Reserved for a future alternate manifest; keeps this execution gate out of `all`.
        #[arg(long, default_value_t = false, hide = true)]
        alternate: bool,
    },
    /// Every workflow the documentation shows passes actionlint against this repository's own actions.
    Docflows {
        /// The actionlint to run; the lint lane is where it is installed, which keeps this gate out of `all`.
        #[arg(long, value_name = "PROGRAM", default_value = "actionlint")]
        actionlint: std::path::PathBuf,
    },
    /// Version consistency between the workspace and the release manifest.
    ReleaseCheck,
    /// Fail closed unless Kani's raw law export proves every assertion reachable and every cover satisfiable.
    KaniLawsAudit {
        /// The fresh JSON document written by pinned Kani 0.68.
        export: std::path::PathBuf,
    },
    /// Every milestone named in the documentation resolves to one roadmap row.
    Milestones,
    /// Every public function of an incidental surface is reached by something that ships.
    Reached,
    /// Every crate declares what its visibility means; incidental APIs are compiled privately.
    Surfaces,
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
        /// The reports of the other parts of this catalog, when the run was one part.
        /// Repeatable.
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
    /// A second opinion, by body shape alone, on every catch-all the ledger waives.
    /// Refuses nothing.
    Waivers,
    /// The whole suite of this commit on the other machines, before it is pushed.
    RemoteCheck {
        /// The file that names the machines, how each is reached, and what each runs.
        #[arg(long, value_name = "FILE")]
        machines: std::path::PathBuf,
        /// The worktree whose checked-out commit is put to them, where it is not this one.
        #[arg(long, value_name = "DIR")]
        worktree: Option<std::path::PathBuf>,
    },
    /// Every gate, in order.
    All,
}

/// Runs the gate named by `args` against the workspace and reports.
/// `cargo` is the exact program selected by the process-environment composition root.
pub fn run_from<I>(
    args: I,
    cargo: &OsStr,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let stream: &mut dyn Write = if error.use_stderr() { stderr } else { stdout };
            let rendered = error.render().to_string();
            let intended = if error.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
            return after_output(stream.write_all(rendered.as_bytes()), intended);
        }
    };
    let root = gates::workspace_root();
    let outcome = match cli.gate {
        Gate::Devgates => gates::devgates(&root),
        Gate::Lints => gates::lints(&root),
        Gate::Deps => gates::deps(&root),
        Gate::Fixtures => gates::fixtures(&root),
        Gate::Tracked => gates::tracked(&root),
        Gate::FuzzClippy { alternate: _ } => {
            fuzzclippy::check(&root, cargo).map_err(|error| gates::GateFailure(error.to_string()))
        }
        Gate::Docflows { actionlint } => docflows::check(&root, actionlint.as_os_str())
            .map_err(|error| gates::GateFailure(error.to_string())),
        Gate::ReleaseCheck => gates::release_check(&root),
        Gate::KaniLawsAudit { export } => kaniaudit::audit(&export, &root)
            .map(|()| "kani-laws: 15 production harnesses, every assertion reachable and every cover satisfiable".to_owned())
            .map_err(|error| gates::GateFailure(error.to_string())),
        Gate::Milestones => gates::milestones(&root),
        Gate::Reached => gates::reached(&root),
        Gate::Surfaces => gates::surfaces(&root),
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
        Gate::RemoteCheck { machines, worktree } => remote::check(worktree.as_deref().unwrap_or(&root), &machines)
            .map_err(|error| gates::GateFailure(error.to_string())),
        Gate::All => gates::all(&root),
    };
    match outcome {
        Ok(report) => after_output(writeln!(stdout, "{report}"), ExitCode::SUCCESS),
        Err(failure) => after_output(writeln!(stderr, "{failure}"), ExitCode::FAILURE),
    }
}

/// A run whose report could not be read at all, like an audit with a layer blind to what was planted for it, is neither a clean audit nor a failed one, so it leaves by an exit code of its own.
fn audit_engine(
    asked: &gates::EngineRun<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let planted = match gates::engine_audit_sentinels() {
        Ok(planted) => planted,
        Err(blind) => {
            return after_output(
                writeln!(stderr, "{blind}"),
                ExitCode::from(engineaudit::EXIT_UNREADABLE),
            );
        }
    };
    match gates::engine_audit(asked) {
        Ok(audit) => {
            let intended = ExitCode::from(audit.exit_code());
            after_output(
                writeln!(
                    stdout,
                    "engine-audit: {planted} planted defects found first, each by the layer it \
                     was planted for, and the clean specimen drew none\n{audit}"
                ),
                intended,
            )
        }
        Err(failure) => after_output(
            writeln!(stderr, "{failure}"),
            ExitCode::from(engineaudit::EXIT_UNREADABLE),
        ),
    }
}

/// A recording that could not be read at all, like an audit with a layer blind to what was planted for it, is neither a clean audit nor a failed one, so it leaves by an exit code of its own.
fn audit_run(
    run: &std::path::Path,
    trace: Option<&std::path::Path>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let planted = match gates::proofaudit_sentinels() {
        Ok(planted) => planted,
        Err(blind) => {
            return after_output(
                writeln!(stderr, "{blind}"),
                ExitCode::from(proofaudit::EXIT_UNREADABLE),
            );
        }
    };
    match gates::proofaudit(run, trace) {
        Ok(audit) => {
            let intended = ExitCode::from(audit.exit_code());
            after_output(
                writeln!(
                    stdout,
                    "proofaudit: {planted} planted defects found first, each by the layer it \
                     was planted for, and the clean specimen drew none\n{audit}"
                ),
                intended,
            )
        }
        Err(failure) => after_output(
            writeln!(stderr, "{failure}"),
            ExitCode::from(proofaudit::EXIT_UNREADABLE),
        ),
    }
}

/// Applies process policy after the composition root has attempted its only observable output.
fn after_output(written: std::io::Result<()>, intended: ExitCode) -> ExitCode {
    match written {
        Ok(()) => intended,
        Err(_output_failure) => ExitCode::FAILURE,
    }
}
