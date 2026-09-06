// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.rust-mutants.toml`: optional, strict, and defaulted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_mutants::error::{self, ErrorCode};
use rust_mutants::glob::Pattern;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::session::Timeout;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The file a run reads, in the workspace root.
pub const FILE_NAME: &str = ".rust-mutants.toml";

/// How long one mutant execution may take when the file does not say.
pub const DEFAULT_TIMEOUT: Timeout = Timeout::Auto;

/// Where run reports are written when the file does not say.
pub const DEFAULT_REPORTS_DIRECTORY: &str = "reports/mutation";

/// How many run directories are kept when the file does not say.
pub const DEFAULT_REPORTS_KEEP: u32 = 20;

/// The harness flags a run may pass through.
pub const ALLOWED_TEST_ARGS: [&str; 4] = [
    "--test-threads",
    "--include-ignored",
    "--nocapture",
    "--show-output",
];

/// Everything `.rust-mutants.toml` can say.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// The schema version. Only `1` is understood.
    pub version: u32,
    /// What is mutated.
    pub project: Project,
    /// What the project is compiled as.
    pub build: Build,
    /// How mutants are proposed and executed.
    pub mutation: Mutation,
    /// How the workspace is built and the tests are run.
    pub execution: Execution,
    /// What is written under the report directory.
    pub reports: Reports,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            project: Project::default(),
            build: Build::default(),
            mutation: Mutation::default(),
            execution: Execution::default(),
            reports: Reports::default(),
        }
    }
}

/// What is mutated.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Project {
    /// The cargo packages to mutate. Empty is every workspace member.
    pub packages: Vec<String>,
    /// Workspace-relative globs a file must match to be mutable.
    pub include: Vec<String>,
    /// Workspace-relative globs that remove a file again.
    pub exclude: Vec<String>,
    /// Directories outside the root the workspace may read code from.
    ///
    /// A run measures a copy of the tree, so a path dependency outside it is
    /// not in the copy. Naming a directory here says the run may copy it
    /// beside the tree, which makes the measurement about a tree that is not
    /// the one on disk: a decision for a person rather than one a run takes.
    pub allow_outside: Vec<String>,
}

/// What the project is compiled as.
///
/// Cargo compiles a different program for a different feature set, target
/// triple, or profile. A run that measures one of them while the project
/// ships another measures a program nobody runs, so these are the words a
/// person would have typed, passed on unchanged. An empty name and a zero are
/// what nobody said.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Build {
    /// The features to turn on.
    pub features: Vec<String>,
    /// Turn on every feature of every selected package.
    pub all_features: bool,
    /// Leave the default features off.
    pub no_default_features: bool,
    /// The target triple to compile for. Empty is the host.
    pub target: String,
    /// The cargo profile to compile with. Empty is each command's own default.
    pub profile: String,
    /// How many compilation jobs cargo may run at once. Zero lets cargo choose.
    pub jobs: u32,
}

impl Build {
    /// What the engine is told to compile.
    #[must_use]
    pub fn config(&self) -> rust_mutants::cargo::BuildConfig {
        let named = |value: &str| (!value.is_empty()).then(|| value.to_owned());
        rust_mutants::cargo::BuildConfig {
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target: named(&self.target),
            profile: named(&self.profile),
            jobs: (self.jobs > 0).then_some(self.jobs),
        }
    }
}

/// How mutants are proposed and executed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Mutation {
    /// Which tier of operators to apply when `operators` is empty.
    #[serde(deserialize_with = "tier", serialize_with = "tier_name")]
    pub tier: Tier,
    /// Exactly these rules, by name. Empty means the tier.
    pub operators: Vec<String>,
    /// How long one mutant execution may take before it is confirmed with the machine to itself. `auto` is a multiple of what the target's own baseline took.
    #[serde(deserialize_with = "timeout", serialize_with = "timeout_text")]
    pub timeout: Timeout,
    /// How long a build may take. `None` is no bound.
    #[serde(
        deserialize_with = "optional_duration",
        serialize_with = "optional_duration_text"
    )]
    pub build_timeout: Option<Duration>,
    /// Run every test target once with nothing active before believing anything a mutant does.
    pub verify: bool,
    /// Measure once which target reached what, and run a mutant only against the targets that reached it.
    pub coverage: bool,
    /// The mutants a reviewer declared equivalent, with the outcome the run must confirm.
    pub expect: Vec<Expectation>,
}

impl Default for Mutation {
    fn default() -> Self {
        Self {
            tier: Tier::Balanced,
            operators: Vec::new(),
            timeout: Timeout::Auto,
            build_timeout: None,
            verify: true,
            coverage: false,
            expect: Vec::new(),
        }
    }
}

/// One mutant whose outcome a reviewer declared in advance, so the run verifies the claim instead of hiding the mutant.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    /// The mutant, by identity or by a prefix that names exactly one.
    pub id: String,
    /// Why the outcome is what it is. Required: an expectation without a reason is a suppression, and a report cannot audit one.
    pub reason: String,
    /// The outcome the run must confirm.
    #[serde(
        default = "expected_by_default",
        deserialize_with = "outcome",
        serialize_with = "outcome_name"
    )]
    pub outcome: Outcome,
}

const fn expected_by_default() -> Outcome {
    Outcome::Survived
}

/// How the workspace is built and the tests are run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Execution {
    /// Never touch the network.
    pub offline: bool,
    /// Refuse to change `Cargo.lock`.
    pub locked: bool,
    /// Harness flags to pass through; see [`ALLOWED_TEST_ARGS`].
    pub test_binary_args: Vec<String>,
    /// Run a library's documented examples as a target of their own.
    ///
    /// A documented example is a test the project wrote, and a mutation only
    /// one of them can notice is one nothing else in the suite covers.
    pub doctests: bool,
    /// Targets never to start, by the id a report names them with.
    ///
    /// A suite whose tests are about the text of what the compiler said fails
    /// under instrumentation for a reason that is not the mutation. Naming it
    /// here is a decision somebody made, and the report says so.
    pub skip_targets: Vec<String>,
}

impl Default for Execution {
    fn default() -> Self {
        Self {
            offline: false,
            locked: false,
            doctests: true,
            test_binary_args: Vec::new(),
            skip_targets: Vec::new(),
        }
    }
}

/// What is written under the report directory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Reports {
    /// Where run reports go, workspace-relative.
    pub directory: PathBuf,
    /// How many run directories are kept.
    pub keep: u32,
}

impl Default for Reports {
    fn default() -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_REPORTS_DIRECTORY),
            keep: DEFAULT_REPORTS_KEEP,
        }
    }
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigErrorKind {
    /// The file could not be read.
    Unreadable,
    /// The file is not the document this version understands.
    Unparsable,
    /// The document parses but says something a run cannot honour.
    Invalid,
    /// The `version` is not one this release understands.
    UnsupportedVersion,
}

impl ConfigErrorKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 4] = [
        Self::Unreadable,
        Self::Unparsable,
        Self::Invalid,
        Self::UnsupportedVersion,
    ];

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::Unreadable => error::CONFIG_UNREADABLE,
            Self::Unparsable => error::CONFIG_UNPARSABLE,
            Self::Invalid => error::CONFIG_INVALID,
            Self::UnsupportedVersion => error::CONFIG_UNSUPPORTED_VERSION,
        }
    }
}

/// Why a configuration could not be used.
#[derive(Debug, thiserror::Error)]
#[error("{}: {path}: {message}", kind.code().code)]
pub struct ConfigError {
    kind: ConfigErrorKind,
    path: String,
    message: String,
}

impl ConfigError {
    fn new(kind: ConfigErrorKind, path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.display().to_string(),
            message: message.into(),
        }
    }

    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> ConfigErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }
}

impl ConfigError {
    /// The failure of reading a configuration file at `path`.
    #[must_use]
    pub fn unreadable(path: &Path, message: impl Into<String>) -> Self {
        Self::new(ConfigErrorKind::Unreadable, path, message)
    }
}

impl Config {
    /// Reads `.rust-mutants.toml` from `root`, or the defaults when there is none.
    ///
    /// # Errors
    /// See [`ConfigErrorKind`].
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let path = root.join(FILE_NAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(ConfigError::new(
                    ConfigErrorKind::Unreadable,
                    &path,
                    error.to_string(),
                ));
            }
        };
        Self::parse(&text, &path)
    }

    /// Reads a configuration from text, naming `path` in any error.
    ///
    /// # Errors
    /// See [`ConfigErrorKind`].
    pub fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text).map_err(|error| {
            ConfigError::new(ConfigErrorKind::Unparsable, path, one_line(&error))
        })?;
        config.validate(path)?;
        Ok(config)
    }

    /// Whether everything the document says can be honoured.
    fn validate(&self, path: &Path) -> Result<(), ConfigError> {
        let invalid = |message: String| ConfigError::new(ConfigErrorKind::Invalid, path, message);
        if self.version != 1 {
            return Err(ConfigError::new(
                ConfigErrorKind::UnsupportedVersion,
                path,
                format!(
                    "version {} is not one this release understands; only 1 is",
                    self.version
                ),
            ));
        }
        for pattern in self.project.include.iter().chain(&self.project.exclude) {
            Pattern::compile(pattern).map_err(|error| invalid(error.to_string()))?;
        }
        let registry = Registry::canonical();
        for operator in &self.mutation.operators {
            if registry.lookup(operator).is_none() {
                return Err(invalid(format!(
                    "operators names {operator:?}, which is not a rule this release knows"
                )));
            }
        }
        for argument in &self.execution.test_binary_args {
            if !allowed_test_arg(argument) {
                return Err(invalid(format!(
                    "test_binary_args holds {argument:?}, which the engine owns: it changes \
                     selection, repetition, the output protocol, or completeness. Allowed: {}",
                    ALLOWED_TEST_ARGS.join(", ")
                )));
            }
        }
        self.check_expectations(&invalid)?;
        self.check_reports(&invalid)
    }

    fn check_expectations(
        &self,
        invalid: &impl Fn(String) -> ConfigError,
    ) -> Result<(), ConfigError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for expectation in &self.mutation.expect {
            if expectation.id.trim().is_empty() {
                return Err(invalid(
                    "an expectation names no mutant; write the identity or a prefix of it"
                        .to_owned(),
                ));
            }
            if expectation.reason.trim().is_empty() {
                return Err(invalid(format!(
                    "the expectation for {:?} has a blank reason, which is a suppression rather \
                     than a claim a report can audit",
                    expectation.id
                )));
            }
            if !matches!(
                expectation.outcome,
                Outcome::Survived | Outcome::Killed | Outcome::TimedOut
            ) {
                return Err(invalid(format!(
                    "the expectation for {:?} expects {}, which is not an outcome a run confirms; \
                     write survived, killed, or timed_out",
                    expectation.id,
                    expectation.outcome.name()
                )));
            }
            if !seen.insert(expectation.id.as_str()) {
                return Err(invalid(format!(
                    "two expectations name {:?}; a mutant has one reason",
                    expectation.id
                )));
            }
        }
        Ok(())
    }

    fn check_reports(&self, invalid: &impl Fn(String) -> ConfigError) -> Result<(), ConfigError> {
        let directory = &self.reports.directory;
        if directory.as_os_str().is_empty() {
            return Err(invalid(
                "the report directory is empty; name one relative to the workspace root".to_owned(),
            ));
        }
        if directory.is_absolute()
            || directory
                .components()
                .any(|part| part == std::path::Component::ParentDir)
        {
            return Err(invalid(format!(
                "the report directory {} leaves the workspace; name one relative to its root",
                directory.display()
            )));
        }
        Ok(())
    }
}

/// Whether a harness flag is one a run passes through rather than one the engine owns.
#[must_use]
pub fn allowed_test_arg(argument: &str) -> bool {
    let name = argument.split_once('=').map_or(argument, |(name, _)| name);
    ALLOWED_TEST_ARGS.contains(&name)
}

fn one_line(error: &toml::de::Error) -> String {
    error
        .to_string()
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join("; ")
}

fn optional_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Duration>, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.is_empty() {
        return Ok(None);
    }
    rust_mutants::duration::parse(&text)
        .map(|value| (!value.is_zero()).then_some(value))
        .map_err(serde::de::Error::custom)
}

/// The word `auto`, or a duration.
fn timeout<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Timeout, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_timeout(&text).map_err(serde::de::Error::custom)
}

/// The word a person writes for a budget: `auto`, or a duration.
///
/// # Errors
/// Returns what is wrong with a duration that is not one.
pub fn parse_timeout(text: &str) -> Result<Timeout, rust_mutants::duration::DurationError> {
    if text.trim() == AUTO {
        return Ok(Timeout::Auto);
    }
    rust_mutants::duration::parse(text).map(Timeout::Fixed)
}

/// How a budget is written back.
#[must_use]
pub fn render_timeout(value: Timeout) -> String {
    match value {
        Timeout::Auto => AUTO.to_owned(),
        Timeout::Fixed(chosen) => rust_mutants::duration::render(chosen),
    }
}

/// The word that says a budget is derived from what a target's own baseline took.
pub const AUTO: &str = "auto";

fn timeout_text<S: Serializer>(value: &Timeout, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&render_timeout(*value))
}

#[expect(
    clippy::ref_option,
    reason = "serde's serialize_with hands the field by reference, whatever its shape"
)]
fn optional_duration_text<S: Serializer>(
    value: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(
        &value
            .map(rust_mutants::duration::render)
            .unwrap_or_default(),
    )
}

fn tier<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Tier, D::Error> {
    let text = String::deserialize(deserializer)?;
    Tier::parse(&text).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "{text:?} is not a tier; write {}",
            Tier::ALL.map(Tier::name).join(", ")
        ))
    })
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with hands the field by reference, whatever its shape"
)]
fn tier_name<S: Serializer>(value: &Tier, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.name())
}

fn outcome<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Outcome, D::Error> {
    let text = String::deserialize(deserializer)?;
    Outcome::parse(&text).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "{text:?} is not an outcome; write {}",
            Outcome::ALL.map(Outcome::name).join(", ")
        ))
    })
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with hands the field by reference, whatever its shape"
)]
fn outcome_name<S: Serializer>(value: &Outcome, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.name())
}

/// The annotated skeleton `rust-mutants init` writes. Every uncommented line is already the default, so the file a person starts from changes nothing.
#[must_use]
pub fn skeleton() -> String {
    format!(
        "\
# rust-mutants configuration. Every key below is optional; what is shown is
# the default. Unknown keys, malformed values, and any version other than 1
# end the run rather than being ignored.
version = 1

[project]
# packages = []                  # cargo package names; empty = every member
# include = []                   # workspace-relative globs a file must match
# exclude = []                   # workspace-relative globs that remove a file
# allow_outside = []             # directories outside the root the build may read

[build]
# features = []                   # cargo features to turn on
# all_features = false            # every feature of every selected package
# no_default_features = false     # leave the default features off
# target = \"\"                     # target triple; empty = the host
# profile = \"\"                    # cargo profile; empty = each command's default
# jobs = 0                        # cargo compilation jobs; 0 = cargo decides

[mutation]
# tier = \"{tier}\"            # {tiers}
# operators = []                 # exactly these rules; empty = the tier
# timeout = \"{timeout}\"                # auto = 5x the target's own baseline, never below 30s
# build_timeout = \"\"             # empty = no bound
# verify = true                  # run the instrumented baseline before believing a mutant
# coverage = false               # measure reach once, then run a mutant only where it was reached

# A mutant a reviewer declared equivalent. The run confirms the claim and
# reports a stale expectation rather than hiding the mutant.
# [[mutation.expect]]
# id = \"\"                        # identity, or a prefix that names exactly one
# reason = \"\"                    # required
# outcome = \"survived\"           # survived | killed | timed_out

[execution]
# offline = false
# locked = false
# doctests = true                # run a library's documented examples as a target
# skip_targets = []              # target ids never to start, as pkg/kind/name
# test_binary_args = []          # allowed: {allowed}

[reports]
# directory = \"{directory}\"   # workspace-relative
# keep = {keep}                       # run directories kept
",
        tier = Tier::Balanced.name(),
        tiers = Tier::ALL.map(Tier::name).join(" | "),
        timeout = AUTO,
        allowed = ALLOWED_TEST_ARGS.join(", "),
        directory = DEFAULT_REPORTS_DIRECTORY,
        keep = DEFAULT_REPORTS_KEEP,
    )
}
