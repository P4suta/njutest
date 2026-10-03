// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Repository gates.
//! Each gate is a pure function over the tree it is given, so a test can hand it a synthetic tree and watch it refuse the right things; the command line only points it at this repository.

#![forbid(unsafe_code)]

pub mod adrs;
pub mod bundle;
mod cfg_conditions;
pub mod claims;
pub mod codeql;
pub mod concurrency;
pub mod confined;
pub mod confirm;
/// The one coverage-floor command shared by local tasks and CI.
pub mod coverage;
pub mod crashes;
pub mod defaulted;
pub mod deps;
pub mod devgates;
pub mod docflows;
pub mod drift;
pub mod engineaudit;
pub mod environment;
pub mod error;
pub mod faults;
pub mod fixtures;
pub mod fuzzclippy;
pub mod gates;
pub mod invariants;
pub mod kaniaudit;
pub mod kanilaws;
pub mod knobs;
pub mod lanes;
pub mod layers;
pub mod lexed;
pub mod lints;
pub mod milestones;
pub mod modelaudit;
pub mod observation;
pub mod prepush;
pub mod proofaudit;
pub mod receipt;
pub mod release;
pub mod remote;
pub mod repair;
pub mod reportdiff;
pub mod repository;
pub mod route;
pub mod sbom;
pub mod schemas;
pub mod sentinel;
pub mod shapes;
pub mod specimen;
pub mod strictjson;
pub mod surface;
pub mod tools;
pub mod wasitestsuite;
pub mod wire;
pub mod work;

use crate::error::Coded as _;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, ExitCode, ExitStatus};

use clap::{Parser, Subcommand};
use gates::RepositoryGate;

#[derive(Debug, Parser)]
#[command(name = "cargo xtask", about = "Repository gates", term_width = 100, color = clap::ColorChoice::Never)]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

/// What `cargo xtask` can be asked to do.
#[derive(Debug, Subcommand)]
enum Task {
    #[command(flatten)]
    Repository(RepositoryGate),
    #[command(flatten)]
    Execution(ExecutionGate),
    /// Every repository-tree gate, in order.
    All,
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
    /// Runs a command with a temporary directory of its own, and fails naming whatever the command left in it (ADR 0006).
    Tidy {
        /// The command and its arguments, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
    /// An exclusively owned local security analysis with retained actual input identities.
    Codeql {
        /// The unchanged pinned bundle, query and native adapter.
        #[command(flatten)]
        options: codeql::Options,
    },
    /// Every pinned cargo plugin, selected by mise and answering through Cargo's complete external-subcommand protocol.
    Tools,
    /// Runs a mise-selected Cargo plugin through Cargo's complete external-subcommand protocol.
    Tool {
        /// The pinned Cargo subcommand and every original argument, after `--`.
        #[arg(last = true, required = true)]
        arguments: Vec<OsString>,
    },
}

#[derive(Debug, Subcommand)]
enum ExecutionGate {
    /// Clippy every independent fuzz target under the root workspace lint policy.
    FuzzClippy,
    /// Every workflow the documentation shows passes actionlint against this repository's own actions.
    Docflows {
        /// The actionlint to run; the lint lane is where it is installed, which keeps this gate out of `all`.
        #[arg(long, value_name = "PROGRAM", default_value = "actionlint")]
        actionlint: std::path::PathBuf,
    },
    /// Run the engine over a package, sealed, and write the receipt of one module's mutants for a registry decision (docs/invariants.md).
    Receipt {
        /// The registry decision the module decides.
        decision: String,
        /// The module, relative to the repository root.
        module: String,
        /// The package that holds it.
        #[arg(long)]
        package: String,
        /// The receipt's file name under xtask/receipts, without `.json`, where the decision rests on several modules: the decision's name, a hyphen, and lowercase words.
        #[arg(long)]
        name: Option<String>,
    },
    /// Prove every production law with Kani, or read back the proof of exactly these inputs, and audit it either way.
    KaniLaws {
        /// Where proofs are kept, one per digest of what they rest on.
        #[arg(long)]
        cache: std::path::PathBuf,
    },
    /// Fail closed unless Kani's raw law export proves every assertion reachable and every cover satisfiable.
    KaniLawsAudit {
        /// The fresh JSON document written by pinned Kani 0.68.
        export: std::path::PathBuf,
    },
    /// Run every preview1 test of WebAssembly/wasi-testsuite on the sealed host as a sealed test instance runs, and fail unless each ends as crates/rust-mutants/tests/wasi-testsuite.toml says.
    ///
    /// The commit the file pins is fetched once into the cache, verified by its id every time, and a test the file does not name, or a name the suite does not hold, fails too.
    WasiTestsuite {
        /// Where the suite is fetched to, one checkout per pinned commit; `target/wasi-testsuite` of the workspace where it is not named.
        #[arg(long, value_name = "DIR")]
        cache: Option<std::path::PathBuf>,
    },
    /// Whether a completed run's verdicts are the ones its own recording supports (ADR 0004).
    Proofaudit {
        /// The directory the run left its report in, or a merged report.
        run: std::path::PathBuf,
        /// The directory the run left its recording in, which is what the proof layers are re-derived from.
        #[arg(long, conflicts_with = "shards")]
        trace: Option<std::path::PathBuf>,
        /// Each shard a merged report was merged from, as its report or its run directory, each re-decided against its own recording before the merge is.
        /// Repeatable.
        #[arg(long = "shard", value_name = "REPORT")]
        shards: Vec<std::path::PathBuf>,
        /// The directory holding each shard's recording under the run it names, as `.njutest/trace` does; without it, each shard's layers are unaudited.
        #[arg(long, requires = "shards")]
        traces: Option<std::path::PathBuf>,
        /// The tree the run measured, which every body an answer it carried rests on is read again from, proved the file measured by the digest its build kept first.
        #[arg(long, value_name = "DIR", conflicts_with = "shards")]
        root: Option<std::path::PathBuf>,
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
        /// The tree the run measured, which the carry evidence is read again from and proved against first.
        #[arg(long, value_name = "DIR")]
        root: Option<std::path::PathBuf>,
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
    /// The release archive of one target, holding every shipped binary where `cargo binstall` reads it, and its SHA-256.
    Bundle {
        /// The target triple to build the binaries for.
        #[arg(long, value_name = "TRIPLE")]
        target: String,
        /// The directory the archive and its checksum are written to.
        #[arg(long, value_name = "DIR")]
        out: std::path::PathBuf,
    },
    /// Enforce the region-coverage floors recorded in this tree.
    CoverageRatchet,
    /// The whole suite of this commit on the other machines, before it is pushed.
    RemoteCheck {
        /// The file that names the machines, how each is reached, and what each runs.
        #[arg(long, value_name = "FILE")]
        machines: std::path::PathBuf,
        /// The worktree whose checked-out commit is put to them, where it is not this one.
        #[arg(long, value_name = "DIR")]
        worktree: Option<std::path::PathBuf>,
    },
}

/// What the composition root read from the process, handed to the commands that need it.
#[derive(Debug, Clone, Copy)]
pub struct Process<'a> {
    /// The exact cargo the composition root selected.
    pub cargo: &'a OsStr,
    /// The environment the process was started with.
    pub environment: &'a environment::Environment,
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

/// The command line `args` spell, or the exit code of the message clap already wrote about it.
fn parsed<I>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<Cli, ExitCode>
where
    I: IntoIterator<Item = OsString>,
{
    Cli::try_parse_from(args).map_err(|error| {
        let stream: &mut dyn Write = if error.use_stderr() { stderr } else { stdout };
        let rendered = error.render().to_string();
        let intended = if error.use_stderr() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
        after_output(stream.write_all(rendered.as_bytes()), intended)
    })
}

/// Runs the gate named by `args` against the workspace and reports.
pub fn run_from<I>(args: I, process: &Process<'_>, streams: &mut Streams<'_>) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let stdout = &mut *streams.output;
    let stderr = &mut *streams.errors;
    let cli = match parsed(args, stdout, stderr) {
        Ok(cli) => cli,
        Err(answered) => return answered,
    };
    if let Err(source) = prepare_work_inputs(process.environment) {
        let failure = work::WorkError::Watch { source };
        return after_output(
            writeln!(stderr, "xtask: {}", failure.coded()),
            ExitCode::FAILURE,
        );
    }
    let root = gates::workspace_root();
    let outcome = match cli.task {
        Task::Repository(gate) => gate.run(&root),
        Task::All => gates::all(&root),
        Task::Slot { lane, command } => return slot(&lane, &command, process, stderr),
        Task::PrePush => return pre_push(process, &mut *streams.input, stderr),
        Task::Tidy { command } => return tidy(&command, process, stderr),
        Task::Codeql { options } => {
            return match codeql::run(&root, &options, process.environment) {
                Ok(directory) => after_output(
                    writeln!(
                        stdout,
                        "codeql: retained generation {}",
                        directory.display()
                    ),
                    ExitCode::SUCCESS,
                ),
                Err(failure) => after_output(
                    writeln!(stderr, "codeql: {}", failure.coded()),
                    ExitCode::FAILURE,
                ),
            };
        }
        Task::Tools => return tools(process, stderr),
        Task::Tool { arguments } => return tool(&arguments, process, stderr),
        Task::Execution(gate) => return run_execution(gate, &root, process, (stdout, stderr)),
    };
    report(outcome, stdout, stderr)
}

/// Establishes configured observation inputs before any gate or child command starts.
fn prepare_work_inputs(environment: &environment::Environment) -> std::io::Result<()> {
    if let Some(directory) = environment.value("NJUTEST_TEST_COST_DIR") {
        std::fs::create_dir_all(directory).map_err(|source| {
            std::io::Error::new(
                source.kind(),
                format!(
                    "NJUTEST_TEST_COST_DIR {} cannot be prepared: {source}",
                    Path::new(directory).display()
                ),
            )
        })?;
    }
    Ok(())
}

/// Executes the selected Cargo command from the caller's directory under its inherited owner.
fn tool(arguments: &[OsString], process: &Process<'_>, stderr: &mut dyn Write) -> ExitCode {
    let command = tools::command(
        &gates::workspace_root(),
        "mise".as_ref(),
        arguments,
        process.environment,
    );
    let mut command = match command {
        Ok(command) => command,
        Err(failure) => {
            return after_output(
                writeln!(stderr, "tool: {}", failure.coded()),
                ExitCode::FAILURE,
            );
        }
    };
    command.current_dir(process.directory);
    execute_tool(&mut command, process.environment, stderr)
}

/// Hands the selected command to the inherited Unix process group without starting another group.
#[cfg(unix)]
fn execute_tool(
    command: &mut Command,
    _environment: &environment::Environment,
    stderr: &mut dyn Write,
) -> ExitCode {
    use std::os::unix::process::CommandExt as _;
    let failure = command.exec();
    after_output(writeln!(stderr, "tool: {failure}"), ExitCode::FAILURE)
}

/// Retains task completion on Windows through the existing owned work boundary.
#[cfg(windows)]
fn execute_tool(
    command: &mut Command,
    environment: &environment::Environment,
    stderr: &mut dyn Write,
) -> ExitCode {
    let stops = match work::Stops::arm() {
        Ok(stops) => stops,
        Err(failure) => {
            return after_output(
                writeln!(stderr, "tool: {}", failure.coded()),
                ExitCode::FAILURE,
            );
        }
    };
    match tools::run(
        tools::Request {
            command,
            bound: None,
            stops: &stops,
            environment,
            output: tools::Output::Inherited,
        },
        |_leader| Ok(()),
    ) {
        Ok(work::Ended::Exited(status)) => ExitCode::from(exit_status(status)),
        Ok(work::Ended::Interrupted { signal }) => ExitCode::from(signalled_code(signal)),
        Ok(work::Ended::OverBudget { .. } | work::Ended::Quiet { .. }) => ExitCode::from(124),
        Err(failure) => after_output(
            writeln!(stderr, "tool: {}", failure.coded()),
            ExitCode::FAILURE,
        ),
    }
}

/// Validates the pinned tools the gates invoke, writing what it established or the refusal that says why.
fn tools(process: &Process<'_>, stderr: &mut dyn Write) -> ExitCode {
    match tools::check(
        &gates::workspace_root(),
        "mise".as_ref(),
        process.cargo,
        process.environment,
    ) {
        Ok(said) => after_output(writeln!(stderr, "{said}"), ExitCode::SUCCESS),
        Err(failure) => after_output(
            writeln!(stderr, "tools: {}", failure.coded()),
            ExitCode::FAILURE,
        ),
    }
}

fn run_execution(
    gate: ExecutionGate,
    root: &Path,
    process: &Process<'_>,
    streams: (&mut dyn Write, &mut dyn Write),
) -> ExitCode {
    let (stdout, stderr) = streams;
    let outcome = match gate {
        ExecutionGate::FuzzClippy => fuzzclippy::check(root, process.cargo)
            .map_err(|error| gates::GateError(error.coded())),
        ExecutionGate::Docflows { actionlint } => docflows::check(root, actionlint.as_os_str())
            .map_err(|error| gates::GateError(error.coded())),
        ExecutionGate::Receipt {
            decision,
            module,
            package,
            name,
        } => receipt::write(
            root,
            process.cargo,
            (&decision, &package, &module),
            name.as_deref(),
        ),
        ExecutionGate::KaniLaws { cache } => kanilaws::laws(root, process.cargo, &cache),
        ExecutionGate::WasiTestsuite { cache } => {
            let cache = cache.map_or_else(
                || root.join(wasitestsuite::DEFAULT_CACHE),
                |named| process.directory.join(named),
            );
            wasitestsuite::run(root, process.cargo, &cache)
                .map_err(|error| gates::GateError(error.coded()))
        }
        ExecutionGate::KaniLawsAudit { export } => kaniaudit::audit(&export, root)
            .map(|()| format!("kani-laws: {} production harnesses, every assertion reachable, every cover satisfiable, and each within its ceiling", kanilaws::harnesses().len()))
            .map_err(|error| gates::GateError(error.coded())),
        ExecutionGate::Proofaudit {
            run,
            trace,
            shards,
            traces,
            root,
        } => {
            return audit_run(
                (&run, trace.as_deref(), &shards, traces.as_deref()),
                root.as_deref(),
                stdout,
                stderr,
            );
        }
        ExecutionGate::EngineAudit {
            run,
            trace,
            shards,
            ledger,
            sites,
            root: measured,
        } => {
            return audit_engine(
                &gates::EngineRun {
                    run: &run,
                    trace: trace.as_deref(),
                    shards: &shards,
                    ledger: ledger.as_deref(),
                    sites,
                    root: measured.as_deref(),
                },
                stdout,
                stderr,
            );
        }
        ExecutionGate::ReportDiff { before, after } => gates::report_diff(&before, &after),
        ExecutionGate::Sbom { output } => gates::sbom(root, output.as_deref()),
        ExecutionGate::Bundle { target, out } => bundle::bundle(&bundle::Request {
            root,
            cargo: process.cargo,
            environment: process.environment,
            target: &target,
            out: &process.directory.join(out),
        })
        .map(|written| written.to_string())
        .map_err(|error| gates::GateError(error.coded())),
        ExecutionGate::CoverageRatchet => coverage::ratchet(root, process.cargo),
        ExecutionGate::RemoteCheck { machines, worktree } => remote::check(worktree.as_deref().unwrap_or(root), &machines)
            .map_err(|error| gates::GateError(error.coded())),
    };
    report(outcome, stdout, stderr)
}

fn report(
    outcome: Result<String, gates::GateError>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    match outcome {
        Ok(report) => after_output(writeln!(stdout, "{report}"), ExitCode::SUCCESS),
        Err(failure) => after_output(writeln!(stderr, "{}", failure.coded()), ExitCode::FAILURE),
    }
}

/// A run whose report could not be read at all, like an audit with a layer blind to what was planted for it, is neither a clean audit nor a failed one, so it leaves by an exit code of its own.
fn audit_engine(
    asked: &gates::EngineRun<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let checkers = match schemas::Checkers::compiled() {
        Ok(checkers) => checkers,
        Err(uncompiled) => {
            return after_output(
                writeln!(stderr, "{}", uncompiled.coded()),
                ExitCode::from(engineaudit::EXIT_UNREADABLE),
            );
        }
    };
    let planted = match gates::engine_audit_sentinels(&checkers) {
        Ok(planted) => planted,
        Err(blind) => {
            return after_output(
                writeln!(stderr, "{}", blind.coded()),
                ExitCode::from(engineaudit::EXIT_UNREADABLE),
            );
        }
    };
    match gates::engine_audit(&checkers, asked) {
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
            writeln!(stderr, "{}", failure.coded()),
            ExitCode::from(engineaudit::EXIT_UNREADABLE),
        ),
    }
}

/// A recording that could not be read at all, like an audit with a layer blind to what was planted for it, is neither a clean audit nor a failed one, so it leaves by an exit code of its own.
fn audit_run(
    (run, trace, shards, traces): (&Path, Option<&Path>, &[std::path::PathBuf], Option<&Path>),
    root: Option<&Path>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ExitCode {
    let checkers = match schemas::Checkers::compiled() {
        Ok(checkers) => checkers,
        Err(uncompiled) => {
            return after_output(
                writeln!(stderr, "{}", uncompiled.coded()),
                ExitCode::from(proofaudit::EXIT_UNREADABLE),
            );
        }
    };
    let planted = match gates::proofaudit_sentinels(&checkers) {
        Ok(planted) => planted,
        Err(blind) => {
            return after_output(
                writeln!(stderr, "{}", blind.coded()),
                ExitCode::from(proofaudit::EXIT_UNREADABLE),
            );
        }
    };
    let audited = if shards.is_empty() {
        gates::proofaudit(&checkers, run, trace, root)
    } else {
        gates::proofaudit_merged(&checkers, run, shards, traces)
    };
    match audited {
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
            writeln!(stderr, "{}", failure.coded()),
            ExitCode::from(proofaudit::EXIT_UNREADABLE),
        ),
    }
}

/// Arms stop observation and admits its recipient before any slot metadata probe starts.
fn slot_controls(
    environment: &environment::Environment,
) -> Result<(work::Stops, tools::Recipient), work::WorkError> {
    let stops = work::Stops::arm()?;
    let recipient = tools::session_recipient(environment, &stops)
        .map_err(|source| work::WorkError::Watch { source })?;
    Ok((stops, recipient))
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
            return after_output(
                writeln!(stderr, "slot: {}", failure.coded()),
                ExitCode::FAILURE,
            );
        }
    };
    let (stops, recipient) = match slot_controls(process.environment) {
        Ok(controls) => controls,
        Err(failure) => {
            return after_output(
                writeln!(stderr, "slot: {}", failure.coded()),
                ExitCode::FAILURE,
            );
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
            return after_output(
                writeln!(stderr, "slot: {}", failure.coded()),
                ExitCode::from(code),
            );
        }
    };
    let mut running = Command::new(program);
    running
        .args(arguments)
        .env(lanes::HELD, lanes.held_with(named))
        .stdin(std::process::Stdio::null());
    let ran = tools::run_with_recipient(
        tools::Request {
            command: &mut running,
            bound: None,
            stops: &stops,
            environment: process.environment,
            output: tools::Output::Inherited,
        },
        |leader| held.working_on(leader),
        recipient,
    );
    if ran.is_err() {
        held.left_work_running();
    }
    drop(held);
    slot_outcome(ran, stderr)
}

fn slot_outcome(ran: Result<work::Ended, work::WorkError>, stderr: &mut dyn Write) -> ExitCode {
    match ran {
        Ok(work::Ended::Exited(status)) => ExitCode::from(exit_status(status)),
        Ok(work::Ended::Interrupted { signal }) => ExitCode::from(signalled_code(signal)),
        Ok(work::Ended::OverBudget { .. } | work::Ended::Quiet { .. }) => ExitCode::from(124),
        Err(failure) => after_output(
            writeln!(stderr, "slot: {}", failure.coded()),
            ExitCode::from(127),
        ),
    }
}

/// Every variable a platform's standard library or a POSIX tool reads its temporary directory from.
const TEMPORARY_VARIABLES: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];

/// The variables that name this platform's temporary directory, in the order its standard library reads them.
#[cfg(windows)]
const TEMPORARY_READ_FROM: [&str; 3] = ["TMP", "TEMP", "TMPDIR"];

/// The variables that name this platform's temporary directory, in the order its standard library reads them.
#[cfg(not(windows))]
const TEMPORARY_READ_FROM: [&str; 1] = ["TMPDIR"];

/// Runs `command` with a temporary directory nothing else uses, and refuses whatever it leaves there.
fn tidy(command: &[OsString], process: &Process<'_>, stderr: &mut dyn Write) -> ExitCode {
    let Some((program, arguments)) = command.split_first() else {
        return after_output(
            writeln!(stderr, "tidy: nothing to run after `--`"),
            ExitCode::from(2),
        );
    };
    let parent = TEMPORARY_READ_FROM
        .iter()
        .find_map(|name| process.environment.value(name))
        .map_or_else(
            || std::path::PathBuf::from("/tmp"),
            std::path::PathBuf::from,
        );
    let scratch = match tempfile::Builder::new()
        .prefix("njutest-tidy-")
        .tempdir_in(&parent)
    {
        Ok(scratch) => scratch,
        Err(source) => {
            return after_output(
                writeln!(
                    stderr,
                    "tidy: no temporary directory under {}: {source}",
                    parent.display()
                ),
                ExitCode::FAILURE,
            );
        }
    };
    let stops = match work::Stops::arm() {
        Ok(stops) => stops,
        Err(failure) => {
            return after_output(
                writeln!(stderr, "tidy: {}", failure.coded()),
                ExitCode::FAILURE,
            );
        }
    };
    let mut running = Command::new(program);
    running.args(arguments);
    let cache = match parent_cache(&parent) {
        Ok(cache) => cache,
        Err(source) => {
            return after_output(
                writeln!(
                    stderr,
                    "tidy: the parent cache owner cannot be created: {source}"
                ),
                ExitCode::FAILURE,
            );
        }
    };
    running.env("NJUTEST_TEST_CACHE_ROOT", cache.path());
    for name in TEMPORARY_VARIABLES {
        running.env(name, scratch.path());
    }
    clear_wrappers(&mut running);
    let inside = match lanes::Lanes::from_environment(process.environment) {
        Ok(lanes) => lanes.inside(lanes::Lane::Heavy),
        Err(_no_lanes) => None,
    };
    let ran = tools::run(
        tools::Request {
            command: &mut running,
            bound: None,
            stops: &stops,
            environment: process.environment,
            output: tools::Output::Inherited,
        },
        |leader| match &inside {
            Some(held) => held.working_on(leader),
            None => Ok(()),
        },
    );
    if ran.is_err()
        && let Some(held) = &inside
    {
        held.left_work_running();
    }
    tidy_outcome(ran, scratch, cache, stderr)
}

fn clear_wrappers(running: &mut Command) {
    for name in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    ] {
        running.env(name, "");
    }
}

/// Finishes tidy after the work owner reports its actual outcome, retaining unknown completion.
fn tidy_outcome(
    ran: Result<work::Ended, work::WorkError>,
    scratch: tempfile::TempDir,
    cache: tempfile::TempDir,
    stderr: &mut dyn Write,
) -> ExitCode {
    let code = match ran {
        Ok(work::Ended::Exited(status)) => exit_status(status),
        Ok(work::Ended::Interrupted { signal }) => signalled_code(signal),
        Ok(work::Ended::OverBudget { .. } | work::Ended::Quiet { .. }) => 124,
        Err(failure) => {
            let retained_cache = cache.keep();
            let retained_scratch = scratch.keep();
            return after_output(
                writeln!(
                    stderr,
                    "tidy: {}; producer completion is unknown, retaining owned roots {} and {}",
                    failure.coded(),
                    retained_cache.display(),
                    retained_scratch.display()
                ),
                ExitCode::from(127),
            );
        }
    };
    let tidy = left_behind(scratch.path(), code, stderr);
    dispose_cache(cache, tidy, stderr)
}

/// Disposes the parent's compilation cache after producer completion, refusing removal failures.
fn dispose_cache(cache: tempfile::TempDir, tidy: ExitCode, stderr: &mut dyn Write) -> ExitCode {
    match cache.close() {
        Ok(()) => tidy,
        Err(source) => after_output(
            writeln!(
                stderr,
                "tidy: the parent-owned cache could not be removed after producer completion: {source}"
            ),
            ExitCode::FAILURE,
        ),
    }
}

/// Creates the parent cleanup root before any producer is started.
fn parent_cache(parent: &Path) -> std::io::Result<tempfile::TempDir> {
    let cache = tempfile::Builder::new()
        .prefix("njutest-suite-cache-")
        .tempdir_in(parent)?;
    let owner = serde_json::json!({
        "schema": "njutest-suite-cache-owner-v1",
        "pid": std::process::id(),
    });
    std::fs::write(cache.path().join("owner.json"), owner.to_string())?;
    Ok(cache)
}

/// The run's own exit `code` where it left nothing in `scratch`, and a refusal naming each entry, with the owner its marker names where one is readable.
fn left_behind(scratch: &Path, code: u8, stderr: &mut dyn Write) -> ExitCode {
    let left = repository::entries(scratch).map(|entries| {
        entries
            .iter()
            .map(|entry| named_leftover(entry))
            .collect::<Vec<String>>()
    });
    match left {
        Ok(left) if left.is_empty() => ExitCode::from(code),
        Ok(mut left) => {
            left.sort();
            after_output(
                writeln!(
                    stderr,
                    "tidy: the run left {} entr{} in the temporary directory it was given, each one a temporary directory nobody owns (ADR 0006):\n  {}",
                    left.len(),
                    if left.len() == 1 { "y" } else { "ies" },
                    left.join("\n  ")
                ),
                ExitCode::FAILURE,
            )
        }
        Err(source) => after_output(
            writeln!(
                stderr,
                "tidy: what the run left could not be read: {source}"
            ),
            ExitCode::FAILURE,
        ),
    }
}

/// One leftover's name, beside the owner its marker records where it records one, so a refusal names who made the directory rather than only where it sits.
fn named_leftover(entry: &Path) -> String {
    let name = match entry.file_name() {
        Some(name) => name.display().to_string(),
        None => entry.display().to_string(),
    };
    match owner_named_by(entry) {
        Some(owner) => format!("{name} ({owner})"),
        None => name,
    }
}

/// The owner a leftover's `owner.json` names: a test's binary and test where a test owner wrote it, and the claiming pid or run holder where a product or engine marker did.
fn owner_named_by(leftover: &Path) -> Option<String> {
    let marker = match std::fs::read_to_string(leftover.join("owner.json")) {
        Ok(marker) => marker,
        Err(_absent) => return None,
    };
    let value = match strictjson::from_str(&marker) {
        Ok(value) => value,
        Err(_unreadable) => return None,
    };
    let schema = value.get("schema").and_then(serde_json::Value::as_str);
    let binary = value.get("binary").and_then(serde_json::Value::as_str);
    let named = value.get("test").and_then(serde_json::Value::as_str);
    let pid = value.get("pid").and_then(serde_json::Value::as_u64);
    let named_by = match (binary, named) {
        (Some(binary), Some(named)) => Some(format!("{binary}: {named}")),
        _ => None,
    };
    match (named_by, pid) {
        (Some(who), Some(pid)) => Some(format!(
            "owned by {} {who} (pid {pid})",
            schema.unwrap_or("an unlabelled schema")
        )),
        (Some(who), None) => Some(format!(
            "owned by {} {who}",
            schema.unwrap_or("an unlabelled schema")
        )),
        (None, Some(pid)) => Some(format!(
            "owner marker {} names pid {pid}",
            schema.unwrap_or("an unlabelled schema")
        )),
        (None, None) => None,
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
    match prepush::gate(&surroundings, input, stderr) {
        Ok(_passed) => ExitCode::SUCCESS,
        Err(failure) => {
            let code = ExitCode::from(failure.exit_code());
            let written = failure
                .coded()
                .lines()
                .try_for_each(|line| writeln!(stderr, "pre-push: {line}"));
            after_output(written, code)
        }
    }
}

/// Applies process policy after the composition root has attempted its only observable output.
fn after_output(written: std::io::Result<()>, intended: ExitCode) -> ExitCode {
    match written {
        Ok(()) => intended,
        Err(_output_failure) => ExitCode::FAILURE,
    }
}
