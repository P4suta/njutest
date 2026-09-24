// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Repository gates.
//! Each gate is a pure function over the tree it is given, so a test can hand it a synthetic tree and watch it refuse the right things; the command line only points it at this repository.

#![forbid(unsafe_code)]

pub mod deps;
pub mod devgates;
pub mod drift;
pub mod engineaudit;
pub mod fixtures;
pub mod fuzzclippy;
pub mod gates;
pub mod kaniaudit;
pub mod lanes;
pub mod lints;
pub mod milestones;
pub mod modelaudit;
pub mod prepush;
pub mod proofaudit;
pub mod release;
pub mod reportdiff;
pub mod route;
pub mod sbom;
pub mod sentinel;
pub mod shapes;
pub mod strictjson;
pub mod surface;
pub mod sweep;
pub mod wire;
pub mod work;

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, ExitCode, ExitStatus};
use std::time::Duration;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cargo xtask", about = "Repository gates", term_width = 100, color = clap::ColorChoice::Never)]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

/// What `cargo xtask` can be asked to do: a repository gate, or one of the tools the gates' own machinery is.
#[derive(Debug, Subcommand)]
enum Task {
    #[command(flatten)]
    Gate(Gate),
    /// Runs a command once this machine's lane for it is free, and holds the lane until the command ends.
    Slot {
        /// The lane: `heavy`, for a run that compiles or tests the whole workspace.
        lane: String,
        /// The command and its arguments, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
    /// The pre-push hook: the exact commit being pushed, checked in this repository's one reusable tree.
    PrePush,
    /// Takes the build output and temporary directories nobody has written for a while, and removes them within a budget.
    Sweep {
        /// How many seconds removing may take before the rest is left for the next sweep.
        #[arg(long, default_value_t = 60)]
        budget_seconds: u64,
    },
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
    /// Every gate, in order.
    All,
}

/// What the composition root read from the process, handed to the commands that need it.
#[derive(Debug, Clone, Copy)]
pub struct Process<'a> {
    /// The exact cargo the composition root selected.
    pub cargo: &'a OsStr,
    /// The environment the process was started with.
    pub environment: &'a [(OsString, OsString)],
    /// The directory the process was started in.
    pub directory: &'a Path,
    /// The program that is running.
    pub executable: &'a Path,
}

/// The process's standard streams.
pub struct Streams<'a> {
    /// Standard input.
    pub input: &'a mut dyn BufRead,
    /// Standard output.
    pub output: &'a mut dyn Write,
    /// Standard error.
    pub errors: &'a mut dyn Write,
}

impl std::fmt::Debug for Streams<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Streams")
    }
}

/// Runs the gate named by `args` against the workspace and reports.
pub fn run_from<I>(args: I, process: &Process<'_>, streams: &mut Streams<'_>) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let stdout = &mut *streams.output;
    let stderr = &mut *streams.errors;
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
    let gate = match cli.task {
        Task::Gate(gate) => gate,
        Task::Slot { lane, command } => return slot(&lane, &command, process, stderr),
        Task::PrePush => return pre_push(process, &mut *streams.input, stderr),
        Task::Sweep { budget_seconds } => {
            return sweep(process, Duration::from_secs(budget_seconds), stdout, stderr);
        }
    };
    let root = gates::workspace_root();
    let outcome = match gate {
        Gate::Devgates => gates::devgates(&root),
        Gate::Lints => gates::lints(&root),
        Gate::Deps => gates::deps(&root),
        Gate::Fixtures => gates::fixtures(&root),
        Gate::Tracked => gates::tracked(&root),
        Gate::FuzzClippy { alternate: _ } => fuzzclippy::check(&root, process.cargo)
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
    run: &Path,
    trace: Option<&Path>,
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

/// Sweeps within `budget` and says what it did, failing where it left something it meant to take.
fn sweep(
    process: &Process<'_>,
    budget: Duration,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    match sweep_report(process, budget) {
        Ok(done) => after_output(
            writeln!(stdout, "{done}"),
            if done.fell_short() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            },
        ),
        Err(failure) => after_output(writeln!(stderr, "sweep: {failure}"), ExitCode::FAILURE),
    }
}

/// Sweeps this repository's idle build output and temporary directories.
fn sweep_report(
    process: &Process<'_>,
    budget: Duration,
) -> Result<sweep::Swept, sweep::SweepError> {
    let temp = lanes::variable(process.environment, "TMPDIR").map_or_else(
        || std::path::PathBuf::from("/tmp"),
        std::path::PathBuf::from,
    );
    let request = sweep::Request {
        repository: process.directory,
        temp: &temp,
        environment: process.environment,
        budget,
    };
    sweep::sweep(&request)
}

/// Holds `lane` while `command` runs, and answers with the command's own exit status.
fn slot(
    lane: &str,
    command: &[OsString],
    process: &Process<'_>,
    stderr: &mut dyn Write,
) -> ExitCode {
    let Some(named) = lanes::Lane::named(lane) else {
        return after_output(
            writeln!(
                stderr,
                "slot: there is no lane named {lane:?}; the lane there is: heavy"
            ),
            ExitCode::from(2),
        );
    };
    let Some((program, arguments)) = command.split_first() else {
        return after_output(
            writeln!(stderr, "slot: nothing to run after `--`"),
            ExitCode::from(2),
        );
    };
    let lanes = match lanes::Lanes::from_environment(process.environment) {
        Ok(lanes) => lanes,
        Err(failure) => {
            return after_output(writeln!(stderr, "slot: {failure}"), ExitCode::FAILURE);
        }
    };
    let stops = match work::Stops::arm() {
        Ok(stops) => stops,
        Err(failure) => {
            return after_output(writeln!(stderr, "slot: {failure}"), ExitCode::FAILURE);
        }
    };
    let holder = lanes::Holder {
        worktree: process.directory.to_path_buf(),
        revision: lanes::revision_of(process.directory, process.environment),
        command: command
            .iter()
            .map(|word| word.display().to_string())
            .collect::<Vec<_>>()
            .join(" "),
    };
    let request = lanes::Request {
        lane: named,
        holder: &holder,
        stops: &stops,
    };
    let held = match lanes.hold(&request, stderr) {
        Ok(held) => held,
        Err(failure) => {
            let code = failure.signal().map_or(1, signalled_code);
            return after_output(writeln!(stderr, "slot: {failure}"), ExitCode::from(code));
        }
    };
    let mut running = Command::new(program);
    running
        .args(arguments)
        .env(lanes::HELD, lanes.held_with(named))
        .stdin(std::process::Stdio::null());
    let ran = work::run(&mut running, None, &stops, |leader| held.working_on(leader));
    drop(held);
    match ran {
        Ok(work::Ended::Exited(status)) => ExitCode::from(exit_status(status)),
        Ok(work::Ended::Interrupted { signal }) => ExitCode::from(signalled_code(signal)),
        Ok(work::Ended::OverBudget { .. }) => ExitCode::from(124),
        Err(failure) => after_output(writeln!(stderr, "slot: {failure}"), ExitCode::from(127)),
    }
}

/// The exit status a shell reports for a process `signal` ended.
fn signalled_code(signal: i32) -> u8 {
    match signal.checked_add(128).map(u8::try_from) {
        Some(Ok(code)) => code,
        Some(Err(_)) | None => 1,
    }
}

/// The exit status a shell would report for `status`, signals included.
fn exit_status(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return match u8::try_from(code) {
            Ok(code) => code,
            Err(_beyond_a_byte) => 1,
        };
    }
    signalled(status)
}

#[cfg(unix)]
fn signalled(status: ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt as _;

    let code = status.signal().and_then(|signal| signal.checked_add(128));
    match code.map(u8::try_from) {
        Some(Ok(code)) => code,
        Some(Err(_)) | None => 1,
    }
}

#[cfg(not(unix))]
const fn signalled(_status: ExitStatus) -> u8 {
    1
}

/// Runs the pre-push gate over the ref updates on `input`.
fn pre_push(process: &Process<'_>, input: &mut dyn BufRead, stderr: &mut dyn Write) -> ExitCode {
    let surroundings = prepush::Surroundings {
        directory: process.directory,
        environment: process.environment,
        executable: process.executable,
    };
    let (code, interrupted) = match prepush::gate(&surroundings, input, stderr) {
        Ok(_passed) => (ExitCode::SUCCESS, false),
        Err(failure) => {
            let code = ExitCode::from(failure.exit_code());
            let written = failure
                .to_string()
                .lines()
                .try_for_each(|line| writeln!(stderr, "pre-push: {line}"));
            (after_output(written, code), failure.exit_code() >= 128)
        }
    };
    if interrupted {
        return code;
    }
    let written = match sweep_report(process, sweep::AFTER_A_PUSH) {
        Ok(done) => writeln!(stderr, "pre-push: {done}"),
        Err(failure) => writeln!(stderr, "pre-push: sweep: {failure}"),
    };
    after_output(written, code)
}

/// Applies process policy after the composition root has attempted its only observable output.
fn after_output(written: std::io::Result<()>, intended: ExitCode) -> ExitCode {
    match written {
        Ok(()) => intended,
        Err(_output_failure) => ExitCode::FAILURE,
    }
}
