// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Argument parsing, the environment a run is given, and the exit codes.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use rust_mutants::runner::Cancel;

use clap::{Parser, Subcommand, ValueEnum};

/// The code every verdict that establishes something earns; [`exit_codes`] says which.
pub const EXIT_ASSURED: u8 = 0;
/// The code a run that found something earns; [`exit_codes`] says which.
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
pub const PROGRAM: &str = "njutest";

/// What a person reads at the foot of `--help`: every exit code, with the names that earn it.
#[must_use]
pub fn exit_codes() -> String {
    let mut named: std::collections::BTreeMap<u8, Vec<&'static str>> =
        std::collections::BTreeMap::new();
    for verdict in crate::report::Verdict::ALL {
        named
            .entry(verdict.exit_code())
            .or_default()
            .push(verdict.name());
    }
    for outcome in crate::assure::replay::Outcome::ALL {
        named
            .entry(outcome.exit_code())
            .or_default()
            .push(outcome.name());
    }
    let mut said = String::from("Exit codes:");
    for (code, names) in named {
        let mut line = names.join(", ");
        if code == EXIT_ERROR {
            line.push_str(", invalid input, or an infrastructure failure");
        }
        crate::text::append(&mut said, format_args!("\n  {code:<4} {line}"));
    }
    crate::text::append(
        &mut said,
        format_args!("\n  {EXIT_INTERRUPTED:<4} interrupted\n  {EXIT_TERMINATED:<4} terminated"),
    );
    said
}

/// What the machine is, as an argument.
#[derive(Debug, Clone)]
pub struct Environment {
    /// The whole environment, as names and values.
    pub vars: Vec<(OsString, OsString)>,
    /// Where the process was started.
    pub working_directory: PathBuf,
    /// The operating system's temporary directory.
    pub temp_directory: PathBuf,
    /// This program's own path, which `doctor` copies to measure what running a newly written file costs.
    ///
    /// An argument for the same reason the temporary directory is: the composition root is where the operating system is asked.
    /// It has to be a program somebody may copy and run — a system binary is signed in place and is killed when it is run from anywhere else — and this one is both to hand and known to run.
    pub program: PathBuf,
    /// The user's cache directory, which earlier outcomes live under.
    pub cache_directory: PathBuf,
    /// Raised when the process is asked to stop.
    /// The composition root owns the signals; every phase reads this flag.
    pub cancel: Cancel,
    /// What the composition root found out about where the output is going, which a renderer is given rather than asks (ADR 0001).
    pub terminal: crate::presentation::Terminal,
}

impl Environment {
    /// The workspace a command was pointed at: what it named, or where it was started.
    #[must_use]
    pub fn rooted(&self, named: Option<&Path>) -> PathBuf {
        named.map_or_else(
            || self.working_directory.clone(),
            |path| self.working_directory.join(path),
        )
    }
    /// The value of `name`, if the environment has one.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&OsStr> {
        rust_mutants::vars::var(&self.vars, name)
    }

    /// Where a user's caches belong, from `vars` alone: `XDG_CACHE_HOME`, then `HOME/.cache`, then `LOCALAPPDATA` on Windows.
    #[must_use]
    pub fn cache_directory_of(vars: &[(OsString, OsString)]) -> PathBuf {
        rust_mutants::userdirs::cache_directory(vars, ".njutest-cache")
    }
}

/// The parsed command line.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "njutest",
    version = crate::VERSION,
    about = "An audit-oriented assurance runner for Rust",
    long_about = "An audit-oriented assurance runner for Rust.\n\n\
        njutest connects ordinary Cargo tests with coverage routing, mutation testing through \
        rust-mutants, paired kill confirmation, a soundness phase, targeted fuzzing, explicit \
        integration resources, and reviewable repair candidates. It reports a verdict — \
        ASSURED, DEFECT, INSUFFICIENT, ERROR — and never a percentage.",
    after_help = exit_codes(),
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
    /// Verify the workspace and report a verdict.
    Verify(Verify),
    /// Write an annotated .njutest.toml.
    Init(Init),
    /// Say what a run would measure, without measuring it.
    Plan(Plan),
    /// Show what a completed run concluded.
    Report(Report),
    /// Show everything a run recorded about one mutant.
    Explain(Explain),
    /// Say what a run's recording holds behind one claim.
    Why(Why),
    /// Say, item by item, what a run found pinned and what it found free.
    Spec(Spec),
    /// Draw one file as a run measured it, each changed line marked with where it stands.
    Guard(Guard),
    /// Record that a reviewer looked at a surviving mutant.
    Accept(Accept),
    /// Go through one run's gaps, one at a time, deciding as you read.
    Review(Review),
    /// Say what repairs a run was offered, and write the ones that hold up.
    Fix(Fix),
    /// Put one finding back to the tests and say whether it is still there.
    Replay(Replay),
    /// Record what each test target enters, for `njutest select` to read.
    Measure(Measure),
    /// Say which test targets can notice what changed since `njutest measure`.
    Select(Select),
    /// Read what a run recorded.
    Trace {
        /// What to read.
        #[command(subcommand)]
        command: TraceCommand,
    },
    /// Bundle everything about one run into one directory.
    Diagnostics(Diagnostics),
    /// Report the toolchain and the tools a run needs.
    Doctor(Doctor),
    /// Say what earlier runs left behind, and collect what is no longer an answer.
    Cache(Cache),
    /// Combine the reports of the parts of one catalog into the report the whole would have written.
    Merge(Merge),
    /// Verify again every time the workspace changes, until interrupted.
    Watch(Watch),
    /// Serve what the latest run found to an editor, over the language server protocol.
    Lsp(Lsp),
}

/// `njutest lsp`.
#[derive(Debug, Clone, clap::Args)]
pub struct Lsp {
    /// The workspace whose reports are served.
    /// The working directory by default.
    #[arg(long, value_name = "DIR")]
    pub directory: Option<PathBuf>,
}

/// `njutest watch`.
#[derive(Debug, Clone, clap::Args)]
pub struct Watch {
    /// How often to ask the workspace whether it changed.
    /// 500 by default.
    #[arg(long, value_name = "MS")]
    pub poll_ms: Option<u64>,
    /// What each round verifies.
    #[command(flatten)]
    pub verify: Verify,
}

/// `njutest merge`.
#[derive(Debug, Clone, clap::Args)]
pub struct Merge {
    /// The reports to combine, one per part.
    #[arg(value_name = "REPORT", required = true)]
    pub reports: Vec<PathBuf>,
    /// Write the combined report here rather than to standard output.
    #[arg(long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

/// `njutest cache`.
#[derive(Debug, Clone, clap::Args)]
pub struct Cache {
    /// The workspace whose configuration bounds the store.
    /// The working directory by default.
    #[arg(long, value_name = "DIR")]
    pub directory: Option<PathBuf>,
    /// Remove what has expired, then the oldest of what is left until the store is under its size.
    #[arg(long)]
    pub gc: bool,
    /// Write every answer this machine holds to FILE, one to a line, for another machine to read.
    #[arg(long, value_name = "FILE")]
    pub export: Option<PathBuf>,
    /// Read answers another machine wrote into this machine's store, holding each to what a run of this one would keep.
    #[arg(long, value_name = "FILE", conflicts_with = "export")]
    pub import: Option<PathBuf>,
}

/// `njutest verify`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one command line flag, and a flag is a bool wherever it is stored"
)]
#[derive(Debug, Clone, clap::Args)]
pub struct Verify {
    /// The workspace to verify.
    /// The working directory by default.
    #[arg(long, value_name = "DIR")]
    pub directory: Option<PathBuf>,
    /// The configuration file.
    /// `.njutest.toml` beside the workspace by default; a missing one is the defaults.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Verify only this package.
    /// Repeatable; the default is every member.
    #[arg(long = "package", short = 'p', value_name = "NAME")]
    pub packages: Vec<String>,
    /// How to write progress.
    #[arg(long, value_enum, default_value_t = Ui::Plain)]
    pub ui: Ui,
    /// What to write when the run is over.
    /// The default is the drawing at a terminal and the record stream anywhere else.
    #[arg(long, value_enum, value_name = "FORMAT")]
    pub format: Option<Format>,
    /// Record what the run does, under DIR or `.njutest/trace/<run>`.
    #[arg(
        long,
        value_name = "DIR",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = ""
    )]
    pub trace: Option<String>,
    /// Keep the directories the run worked in.
    #[arg(long)]
    pub keep_temp: bool,
    /// Pass --offline to every cargo command.
    #[arg(long)]
    pub offline: bool,
    /// Pass --locked to every cargo command.
    #[arg(long)]
    pub locked: bool,
    /// Establish everything afresh instead of reading back what an earlier run of the same inputs established.
    #[arg(long)]
    pub no_cache: bool,
    /// Mutate only the files that differ from a revision.
    /// `CHANGE_ASSURED` is the most this can conclude.
    #[arg(long)]
    pub changed: bool,
    /// The revision to compare against.
    /// Implies --changed.
    #[arg(long, value_name = "REV")]
    pub changed_from: Option<String>,
    /// Judge only one part of the catalog, as `K/N`.
    /// Every part measures the whole baseline; `njutest merge` combines what they judged.
    #[arg(long, value_name = "K/N")]
    pub shard: Option<String>,
    /// Arguments for the test binaries.
    /// Only the flags njutest does not own are allowed: --test-threads, --include-ignored, --nocapture, --show-output.
    #[arg(last = true, value_name = "TEST ARGS")]
    pub test_args: Vec<String>,
}

/// `njutest init`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub struct Init {
    /// Replace a configuration that is already there.
    #[arg(long)]
    pub force: bool,
}

/// `njutest doctor`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub struct Doctor {}

/// `njutest plan`.
#[derive(Debug, Clone, clap::Args)]
pub struct Plan {
    /// The workspace to plan for.
    /// The working directory by default.
    #[arg(long, value_name = "DIR")]
    pub directory: Option<PathBuf>,
    /// Plan only this package.
    /// Repeatable.
    #[arg(long = "package", short = 'p', value_name = "NAME")]
    pub packages: Vec<String>,
    /// Say what put each target in scope.
    #[arg(long)]
    pub why: bool,
    /// Pass --offline to every cargo command.
    #[arg(long)]
    pub offline: bool,
    /// Pass --locked to every cargo command.
    #[arg(long)]
    pub locked: bool,
}

/// `njutest report`.
#[derive(Debug, Clone, clap::Args)]
pub struct Report {
    /// The run to show.
    /// The latest by default.
    #[arg(value_name = "RUN")]
    pub run: Option<String>,
    /// How to write it.
    #[arg(long, value_enum, default_value_t = Format::Lines)]
    pub format: Format,
}

/// `njutest explain`.
#[derive(Debug, Clone, clap::Args)]
pub struct Explain {
    /// The mutant, by identity or by any prefix that names exactly one.
    #[arg(value_name = "MUTANT")]
    pub mutant: String,
    /// The run to read.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
}

/// `njutest spec`.
#[derive(Debug, Clone, clap::Args)]
pub struct Spec {
    /// What to specify: a file, an item as the source names it, or `PATH:ITEM`.
    /// Every item the run changed by default.
    #[arg(value_name = "SUBJECT")]
    pub subject: Option<String>,
    /// The run to read.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
}

/// `njutest guard`.
#[derive(Debug, Clone, clap::Args)]
pub struct Guard {
    /// The file to draw, from the project's root.
    #[arg(value_name = "PATH")]
    pub path: String,
    /// The run to read.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
}

/// `njutest why`.
#[derive(Debug, Clone, clap::Args)]
pub struct Why {
    /// The run to read.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
    /// What to ask about.
    #[command(subcommand)]
    pub claim: Asked,
}

/// What `njutest why` is asked about.
///
/// A subcommand rather than a flag with a value, because both halves of the product mint sixty-four hex characters in separate identity domains and the name a person types does not say which was meant.
/// Two variants the parser makes a caller choose between beat two optional fields with a note saying exactly one must be set.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum Asked {
    /// One mutation of the source.
    Mutation {
        /// Its identity.
        #[arg(value_name = "MUTANT")]
        id: String,
    },
    /// One question about one exchange on one seam.
    Seam {
        /// Its identity.
        #[arg(value_name = "QUESTION")]
        id: String,
    },
}

impl Asked {
    /// The claim this asks about.
    #[must_use]
    pub fn asked(&self) -> crate::why::Claim {
        match self {
            Self::Mutation { id } => crate::why::Claim::Mutation(id.clone()),
            Self::Seam { id } => crate::why::Claim::Seam(id.clone()),
        }
    }
}

/// `njutest replay`.
#[derive(Debug, Clone, clap::Args)]
pub struct Replay {
    /// The finding, by subject or by any prefix that names exactly one.
    #[arg(value_name = "FINDING")]
    pub finding: String,
    /// The run to read the finding from.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
    /// Pass `--offline` to cargo.
    #[arg(long)]
    pub offline: bool,
    /// Pass `--locked` to cargo.
    #[arg(long)]
    pub locked: bool,
}

/// `njutest measure`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub struct Measure {
    /// Pass `--offline` to cargo.
    #[arg(long)]
    pub offline: bool,
    /// Pass `--locked` to cargo.
    #[arg(long)]
    pub locked: bool,
}

/// How `njutest select` says what it decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, njutest_macros::AllVariants)]
pub enum SelectFormat {
    /// Every target, whether it runs, and why.
    Human,
    /// A nextest filterset that leaves out every target proved unable to notice the change.
    Nextest,
    /// The targets proved unable to notice the change, one per line.
    Skippable,
}

/// `njutest select`.
#[derive(Debug, Clone, Copy, clap::Args)]
pub struct Select {
    /// How to say what was decided.
    #[arg(long, value_enum, default_value_t = SelectFormat::Human)]
    pub format: SelectFormat,
    /// Pass `--offline` to cargo.
    #[arg(long)]
    pub offline: bool,
    /// Pass `--locked` to cargo.
    #[arg(long)]
    pub locked: bool,
}

/// `njutest review`.
#[derive(Debug, Clone, clap::Args)]
pub struct Review {
    /// The run to go through.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
}

/// `njutest accept`.
#[derive(Debug, Clone, clap::Args)]
pub struct Accept {
    /// The mutant, by identity or by any prefix that names exactly one.
    #[arg(value_name = "MUTANT")]
    pub mutant: String,
    /// Why it may survive.
    /// Required: an acceptance without one is a mutant nobody looked at.
    #[arg(long)]
    pub reason: String,
    /// Who decided.
    #[arg(long)]
    pub owner: Option<String>,
    /// Where the decision is recorded.
    #[arg(long)]
    pub ticket: Option<String>,
    /// When to look again, as RFC3339.
    /// An acceptance with none never lapses.
    #[arg(long, value_name = "WHEN")]
    pub expires: Option<jiff::Timestamp>,
    /// The run that measured it.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
}

/// `njutest fix`.
#[derive(Debug, Clone, clap::Args)]
pub struct Fix {
    /// The run whose candidates to read.
    /// The latest by default.
    #[arg(long, value_name = "RUN")]
    pub run: Option<String>,
    /// Write the candidates that hold up, rather than only saying what they are.
    #[arg(long)]
    pub apply: bool,
    /// Write only this candidate, by its digest or any prefix that names exactly one.
    #[arg(long, value_name = "DIGEST")]
    pub candidate: Option<String>,
    /// Never touch the network.
    #[arg(long)]
    pub offline: bool,
    /// Refuse to change `Cargo.lock`.
    #[arg(long)]
    pub locked: bool,
}

/// `njutest diagnostics`.
#[derive(Debug, Clone, clap::Args)]
pub struct Diagnostics {
    /// The run to bundle.
    #[arg(value_name = "RUN")]
    pub run: String,
}

/// `njutest trace`.
#[derive(Debug, Clone, Subcommand)]
#[non_exhaustive]
pub enum TraceCommand {
    /// What one recording holds, and what is wrong with it.
    Summary {
        /// The run to read.
        /// The latest by default.
        #[arg(value_name = "RUN")]
        run: Option<String>,
    },
    /// What moved between two recordings, without replaying either.
    Diff {
        /// The earlier run.
        #[arg(value_name = "RUN-A")]
        a: String,
        /// The later run.
        #[arg(value_name = "RUN-B")]
        b: String,
    },
}

/// What a run, stored or just finished, is written out as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum Format {
    /// The record stream.
    Lines,
    /// The canonical document, exactly as the run wrote it.
    Json,
    /// What the seams the run watched observed the system doing, and who holds each sentence up.
    Spec,
    /// What a person reads: the source, with every place the tests do not see marked on it.
    Human,
    /// A briefing for something that will act on this without a screen, as Markdown.
    Agent,
}

impl Format {
    const DEFAULT_OUTPUT: Self = Self::Lines;
}

impl Default for Format {
    fn default() -> Self {
        Self::DEFAULT_OUTPUT
    }
}

/// How a run writes what it is doing while it does it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, njutest_macros::AllVariants)]
#[value(rename_all = "lower")]
pub enum Ui {
    /// Lines a person reads.
    Plain,
    /// One JSON object per line, for a program.
    Jsonl,
    /// One block that says where the run is, rewritten in place.
    Dashboard,
}

impl Ui {
    const DEFAULT_PRESENTATION: Self = Self::Plain;
}

impl Default for Ui {
    fn default() -> Self {
        Self::DEFAULT_PRESENTATION
    }
}

/// A command line that could not be parsed, or a request to print help or the version, rendered for the stream it belongs on.
#[derive(Debug)]
pub struct Usage {
    /// The text to write, newline-terminated.
    pub text: String,
    /// Whether the text belongs on stderr (a diagnostic) or stdout (help).
    pub to_stderr: bool,
    /// The exit code the process ends with.
    pub exit_code: u8,
}

/// The same arguments, less the word cargo repeats when it calls a subcommand.
fn subcommand(mut args: Vec<OsString>) -> Vec<OsString> {
    let called_by_cargo = args
        .first()
        .and_then(|name| Path::new(name).file_stem())
        .is_some_and(|stem| stem == "cargo-njutest");
    if called_by_cargo && args.get(1).is_some_and(|word| word == "njutest") {
        let repeated = args.remove(1);
        assert_eq!(repeated, "njutest", "the guarded cargo alias is exact");
    }
    args
}

/// Parses `args`, program name first.
/// A bare invocation is the help text, as it is in goatest.
///
/// # Errors
/// Returns the rendered usage error, help text, or version text.
pub fn parse<I>(args: I) -> Result<Request, Usage>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args: Vec<OsString> = subcommand(args.into_iter().collect());
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

/// Renders a diagnostic the way every njutest diagnostic is rendered: one `njutest: ` prefix, a lowercase message, and the usage that follows it.
fn diagnose(rendered: &str) -> String {
    let message = rendered.strip_prefix("error: ").unwrap_or(rendered);
    format!("{PROGRAM}: {message}")
}
