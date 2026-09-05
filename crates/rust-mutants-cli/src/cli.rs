// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Argument parsing. This module knows the command tree and nothing about
//! executing it.

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
    ///
    /// Fast: the workspace is copied and type-checked, but nothing is
    /// instrumented and nothing is built.
    List {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
    },
    /// Catalog the mutants the compiler accepts, and the candidates it
    /// refused with its own words.
    Catalog {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
        /// Print the catalog as one JSON document.
        #[arg(long)]
        json: bool,
    },
    /// Run one mutant and report what the tests said.
    Run {
        /// Which workspace to read.
        #[command(flatten)]
        scope: Scope,
        /// The mutant, by identity or by any prefix that names exactly one.
        #[arg(long, value_name = "PREFIX")]
        mutant: String,
        /// Run only this target, by `package/kind/name` or by name.
        #[arg(long, value_name = "TARGET")]
        target: Option<String>,
        /// Run only this test, by its libtest path.
        #[arg(long, value_name = "TEST")]
        test: Option<String>,
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
    /// Print one file as the engine rewrites it, guards and runtime
    /// included.
    ///
    /// The tree is copied and type-checked, but nothing is built and
    /// nothing is validated: what it prints is every candidate the rules
    /// propose, which is what a person debugging a guard wants to see.
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
}

/// What a command reads, and how much of it.
#[derive(Debug, Clone, Args)]
pub struct Scope {
    /// The workspace root. Defaults to the working directory.
    #[arg(long, value_name = "DIR")]
    pub root: Option<PathBuf>,
    /// Which tier of operators to apply.
    #[arg(long, value_enum, default_value_t = TierArg::Balanced)]
    pub tier: TierArg,
    /// Apply exactly these operators, by name. Repeatable.
    #[arg(long = "operator", value_name = "RULE")]
    pub operators: Vec<String>,
    /// Only mutate files matching this pattern. Repeatable.
    #[arg(long = "include", value_name = "GLOB")]
    pub include: Vec<String>,
    /// Never mutate files matching this pattern. Repeatable.
    #[arg(long = "exclude", value_name = "GLOB")]
    pub exclude: Vec<String>,
    /// Only mutate these packages. Repeatable.
    #[arg(long = "package", short = 'p', value_name = "NAME")]
    pub packages: Vec<String>,
    /// How the workspace is treated.
    #[command(flatten)]
    pub switches: Switches,
}

/// The plain yes-or-no choices, kept together so the scope reads as what it
/// selects rather than as a row of flags.
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
    /// What the command reads.
    #[must_use]
    pub const fn scope(&self) -> &Scope {
        match self {
            Self::List { scope }
            | Self::Catalog { scope, .. }
            | Self::Run { scope, .. }
            | Self::Explain { scope, .. }
            | Self::Instrument { scope, .. }
            | Self::WhySkipped { scope } => scope,
        }
    }
}

/// A command that could not be parsed, or a request to print help or the
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

/// Parses `args`, program name first.
///
/// # Errors
///
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
