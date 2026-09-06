// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Argument parsing. This module knows the command tree and nothing about executing it.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// The parsed command line.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "rust-mutants",
    version = rust_mutants::VERSION,
    about = "Mutation testing for Rust: one instrumented snapshot, every mutant behind a guard",
    long_about = "Mutation testing for Rust and Cargo.\n\n\
        rust-mutants copies the workspace into a disposable snapshot, instruments every \
        compilable mutant of the selected files once, builds the test binaries once, and \
        activates one mutant per test process through an environment variable. The source \
        workspace is never modified.",
    arg_required_else_help = true,
    term_width = 100,
    color = clap::ColorChoice::Never
)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// List the candidates the rules propose, before the compiler has ruled.
    List {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
    },
    /// Ask the compiler, for every mutant, whether it renders it identically to the code it mutates.
    Equivalence {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
        /// Ask about at most this many mutants, in catalog order. Zero asks about every one of them.
        #[arg(long, value_name = "N", default_value_t = 0)]
        limit: usize,
    },
    /// Catalog the mutants the compiler accepts, and the candidates it refused with its own words.
    Catalog {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
        /// Print the catalog as one JSON document.
        #[arg(long)]
        json: bool,
    },
    /// Run the mutants and report what the tests noticed. Every accepted mutant unless one is named.
    Run {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
        /// Run only this mutant, by identity or by any prefix that names exactly one.
        #[arg(long, value_name = "PREFIX")]
        mutant: Option<String>,
        /// With `--mutant`, run only this target, by `package/kind/name` or by name.
        #[arg(long, value_name = "TARGET", requires = "mutant")]
        target: Option<String>,
        /// With `--mutant`, run only this test, by its libtest path.
        #[arg(long, value_name = "TEST", requires = "mutant")]
        test: Option<String>,
        /// Run only this part of the catalog, as `K/N`. Every part of one tree cuts the same way, so the parts together are the whole.
        #[arg(long, value_name = "K/N", conflicts_with = "mutant")]
        shard: Option<String>,
        /// Do not write a run report under the report directory.
        #[arg(long, conflicts_with = "mutant")]
        no_report: bool,
        /// Execute every mutant afresh rather than reading back what an earlier run of this exact tree established.
        #[arg(long, conflicts_with = "mutant")]
        no_cache: bool,
        /// Arguments for the test harness itself.
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<String>,
    },
    /// Say everything known about one mutant.
    Explain {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
        /// The mutant, by identity or by any prefix that names exactly one.
        #[arg(value_name = "PREFIX")]
        mutant: String,
    },
    /// Print one file as the engine rewrites it, guards and runtime included.
    Instrument {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
        /// The file, as a workspace-relative path.
        #[arg(long, value_name = "PATH")]
        file: String,
    },
    /// Tally why places were passed over.
    WhySkipped {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
    },
    /// Write a `.rust-mutants.toml` whose every value is already the default.
    Init {
        /// The workspace root. Defaults to the working directory.
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// Overwrite a file that is already there.
        #[arg(long)]
        force: bool,
    },
    /// Say what a run would find in this environment: the toolchain, the configuration, the temporary directory.
    Doctor {
        /// The workspace root. Defaults to the working directory.
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// Print the `rust-mutants/doctor` document rather than the lines a person reads.
        #[arg(long)]
        json: bool,
    },
    /// Combine the reports of the parts of one catalog into the report the whole would have written.
    Merge {
        /// The reports to combine, one per part.
        #[arg(value_name = "REPORT", required = true)]
        reports: Vec<PathBuf>,
        /// Write the combined report here rather than to standard output.
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Read a recording back: what it counted, what each phase took, and what moved between two of them.
    Trace {
        /// Which reading.
        #[command(subcommand)]
        command: TraceCommand,
    },
    /// Read back a stored run report.
    Report {
        /// The workspace root. Defaults to the working directory.
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// The run, by its identity. Defaults to the newest.
        #[arg(long, value_name = "ID")]
        run: Option<String>,
        /// How to write it.
        #[arg(long, value_enum, value_name = "FORMAT", default_value_t = Format::Lines)]
        format: Format,
        /// Write it here rather than to standard output.
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// Read it at the terminal instead of writing it.
        #[arg(long, conflicts_with_all = ["format", "output"])]
        tui: bool,
    },
    /// Say what the engine left in the temporary directory, and remove what no run still owns.
    Cache {
        /// Remove every abandoned snapshot and target directory.
        #[arg(long)]
        gc: bool,
    },
}

/// How a stored run is written back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// The lines a person reads.
    Lines,
    /// The stored `rust-mutants/run-report` document, verbatim.
    Json,
    /// One self-contained HTML page.
    Html,
    /// The mutation testing report every Stryker reader understands.
    Stryker,
}

/// What a command reads, and how much of it. Every value here also has a key in `.rust-mutants.toml`; a flag given on the command line wins.
#[derive(Debug, Clone, Args)]
pub struct Scope {
    /// The workspace root. Defaults to the working directory.
    #[arg(long, value_name = "DIR")]
    pub root: Option<PathBuf>,
    /// Read this configuration file instead of the one in the workspace root.
    #[arg(long, value_name = "FILE", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,
    /// Read no configuration file at all.
    #[arg(long)]
    pub no_config: bool,
    /// Which tier of operators to apply.
    #[arg(long, value_enum, value_name = "TIER")]
    pub tier: Option<TierArg>,
    /// Apply exactly these operators, by name. Repeatable.
    #[arg(long = "operator", value_name = "RULE")]
    pub operators: Vec<String>,
    /// Only mutate files matching this pattern. Repeatable.
    #[arg(long = "include", value_name = "GLOB")]
    pub include: Vec<String>,
    /// Never mutate files matching this pattern. Repeatable.
    #[arg(long = "exclude", value_name = "GLOB")]
    pub exclude: Vec<String>,
    /// Mutate only the files that differ from `HEAD`, committed and not. Narrows `--include` rather than widening it.
    #[arg(long, conflicts_with = "changed_from")]
    pub changed: bool,
    /// Mutate only the files that differ from this revision.
    #[arg(long, value_name = "REV")]
    pub changed_from: Option<String>,
    /// Only mutate these packages. Repeatable.
    #[arg(long = "package", short = 'p', value_name = "NAME")]
    pub packages: Vec<String>,
    /// Let the build read this directory from outside the root, copying it beside the tree. Repeatable.
    #[arg(long = "allow-outside", value_name = "DIR")]
    pub allow_outside: Vec<PathBuf>,
    /// Compile with these cargo features. Repeatable, and each may be a comma-separated list.
    #[arg(long = "features", value_name = "LIST", value_delimiter = ',')]
    pub features: Vec<String>,
    /// Compile for this target triple instead of the host. Not `run --target`, which names a test target.
    #[arg(long = "build-target", value_name = "TRIPLE")]
    pub build_target: Option<String>,
    /// Compile with this cargo profile.
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,
    /// How many compilation jobs cargo may run at once.
    #[arg(long = "build-jobs", value_name = "N")]
    pub build_jobs: Option<u32>,
    /// How many mutants to measure at once. Zero is as many as the machine has, capped at four.
    #[arg(long, short = 'j', value_name = "N")]
    pub jobs: Option<usize>,
    /// Never start this target, by the id a report names it with. Repeatable.
    #[arg(long = "skip-target", value_name = "PKG/KIND/NAME")]
    pub skip_targets: Vec<String>,
    /// How long one mutant execution may take before it is retried serially, as in `90s` or `5m`.
    #[arg(long, value_name = "DURATION")]
    pub timeout: Option<String>,
    /// Record what the run does, as JSON Lines. Without a directory a run writes beside its report and every other command under `<reports>/traces/`.
    #[arg(long, value_name = "DIR", num_args = 0..=1, default_missing_value = "")]
    pub trace: Option<String>,
    /// How the workspace is treated.
    #[command(flatten)]
    pub switches: Switches,
}

/// The plain yes-or-no choices, kept together so the scope reads as what it selects rather than as a row of flags.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one command line flag, and a flag is a bool wherever it is stored"
)]
#[derive(Debug, Clone, Copy, Args)]
pub struct Switches {
    /// Never touch the network.
    #[arg(long)]
    pub offline: bool,
    /// Refuse to change `Cargo.lock`.
    #[arg(long)]
    pub locked: bool,
    /// Keep the snapshot and the target directory instead of removing them.
    #[arg(long)]
    pub keep_temp: bool,
    /// Skip the run of every target with nothing active.
    #[arg(long)]
    pub no_verify: bool,
    /// Measure once which target reached what, and run a mutant only against the targets that reached it.
    #[arg(long)]
    pub coverage: bool,
    /// Run every mutant against every target, measuring no coverage and proving nothing about reach.
    #[arg(long, conflicts_with = "coverage")]
    pub no_coverage: bool,
    /// Leave a library's documented examples out of the targets.
    #[arg(long)]
    pub no_doctests: bool,
    /// Compile with every feature of every selected package.
    #[arg(long)]
    pub all_features: bool,
    /// Compile with the default features off.
    #[arg(long)]
    pub no_default_features: bool,
}

/// The tiers, as the command line spells them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TierArg {
    /// The operators worth running everywhere.
    Balanced,
    /// Balanced, plus the bitwise and compound-assignment families.
    Strong,
    /// Every operator, statement deletion included.
    All,
}

impl TierArg {
    /// The engine's tier.
    #[must_use]
    pub const fn tier(self) -> rust_mutants::rule::Tier {
        match self {
            Self::Balanced => rust_mutants::rule::Tier::Balanced,
            Self::Strong => rust_mutants::rule::Tier::Strong,
            Self::All => rust_mutants::rule::Tier::All,
        }
    }
}

impl Command {
    /// What the command reads, for the commands that read a workspace.
    #[must_use]
    pub const fn scope(&self) -> Option<&Scope> {
        match self {
            Self::List { scope }
            | Self::Equivalence { scope, .. }
            | Self::Catalog { scope, .. }
            | Self::Run { scope, .. }
            | Self::Explain { scope, .. }
            | Self::Instrument { scope, .. }
            | Self::WhySkipped { scope } => Some(scope),
            Self::Init { .. }
            | Self::Doctor { .. }
            | Self::Merge { .. }
            | Self::Report { .. }
            | Self::Trace { .. }
            | Self::Cache { .. } => None,
        }
    }

    /// The workspace root the command names, when it names one.
    #[must_use]
    pub const fn root(&self) -> Option<&PathBuf> {
        match self {
            Self::Init { root, .. } | Self::Doctor { root, .. } | Self::Report { root, .. } => {
                root.as_ref()
            }
            Self::Cache { .. } | Self::Merge { .. } => None,
            _ => match self.scope() {
                Some(scope) => scope.root.as_ref(),
                None => None,
            },
        }
    }
}

/// A command that could not be parsed, or a request to print help or the version, rendered for the stream it belongs on.
#[derive(Debug)]
pub struct Usage {
    /// The text to write, newline-terminated.
    pub text: String,
    /// Whether the text belongs on stderr (a diagnostic) or stdout (help).
    pub to_stderr: bool,
    /// The exit code the process ends with.
    pub exit_code: u8,
}

/// Parses `args`, program name first.
///
/// # Errors
/// Returns the rendered usage error, help text, or version text.
pub fn parse<I>(args: I) -> Result<Cli, Usage>
where
    I: IntoIterator<Item = OsString>,
{
    Cli::try_parse_from(args).map_err(|error| {
        let to_stderr = error.use_stderr();
        Usage {
            text: error.render().to_string(),
            to_stderr,
            exit_code: if to_stderr { crate::EXIT_USAGE } else { 0 },
        }
    })
}

/// What to ask of a recording.
#[derive(Debug, Clone, Subcommand)]
pub enum TraceCommand {
    /// What a recording counted, what every phase took, and which commands were the slowest.
    Summary {
        /// The workspace root. Defaults to the working directory.
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// Read this run's recording rather than the newest one.
        #[arg(long, value_name = "ID")]
        run: Option<String>,
        /// Read a recording in this directory instead of one under the report directory.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
        /// Name at most this many of the slowest commands.
        #[arg(long, value_name = "N", default_value_t = 5)]
        slowest: usize,
    },
    /// Say whether a recording is complete: it begins, it ends, it lost nothing, and every phase it opened it closed.
    Check {
        /// The workspace root. Defaults to the working directory.
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// Check this run's recording rather than the newest one.
        #[arg(long, value_name = "ID")]
        run: Option<String>,
        /// Check a recording in this directory instead of one under the report directory.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    /// What moved between two recordings.
    Diff {
        /// The workspace root. Defaults to the working directory.
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// The run to read first.
        a: String,
        /// The run to read second.
        b: String,
    },
}
