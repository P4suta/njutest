// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.mjutest.toml`: optional, strict, and defaulted.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::error::{self, ErrorCode};

/// The file a run reads, in the workspace root.
pub const FILE_NAME: &str = ".mjutest.toml";

/// The upper bound on one executed command when the file does not say.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// How much of the outcome cache is kept when the file does not say.
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// How long a cached outcome is kept when the file does not say.
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_hours(720);

/// How many run directories are kept when the file does not say.
pub const DEFAULT_REPORTS_KEEP: u32 = 20;

/// The harness flags a run may pass through after `--`.
pub const ALLOWED_TEST_ARGS: [&str; 4] = [
    "--test-threads",
    "--include-ignored",
    "--nocapture",
    "--show-output",
];

/// The environment variables libtest reads for itself, which a run owns.
pub const RESERVED_ENV_PREFIX: &str = "RUST_TEST_";

/// Which assurance contract a run answers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Contract {
    /// The soundness phase is a static inventory, and a non-empty one is a limitation rather than a failure.
    #[default]
    #[serde(rename = "standard-v1")]
    StandardV1,
    /// The soundness phase runs Miri, and undefined behaviour is a defect.
    #[serde(rename = "deep-v1")]
    DeepV1,
}

/// Everything `.mjutest.toml` can say.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// The schema version. Only `1` is understood.
    pub version: u32,
    /// Which contract a run answers to.
    pub contract: Contract,
    /// What is under verification.
    pub project: Project,
    /// How tests are built and run.
    pub execution: Execution,
    /// What is kept between runs.
    pub cache: Cache,
    /// How mutants are proved about before they are executed.
    pub mutation: Mutation,
    /// What is kept under `reports/`.
    pub reports: Reports,
    /// The `deep-v1` soundness phase.
    pub soundness: Soundness,
    /// The fuzz targets a run may drive.
    pub fuzz: Fuzz,
    /// The integration resources a run may start, by name.
    pub resources: BTreeMap<String, Resource>,
    /// The provider that writes candidate tests.
    pub generation: Option<Generation>,
    /// The surviving mutants a reviewer accepted, with reasons.
    pub acceptance: Vec<Acceptance>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            contract: Contract::default(),
            project: Project::default(),
            execution: Execution::default(),
            cache: Cache::default(),
            mutation: Mutation::default(),
            reports: Reports::default(),
            soundness: Soundness::default(),
            fuzz: Fuzz::default(),
            resources: BTreeMap::new(),
            generation: None,
            acceptance: Vec::new(),
        }
    }
}

/// What is under verification.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Project {
    /// The cargo packages to verify. Empty is every workspace member.
    pub packages: Vec<String>,
    /// Workspace-relative globs whose files are left out of the mutations, which the report carries as an explicit limitation.
    pub exclude: Vec<String>,
}

impl Project {
    /// The exclusions, compiled.
    ///
    /// They say which files are mutated and nothing else: the tree is copied
    /// whole, every package still builds, and every test still runs, so a file
    /// left out of the mutations is still one the run is a function of. A
    /// pattern that does not compile cannot reach here, because
    /// [`Config::parse`] refuses it; one that somehow does is left out, which
    /// mutates more rather than less.
    #[must_use]
    pub fn excluded(&self) -> Vec<rust_mutants::glob::Pattern> {
        self.exclude
            .iter()
            .filter_map(|pattern| rust_mutants::glob::Pattern::compile(pattern).ok())
            .collect()
    }
}

/// How tests are built and run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Execution {
    /// Cargo features to enable.
    pub features: Vec<String>,
    /// Pass `--all-features`.
    pub all_features: bool,
    /// Pass `--no-default-features`.
    pub no_default_features: bool,
    /// Harness flags to pass through; see [`ALLOWED_TEST_ARGS`].
    pub test_binary_args: Vec<String>,
    /// Environment variable *names* a test process may see.
    pub environment: Vec<String>,
    /// The upper bound on one executed command.
    #[serde(deserialize_with = "duration", serialize_with = "as_millis")]
    pub timeout: Duration,
    /// How many mutation workers. Zero means the logical CPUs, capped.
    pub jobs: u32,
    /// Test targets never to start, by the stable id a report names them with.
    pub skip_targets: Vec<String>,
}

impl Execution {
    /// What a build of this project is, beyond the tree itself.
    ///
    /// Cargo compiles a different program for a different feature set, so a
    /// run that measures the default build while the project ships another
    /// measures a program nobody runs: the features a configuration names are
    /// the features every command of the run compiles with.
    #[must_use]
    pub fn build(&self) -> rust_mutants::cargo::BuildConfig {
        rust_mutants::cargo::BuildConfig {
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            ..rust_mutants::cargo::BuildConfig::default()
        }
    }
}

impl Default for Execution {
    fn default() -> Self {
        Self {
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            test_binary_args: Vec::new(),
            environment: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            jobs: 0,
            skip_targets: Vec::new(),
        }
    }
}

/// How mutants are proved about before they are executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Mutation {
    /// Ask the compiler whether it renders each surviving mutation identically to the code it mutates. It costs two builds of a tree of its own for every survivor whose premises hold, and it removes a finding only where no test could have noticed the mutation.
    pub equivalence: bool,
}

/// What is kept between runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Cache {
    /// How much of the outcome cache is kept.
    pub max_bytes: u64,
    /// How long a cached outcome is kept.
    #[serde(deserialize_with = "duration", serialize_with = "as_millis")]
    pub ttl: Duration,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_CACHE_MAX_BYTES,
            ttl: DEFAULT_CACHE_TTL,
        }
    }
}

/// What is kept under `reports/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Reports {
    /// How many run directories are kept.
    pub keep: u32,
}

impl Default for Reports {
    fn default() -> Self {
        Self {
            keep: DEFAULT_REPORTS_KEEP,
        }
    }
}

/// The `deep-v1` soundness phase.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Soundness {
    /// Flags for Miri.
    pub miri_flags: Vec<String>,
    /// Sanitizers to run under, on a toolchain that has them.
    pub sanitizers: Vec<String>,
}

/// The fuzz targets a run may drive.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Fuzz {
    /// Drive the targets, rather than only saying they are there.
    pub run: bool,
    /// How long one target is driven for.
    #[serde(deserialize_with = "duration", serialize_with = "as_millis")]
    pub max_total_time: Duration,
    /// The targets to drive, by name. Empty is every target the tree holds.
    pub targets: Vec<String>,
}

impl Default for Fuzz {
    fn default() -> Self {
        Self {
            run: false,
            max_total_time: Duration::from_secs(60),
            targets: Vec::new(),
        }
    }
}

/// One integration resource a run may start.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// The provider to run.
    pub command: Vec<String>,
    /// How long it may take to become ready.
    #[serde(
        default = "default_resource_timeout",
        deserialize_with = "duration",
        serialize_with = "as_millis"
    )]
    pub timeout: Duration,
    /// Several tests may use it at once.
    #[serde(default)]
    pub shared: bool,
    /// One test at a time, which forces a single worker.
    #[serde(default)]
    pub exclusive: bool,
    /// Environment variable names the provider may see.
    #[serde(default)]
    pub environment: Vec<String>,
}

const fn default_resource_timeout() -> Duration {
    Duration::from_secs(30)
}

/// The provider that writes candidate tests.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Generation {
    /// The provider to run.
    pub command: Vec<String>,
    /// Where it may write, as workspace-relative globs.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Environment variable names it may see.
    #[serde(default)]
    pub environment: Vec<String>,
}

/// One surviving mutant a reviewer accepted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    /// The mutant, by identity or by a prefix that names exactly one.
    pub id: String,
    /// Why it was accepted. Required: an acceptance without a reason is a suppression, and a report cannot audit one.
    pub reason: String,
    /// When the acceptance lapses, after which it answers for nothing.
    #[serde(default)]
    pub expires: Option<jiff::Timestamp>,
    /// Who accepted it.
    #[serde(default)]
    pub owner: Option<String>,
    /// The ticket the decision lives in.
    #[serde(default)]
    pub ticket: Option<String>,
}

impl Acceptance {
    /// Whether this acceptance still answers for anything at `now`.
    ///
    /// An acceptance is a person saying they looked, and the expiry is when
    /// they said to look again. One that has passed answers for nothing, or
    /// the date is a comment. One that names no date never lapses, which is
    /// what a reviewer who wrote none asked for.
    #[must_use]
    pub fn holds(&self, now: jiff::Timestamp) -> bool {
        self.expires.is_none_or(|when| when > now)
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

impl Config {
    /// Reads `.mjutest.toml` from `root`, or the defaults when there is none.
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
        for pattern in &self.project.exclude {
            rust_mutants::glob::Pattern::compile(pattern)
                .map_err(|error| invalid(format!("exclude names an {error}")))?;
        }
        for argument in &self.execution.test_binary_args {
            if !allowed_test_arg(argument) {
                return Err(invalid(format!(
                    "test_binary_args holds {argument:?}, which mjutest owns: it changes \
                     routing, repetition, selection, the output protocol, or completeness. \
                     Allowed: {}",
                    ALLOWED_TEST_ARGS.join(", ")
                )));
            }
        }
        for name in self
            .execution
            .environment
            .iter()
            .chain(
                self.resources
                    .values()
                    .flat_map(|one| one.environment.iter()),
            )
            .chain(
                self.generation
                    .iter()
                    .flat_map(|one| one.environment.iter()),
            )
        {
            check_environment_name(name).map_err(invalid)?;
        }
        for (name, resource) in &self.resources {
            if resource.command.is_empty() {
                return Err(invalid(format!("resource {name:?} has no command to run")));
            }
            if resource.shared && resource.exclusive {
                return Err(invalid(format!(
                    "resource {name:?} is both shared and exclusive; it is one or the other"
                )));
            }
        }
        if let Some(generation) = &self.generation
            && generation.command.is_empty()
        {
            return Err(invalid("generation has no command to run".to_owned()));
        }
        for acceptance in &self.acceptance {
            if acceptance.reason.trim().is_empty() {
                return Err(invalid(format!(
                    "the acceptance of {:?} has no reason; an acceptance without one is a \
                     suppression, and a report cannot audit it",
                    acceptance.id
                )));
            }
        }
        Ok(())
    }
}

/// Whether a harness argument is one a run may pass through.
fn allowed_test_arg(argument: &str) -> bool {
    let name = argument.split_once('=').map_or(argument, |(name, _)| name);
    ALLOWED_TEST_ARGS.contains(&name)
}

/// Whether an environment entry is a name rather than an assignment, and one a run does not own.
fn check_environment_name(entry: &str) -> Result<(), String> {
    if let Some((name, _)) = entry.split_once('=') {
        return Err(format!(
            "environment holds an assignment to {name:?}; entries are names, never values"
        ));
    }
    if entry.trim().is_empty() {
        return Err("environment holds an empty name".to_owned());
    }
    if entry.starts_with(RESERVED_ENV_PREFIX) {
        return Err(format!(
            "environment holds {entry:?}, which libtest reads for itself and mjutest owns"
        ));
    }
    Ok(())
}

/// A parse error on one line: the whole of what the parser said, with its line breaks folded away.
fn one_line(error: &toml::de::Error) -> String {
    error
        .to_string()
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join("; ")
}

/// A duration: a sequence of decimal numbers, each with a unit, as in `10m` or `2h45m30s`. Both products read the one spelling, which the engine owns.
///
/// # Errors
/// Returns what is wrong with the text.
pub fn parse_duration(text: &str) -> Result<Duration, rust_mutants::duration::DurationError> {
    rust_mutants::duration::parse(text)
}

/// Reads a duration written the way the contract describes.
fn duration<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_duration(&text).map_err(serde::de::Error::custom)
}

/// The annotated skeleton `mjutest init` writes.
#[must_use]
pub fn skeleton() -> String {
    let timeout = DEFAULT_TIMEOUT.as_secs() / 60;
    let ttl = DEFAULT_CACHE_TTL.as_secs() / 3600;
    format!(
        "\
# mjutest configuration. Every key below is optional; what is shown is the
# default. Unknown keys, malformed values, and any version other than 1 end
# the run rather than being ignored.
version = 1
contract = \"standard-v1\"        # \"standard-v1\" | \"deep-v1\"

[project]
# packages = []                  # cargo package names; empty = every member
# exclude = []                   # workspace-relative globs; the files are not mutated

[execution]
# features = []
# all_features = false
# no_default_features = false
# test_binary_args = []          # allowed: {allowed}
# environment = []               # variable names only, never values
# timeout = \"{timeout}m\"              # upper bound for one executed command
# jobs = 0                       # mutation workers; 0 = logical CPUs, capped
# skip_targets = []              # target ids never to start; reported as a limitation

[mutation]
# equivalence = false            # ask the compiler about every survivor

[cache]
# max_bytes = {max_bytes}
# ttl = \"{ttl}h\"

[reports]
# keep = {keep}                       # run directories kept under reports/runs

[fuzz]
# run = false                    # drive the fuzz targets, not only find them
# max_total_time = \"60s\"         # per target
# targets = []                   # empty = every target the tree holds

[soundness]                      # deep-v1 only
# miri_flags = []
# sanitizers = []                # e.g. [\"thread\"] on nightly

# [resources.postgres]
# command = [\"./tools/postgres-provider\"]
# timeout = \"30s\"
# shared = true                  # or exclusive = true (forces jobs = 1)
# environment = [\"POSTGRES_IMAGE\"]

# [generation]
# command = [\"./tools/test-generator\"]
# allowed_paths = [\"**/tests/**/*.rs\", \"**/fuzz/corpus/**\"]
# environment = [\"GENERATOR_TOKEN\"]

# [[acceptance]]
# id = \"0123456789abcdef\"
# reason = \"reviewed equivalent boundary\"
# expires = \"2026-12-31T00:00:00Z\"
# owner = \"quality-team\"
# ticket = \"QA-123\"
",
        allowed = ALLOWED_TEST_ARGS.join(", "),
        max_bytes = DEFAULT_CACHE_MAX_BYTES,
        keep = DEFAULT_REPORTS_KEEP,
    )
}

/// A [`Duration`] as whole milliseconds, so the digest of a configuration does not depend on how a person spelled `10m`.
fn as_millis<S: serde::Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u128(value.as_millis())
}

impl Config {
    /// The effective configuration, rendered canonically: every field, in declaration order, with durations as milliseconds so two spellings of the same bound are one configuration.
    ///
    /// # Errors
    /// Nothing a caller can act on; a configuration that cannot be rendered
    /// is an invariant failure and answers with the empty document.
    #[must_use]
    pub fn canonical(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_error| String::from("{}"))
    }

    /// The SHA-256 of [`Config::canonical`], which is what a report records and what a cached result is keyed on.
    #[must_use]
    pub fn digest(&self) -> String {
        hex::encode(Sha256::digest(self.canonical().as_bytes()))
    }
}
