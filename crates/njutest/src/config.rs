// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.njutest.toml`: optional, strict, and defaulted.

use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::error::{self, ErrorCode};

/// The file a run reads, in the workspace root.
pub const FILE_NAME: &str = ".njutest.toml";

/// The upper bound on one executed command when the file does not say.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// How many guard takes a mutation may spend when the configuration does not say.
///
/// The engine's own default, not a second number: two products choosing separately what a mutation is allowed is two answers to one question, and a reader who moved between them would find the same code judged differently (ADR 0023).
const fn default_steps() -> u64 {
    rust_mutants::session::DEFAULT_MUTANT_STEPS
}

/// How much of the outcome cache is kept when the file does not say.
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// How long a cached outcome is kept when the file does not say.
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_hours(720);

/// Where a run writes, unless the configuration says otherwise.
pub const DEFAULT_REPORTS_DIRECTORY: &str = "reports";

/// How many run directories are kept when the file does not say.
pub const DEFAULT_REPORTS_KEEP: u32 = 20;

/// One canonical workspace-relative report directory.
///
/// The private UTF-8 spelling is platform independent: forward slashes are the only separators and every component is a non-empty ordinary name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDirectory(String);

/// Why a configured report directory is not one canonical workspace-relative path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReportDirectoryError {
    /// The spelling is empty.
    #[error("the report directory must not be empty")]
    Empty,
    /// The spelling is absolute or starts with a platform volume prefix.
    #[error("the report directory must be workspace-relative and volume-free")]
    Rooted,
    /// The spelling contains an empty, current, or parent component.
    #[error("the report directory must contain only non-empty normal components")]
    Component,
    /// A platform-dependent separator or an embedded NUL was used.
    #[error("the report directory must use portable UTF-8 path syntax")]
    NonPortable,
}

impl ReportDirectory {
    /// The canonical path spelling as a host path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    /// The canonical portable spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ReportDirectory {
    type Error = ReportDirectoryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(ReportDirectoryError::Empty);
        }
        let bytes = value.as_bytes();
        if value.starts_with('/')
            || matches!(bytes, [letter, b':', ..] if letter.is_ascii_alphabetic())
        {
            return Err(ReportDirectoryError::Rooted);
        }
        if value.contains('\\') || value.contains('\0') || value.contains(':') {
            return Err(ReportDirectoryError::NonPortable);
        }
        if value
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        {
            return Err(ReportDirectoryError::Component);
        }
        Ok(Self(value))
    }
}

impl TryFrom<&str> for ReportDirectory {
    type Error = ReportDirectoryError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl Serialize for ReportDirectory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReportDirectory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Contract {
    /// The soundness phase is a static inventory, and a non-empty one is a limitation rather than a failure.
    #[serde(rename = "standard-v1")]
    StandardV1,
    /// The soundness phase runs Miri, and undefined behaviour is a defect.
    #[serde(rename = "deep-v1")]
    DeepV1,
    /// The soundness phase additionally proves eligible mutations with the pinned model checker.
    #[serde(rename = "verified-v1")]
    VerifiedV1,
    /// Every dimension is asked, soundness runs as `deep-v1`, and a dimension not established is not assured (ADR 0033).
    #[serde(rename = "whole-v1")]
    WholeV1,
}

impl Default for Contract {
    fn default() -> Self {
        Self::PROTOCOL_DEFAULT
    }
}

impl Contract {
    const PROTOCOL_DEFAULT: Self = Self::StandardV1;

    /// Whether the soundness phase runs Miri, where an inventory alone would be a limitation.
    #[must_use]
    pub const fn runs_miri(self) -> bool {
        match self {
            Self::DeepV1 | Self::WholeV1 => true,
            Self::StandardV1 | Self::VerifiedV1 => false,
        }
    }

    /// Whether a run proves eligible survivors with the pinned model checker.
    #[must_use]
    pub const fn proves_models(self) -> bool {
        match self {
            Self::VerifiedV1 => true,
            Self::StandardV1 | Self::DeepV1 | Self::WholeV1 => false,
        }
    }

    /// Whether a run asks every dimension it can measure, and is not assured along any it did not (ADR 0033).
    #[must_use]
    pub const fn asks_every_dimension(self) -> bool {
        match self {
            Self::WholeV1 => true,
            Self::StandardV1 | Self::DeepV1 | Self::VerifiedV1 => false,
        }
    }
}

/// Everything `.njutest.toml` can say.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// The schema version.
    /// Only `1` is understood.
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
    /// The bounds of the `verified-v1` model-checking phase.
    pub verification: Verification,
    /// What is kept under `reports/`.
    pub reports: Reports,
    /// The `deep-v1` soundness phase.
    pub soundness: Soundness,
    /// The fuzz targets a run may drive.
    pub fuzz: Fuzz,
    /// Whether a run fails the calls a `?` asks about.
    pub faults: Faults,
    /// What a run sets differently for one more control of each target, to ask whether the target's verdict and reach hold where it differs.
    pub repeatable: Repeatable,
    /// The integration resources a run may start, by name.
    pub resources: BTreeMap<String, Resource>,
    /// The provider that writes candidate tests.
    pub generation: Option<Generation>,
    /// The surviving mutants a reviewer accepted, with reasons.
    pub acceptance: Vec<Acceptance>,
    /// The builds to measure beyond the one `[execution]` describes.
    pub configuration: Vec<Configuration>,
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
            verification: Verification::default(),
            reports: Reports::default(),
            soundness: Soundness::default(),
            fuzz: Fuzz::default(),
            faults: Faults::default(),
            repeatable: Repeatable::default(),
            resources: BTreeMap::new(),
            generation: None,
            acceptance: Vec::new(),
            configuration: Vec::new(),
        }
    }
}

/// What is under verification.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Project {
    /// The cargo packages to verify.
    /// Empty is every workspace member.
    pub packages: Vec<String>,
    /// Workspace-relative globs a file must match for anything in it to be mutated.
    /// Empty is every file.
    pub include: Vec<String>,
    /// Workspace-relative globs whose files are left out of the mutations, which the report carries as an explicit limitation.
    pub exclude: Vec<String>,
}

impl Project {
    /// The inclusions, compiled.
    #[must_use]
    pub fn included(&self) -> Vec<rust_mutants::glob::Pattern> {
        self.include
            .iter()
            .filter_map(
                |pattern| match rust_mutants::glob::Pattern::compile(pattern) {
                    Ok(pattern) => Some(pattern),
                    Err(_) => None,
                },
            )
            .collect()
    }

    /// The exclusions, compiled.
    #[must_use]
    pub fn excluded(&self) -> Vec<rust_mutants::glob::Pattern> {
        self.exclude
            .iter()
            .filter_map(
                |pattern| match rust_mutants::glob::Pattern::compile(pattern) {
                    Ok(pattern) => Some(pattern),
                    Err(_) => None,
                },
            )
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
    /// The upper bound on one measurement, which is one test binary run against one mutation.
    #[serde(deserialize_with = "duration", serialize_with = "as_millis")]
    pub timeout: Duration,
    /// How many times a mutation's guard may be taken before its process is stopped.
    /// `0` counts nothing and leaves `timeout` as the only thing that can end a mutation that does not end.
    ///
    /// A clock measures partly the machine, so two runs of one catalogue on one commit can disagree about a mutation that never returns.
    /// A count is the same number under any load, and a mutation that spends it is `step_limit_reached` rather than `waited`.
    /// It is a deterministic execution fact, not a mutation verdict without a matched control.
    #[serde(default = "default_steps")]
    pub steps: u64,
    /// The upper bound on one build.
    /// `None` is no bound, which is the default: a build is not a measurement, and a project that tightened the one it waits for per mutation did not thereby say how long its own compiler may take.
    #[serde(
        default,
        deserialize_with = "optional_duration",
        serialize_with = "as_optional_millis"
    )]
    pub build_timeout: Option<Duration>,
    /// How many mutation workers.
    /// Zero means the logical CPUs, capped.
    pub jobs: u32,
    /// Test targets never to start, by the stable id a report names them with.
    pub skip_targets: Vec<String>,
    /// Whether to make the coverage build, which is an independent second opinion rather than the measurement (ADR 0014).
    /// `false` is the default, because the guards are the measurement and a coverage build is a whole extra compile and run of every target.
    #[serde(default)]
    pub coverage: bool,
}

impl Execution {
    /// What a build of this project is, beyond the tree itself.
    #[must_use]
    pub fn build(&self) -> rust_mutants::cargo::BuildConfig {
        rust_mutants::cargo::BuildConfig {
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target: None,
            profile: None,
            jobs: None,
            debug: false,
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
            steps: default_steps(),
            build_timeout: None,
            jobs: 0,
            skip_targets: Vec::new(),
            coverage: false,
        }
    }
}

/// How mutants are proved about before they are executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Mutation {
    /// Ask the compiler whether it renders each surviving mutation identically to the code it mutates.
    /// It costs two builds of a tree of its own for every survivor whose premises hold, and it removes a finding only where no test could have noticed the mutation.
    pub equivalence: bool,
}

/// Whether a run fails, one at a time, every call a `?` asks about, and asks the suite what it noticed (ADR 0032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Faults {
    /// Put the faults.
    /// It costs a second instrumented build and baseline, and one execution for every `?` a test reaches.
    pub inject: bool,
}

/// Bounds that exist only for the `verified-v1` contract.
///
/// Both values are optional in the document so serde can distinguish a missing key from a zero one.
/// [`Verification::checked`] is the only route to the invariant-bearing [`Verified`] value consumed by the model runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Verification {
    /// Maximum loop unwind used by every generated proof harness.
    pub unwind: Option<u32>,
    /// Wall-clock ceiling for one model-checker process.
    #[serde(
        default,
        deserialize_with = "optional_duration",
        serialize_with = "as_optional_millis"
    )]
    pub timeout: Option<Duration>,
}

impl Verification {
    /// Turns document-shaped optional values into the values a verifier may actually consume.
    ///
    /// # Errors
    /// Returns the first missing or zero bound.
    /// No caller can accidentally turn either condition into an unbounded proof.
    pub fn checked(self) -> Result<Verified, VerificationError> {
        let unwind = self.unwind.ok_or(VerificationError::MissingUnwind)?;
        let unwind = NonZeroU32::new(unwind).ok_or(VerificationError::ZeroUnwind)?;
        let timeout = self.timeout.ok_or(VerificationError::MissingTimeout)?;
        if timeout.is_zero() {
            return Err(VerificationError::ZeroTimeout);
        }
        let timeout_ms = u64::try_from(timeout.as_millis())
            .map_err(|_error| VerificationError::TimeoutTooLarge)?;
        let timeout_ms =
            NonZeroU64::new(timeout_ms).ok_or(VerificationError::SubmillisecondTimeout)?;
        Ok(Verified { unwind, timeout_ms })
    }

    const fn is_empty(self) -> bool {
        self.unwind.is_none() && self.timeout.is_none()
    }
}

/// Bounds safe to hand to the model checker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    unwind: NonZeroU32,
    timeout_ms: NonZeroU64,
}

impl Verified {
    /// The loop unwind, proven nonzero when the configuration was read.
    #[must_use]
    pub const fn unwind(self) -> NonZeroU32 {
        self.unwind
    }

    /// The process ceiling, proven nonzero when the configuration was read.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn timeout(self) -> Duration {
        Duration::from_millis(self.timeout_ms.get())
    }

    /// Milliseconds in the exact nonzero representation retained in evidence.
    #[must_use]
    pub const fn timeout_ms(self) -> NonZeroU64 {
        self.timeout_ms
    }
}

/// Why verifier bounds cannot become an executable setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VerificationError {
    /// `verified-v1` did not name an unwind.
    #[error("verified-v1 requires verification.unwind")]
    MissingUnwind,
    /// An unwind of zero proves no path and is not a verifier bound.
    #[error("verification.unwind must be greater than zero")]
    ZeroUnwind,
    /// `verified-v1` did not name a process timeout.
    #[error("verified-v1 requires verification.timeout")]
    MissingTimeout,
    /// A zero timeout starts no proof and is not a verifier bound.
    #[error("verification.timeout must be greater than zero")]
    ZeroTimeout,
    /// A positive sub-millisecond value cannot be represented in report evidence.
    #[error("verification.timeout must be at least one millisecond")]
    SubmillisecondTimeout,
    /// The duration does not fit the report's millisecond representation.
    #[error("verification.timeout is too large to represent in milliseconds")]
    TimeoutTooLarge,
    /// Verifier bounds were attached to a contract that never consumes them.
    #[error("[verification] is only valid with contract = \"verified-v1\"")]
    WrongContract,
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Reports {
    /// How many run directories are kept.
    pub keep: u32,
    /// The workspace-relative directory every run writes under.
    ///
    /// A project that already means something by `reports/` says so here rather than living with it.
    /// Runs go in `<directory>/runs`, one directory each, and the indexes that name the newest sit beside them.
    pub directory: ReportDirectory,
}

impl Default for Reports {
    fn default() -> Self {
        Self {
            keep: DEFAULT_REPORTS_KEEP,
            directory: ReportDirectory(DEFAULT_REPORTS_DIRECTORY.to_owned()),
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

/// What a run sets differently for one more control of each target: nothing unless asked, since each knob is one more run of every target.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Repeatable {
    /// The knobs to put, by name.
    pub knobs: Vec<crate::report::knobs::Knob>,
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
    /// The targets to drive, by name.
    /// Empty is every target the tree holds.
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
    /// The variable of the provider's answer that names where the tests dial, which is the seam an interposer sits in front of.
    /// Empty watches nothing.
    #[serde(default)]
    pub interpose: String,
    /// How much of what goes past that seam is read.
    #[serde(default)]
    pub wire: crate::wire::Wire,
    /// How long `delay-response` holds an answer up for, which is the question being asked of this dependency.
    ///
    /// How slow is too slow is a property of the system under test and not of this tool: a service with a one-second budget and a nightly batch job are asking different questions of the same seam, and a run that picked for them would be reporting an answer to a question nobody put.
    #[serde(
        default = "default_resource_hold",
        deserialize_with = "duration",
        serialize_with = "as_millis"
    )]
    pub hold: Duration,
}

const fn default_resource_timeout() -> Duration {
    Duration::from_secs(30)
}

const fn default_resource_hold() -> Duration {
    crate::wire::interpose::HELD_UP
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

/// What the report calls the build `[execution]` describes.
pub const DEFAULT_CONFIGURATION: &str = "default";

/// One further build of the project to measure, beyond the one `[execution]` describes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Configuration {
    /// What a report calls it, which must be unique and not [`DEFAULT_CONFIGURATION`].
    pub name: String,
    /// Cargo features to enable instead of `[execution] features`.
    pub features: Vec<String>,
    /// Pass `--all-features`.
    pub all_features: bool,
    /// Pass `--no-default-features`.
    pub no_default_features: bool,
    /// The cargo profile to compile with.
    /// `None` is the command's own default.
    pub profile: Option<String>,
    /// The target triple to compile for.
    /// `None` is the host.
    pub target: Option<String>,
}

impl Configuration {
    /// What a build of the project under this configuration is.
    #[must_use]
    pub fn build(&self) -> rust_mutants::cargo::BuildConfig {
        rust_mutants::cargo::BuildConfig {
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            profile: self.profile.clone(),
            target: self.target.clone(),
            jobs: None,
            debug: false,
        }
    }
}

/// Whether every configuration carries a name a report can tell from the others'.
fn named_once(configurations: &[Configuration], path: &Path) -> Result<(), ConfigError> {
    let invalid = |message: String| ConfigError::new(ConfigErrorKind::Invalid, path, message);
    let mut named: Vec<&str> = Vec::new();
    for configuration in configurations {
        let name = configuration.name.trim();
        if name.is_empty() {
            return Err(invalid(
                "a configuration is named, because a report says which build each \
                 answer came from"
                    .to_owned(),
            ));
        }
        if name == DEFAULT_CONFIGURATION {
            return Err(invalid(format!(
                "{DEFAULT_CONFIGURATION:?} is what a report calls the build [execution] \
                 describes, so a configuration cannot take it"
            )));
        }
        if named.contains(&name) {
            return Err(invalid(format!(
                "two configurations are named {name:?}, and a report could not tell \
                 their answers apart"
            )));
        }
        named.push(name);
    }
    Ok(())
}

/// One surviving mutant a reviewer accepted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    /// The mutant, by identity or by a prefix that names exactly one.
    ///
    /// An identity is a function of the whole file, so it is re-minted by any edit to that file — including the edit somebody makes next.
    /// An acceptance is a durable record, so it is worth writing the locator instead: `path`, `item`, `rule` and `original` name the same mutation after the file has changed around it.
    #[serde(default)]
    pub id: String,
    /// The workspace-relative path the mutation is in, for a locator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The item the mutation is in, by a suffix of its path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// The rule that produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// The bytes the edit replaces, as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    /// The line, as a hint that separates two mutations the rest would name together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Why it was accepted.
    /// Required: an acceptance without a reason is a suppression, and a report cannot audit one.
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
    /// The locator this acceptance writes, when it writes one rather than an identity.
    #[must_use]
    pub fn locator(&self) -> Option<rust_mutants::session::Locator> {
        Some(rust_mutants::session::Locator {
            path: self.path.clone()?,
            item: self.item.clone()?,
            rule: self.rule.clone()?,
            original: self.original.clone().unwrap_or_default(),
            line: self.line,
            count: None,
        })
    }

    /// What a reader wrote to name the mutation, for a message about it.
    #[must_use]
    pub fn named(&self) -> String {
        self.locator().map_or_else(
            || self.id.clone(),
            |one| format!("{}:{}:{}", one.path, one.item, one.rule),
        )
    }

    /// Whether this acceptance still answers for anything at `now`.
    #[must_use]
    pub fn holds(&self, now: jiff::Timestamp) -> bool {
        self.expires.is_none_or(|when| when > now)
    }
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
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
    #[source]
    source: Option<serde_json::Error>,
}

impl ConfigError {
    fn new(kind: ConfigErrorKind, path: &Path, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.display().to_string(),
            message: message.into(),
            source: None,
        }
    }

    fn unserializable(source: serde_json::Error) -> Self {
        Self {
            kind: ConfigErrorKind::Invalid,
            path: "effective configuration".to_owned(),
            message: "cannot render the validated configuration canonically".to_owned(),
            source: Some(source),
        }
    }

    /// The failure mode.
    #[must_use]
    #[cfg(feature = "testkit")]
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
    /// The invariant-bearing verifier settings for this contract.
    ///
    /// # Errors
    /// Returns a typed error when a `verified-v1` bound is absent or zero, or when another contract carries verifier-only keys.
    /// This rechecks public fields so a caller that constructs or mutates [`Config`] cannot bypass the same boundary enforced by [`Config::parse`].
    pub fn verified(&self) -> Result<Option<Verified>, VerificationError> {
        match self.contract {
            Contract::VerifiedV1 => self.verification.checked().map(Some),
            Contract::StandardV1 | Contract::DeepV1 | Contract::WholeV1
                if self.verification.is_empty() =>
            {
                Ok(None)
            }
            Contract::StandardV1 | Contract::DeepV1 | Contract::WholeV1 => {
                Err(VerificationError::WrongContract)
            }
        }
    }

    /// Reads `.njutest.toml` from `root`, or the defaults when there is none.
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
        let mut config: Self = toml::from_str(text).map_err(|error| {
            ConfigError::new(ConfigErrorKind::Unparsable, path, one_line(&error))
        })?;
        config.validate(path)?;
        config.asked_everything(text, path)?;
        Ok(config)
    }

    /// Puts every fault and every knob where the contract asks every dimension, and refuses a document that says, in so many words, not to (ADR 0033).
    fn asked_everything(&mut self, text: &str, path: &Path) -> Result<(), ConfigError> {
        if !self.contract.asks_every_dimension() {
            return Ok(());
        }
        let written: toml::Table = toml::from_str(text).map_err(|error: toml::de::Error| {
            ConfigError::new(ConfigErrorKind::Unparsable, path, one_line(&error))
        })?;
        let said = |section: &str, key: &str| {
            written
                .get(section)
                .and_then(toml::Value::as_table)
                .and_then(|table| table.get(key))
                .is_some()
        };
        let every: Vec<crate::report::knobs::Knob> = crate::report::knobs::Knob::ALL.to_vec();
        let refused = if said("faults", "inject") && !self.faults.inject {
            Some("[faults] inject = false")
        } else if said("repeatable", "knobs") && self.repeatable.knobs != every {
            Some("[repeatable] knobs naming fewer than every knob")
        } else {
            None
        };
        if let Some(said) = refused {
            return Err(ConfigError::new(
                ConfigErrorKind::Invalid,
                path,
                format!(
                    "contract = \"whole-v1\" asks every dimension, and {said} asks the run not \
                     to measure one of them; drop the key, or name another contract"
                ),
            ));
        }
        self.faults.inject = true;
        self.repeatable.knobs = every;
        Ok(())
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
        self.verified()
            .map_err(|error| invalid(error.to_string()))?;
        named_once(&self.configuration, path)?;
        for pattern in &self.project.include {
            rust_mutants::glob::Pattern::compile(pattern)
                .map_err(|error| invalid(format!("include names an {error}")))?;
        }
        for pattern in &self.project.exclude {
            rust_mutants::glob::Pattern::compile(pattern)
                .map_err(|error| invalid(format!("exclude names an {error}")))?;
        }
        for argument in &self.execution.test_binary_args {
            if !allowed_test_arg(argument) {
                return Err(invalid(format!(
                    "test_binary_args holds {argument:?}, which njutest owns: it changes \
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
            check_environment_name(name).map_err(|error| invalid(error.to_string()))?;
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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
enum EnvironmentNameError {
    #[error("environment holds an assignment to {name:?}; entries are names, never values")]
    Assignment { name: String },
    #[error("environment holds an empty name")]
    Empty,
    #[error("environment holds {name:?}, which libtest reads for itself and njutest owns")]
    Reserved { name: String },
}

fn check_environment_name(entry: &str) -> Result<(), EnvironmentNameError> {
    if let Some((name, _)) = entry.split_once('=') {
        return Err(EnvironmentNameError::Assignment {
            name: name.to_owned(),
        });
    }
    if entry.trim().is_empty() {
        return Err(EnvironmentNameError::Empty);
    }
    if entry.starts_with(RESERVED_ENV_PREFIX) {
        return Err(EnvironmentNameError::Reserved {
            name: entry.to_owned(),
        });
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

/// A duration: a sequence of decimal numbers, each with a unit, as in `10m` or `2h45m30s`.
/// Both products read the one spelling, which the engine owns.
///
/// # Errors
/// Returns what is wrong with the text.
pub fn parse_duration(text: &str) -> Result<Duration, rust_mutants::duration::DurationError> {
    rust_mutants::duration::parse(text)
}

/// Reads a duration written the way the contract describes.
fn duration<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_duration(&text).map_err(serde::de::Error::custom)
}

/// The annotated skeleton `njutest init` writes.
#[must_use]
pub fn skeleton() -> String {
    let timeout = DEFAULT_TIMEOUT.as_secs() / 60;
    let ttl = DEFAULT_CACHE_TTL.as_secs() / 3600;
    format!(
        "\
# njutest configuration. Every key below is optional; what is shown is the
# default. Unknown keys, malformed values, and any version other than 1 end
# the run rather than being ignored.
version = 1
contract = \"standard-v1\"        # \"standard-v1\" | \"deep-v1\" | \"verified-v1\"

[project]
# packages = []                  # cargo package names; empty = every member
# include = []                   # workspace-relative globs a file must match to be mutated
# exclude = []                   # workspace-relative globs; the files are not mutated

[execution]
# features = []
# all_features = false
# no_default_features = false
# test_binary_args = []          # allowed: {allowed}
# environment = []               # variable names only, never values
# timeout = \"{timeout}m\"              # upper bound for one measurement
# build_timeout = \"\"            # upper bound for one build; empty = no bound
# jobs = 0                       # mutation workers; 0 = logical CPUs, capped
# skip_targets = []              # target ids never to start; reported as a limitation
# coverage = false                # make the coverage build as a second opinion (ADR 0014)

#[[configuration]]              # a further build to measure; none by default
# name = \"all-features\"         # what the report calls it; not \"default\", and unique
# features = []
# all_features = true
# no_default_features = false
# profile = \"\"                  # cargo profile; empty = the command's own default
# target = \"\"                   # target triple; empty = the host

[mutation]
# equivalence = false            # ask the compiler about every survivor

[verification]                   # verified-v1 only; both keys are mandatory and nonzero
# unwind = 8                     # maximum loop unwind for every proof harness
# timeout = \"2m\"                # wall-clock ceiling for one checker process

[cache]
# max_bytes = {max_bytes}
# ttl = \"{ttl}h\"

[reports]
# keep = {keep}                       # run directories kept
# directory = \"reports\"        # where every run writes: the JSON report, the record stream,
#                             # and the HTML, SARIF and JUnit projections of the same run,
#                             # one directory per run under <directory>/runs

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
# interpose = \"\"                 # the variable of the answer naming where the tests dial
# wire = \"raw\"                   # how much of what goes past that seam is read: raw | http
# hold = \"30s\"                   # how long delay-response holds an answer up: how slow is too slow, here

# [generation]
# command = [\"./tools/test-generator\"]
# allowed_paths = [\"**/tests/**/*.rs\", \"**/fuzz/corpus/**\"]
# environment = [\"GENERATOR_TOKEN\"]

# [[acceptance]]
# path = \"src/lib.rs\"      # a locator survives an edit to the file; an identity does not,
# item = \"clamp\"           # because it is a function of the file's bytes and the edit
# rule = \"le-to-lt\"        # that fixes a survivor is one to the same file
# original = \"<=\"
# line = 42                 # a hint, when the rest names more than one
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
fn as_millis<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u128(value.as_millis())
}

/// A bound a project may leave unsaid, where an empty string says it out loud.
fn optional_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Duration>, D::Error> {
    let text = Option::<String>::deserialize(deserializer)?;
    match text.as_deref() {
        None | Some("") => Ok(None),
        Some(said) => parse_duration(said)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[expect(
    clippy::ref_option,
    reason = "serde's serialize_with hands the field by reference, so the signature is its \
              contract rather than a choice"
)]
fn as_optional_millis<S: Serializer>(
    value: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(bound) => serializer.serialize_u128(bound.as_millis()),
        None => serializer.serialize_none(),
    }
}

impl Config {
    /// The effective configuration, rendered canonically: every field, in declaration order, with durations as milliseconds so two spellings of the same bound are one configuration.
    ///
    /// # Errors
    /// [`ConfigErrorKind::Invalid`] when the validated in-memory value cannot be serialized.
    /// No placeholder document is substituted into identity.
    pub fn canonical(&self) -> Result<String, ConfigError> {
        serde_json::to_string(self).map_err(ConfigError::unserializable)
    }

    /// The SHA-256 of [`Config::canonical`], which is what a report records and what a cached result is keyed on.
    ///
    /// # Errors
    /// The same typed serialization failure as [`Config::canonical`].
    pub fn digest(&self) -> Result<String, ConfigError> {
        self.canonical()
            .map(|canonical| hex::encode(Sha256::digest(canonical.as_bytes())))
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, ReportDirectory, ReportDirectoryError};
    use std::path::Path;

    #[test]
    fn report_directory_accepts_only_canonical_workspace_relative_spellings() {
        assert!(matches!(
            ReportDirectory::try_from(""),
            Err(ReportDirectoryError::Empty)
        ));
        for rooted in ["/reports", "C:", "C:reports"] {
            assert!(matches!(
                ReportDirectory::try_from(rooted),
                Err(ReportDirectoryError::Rooted)
            ));
        }
        for component in [".", "..", "reports/.", "reports/..", "reports//runs"] {
            assert!(matches!(
                ReportDirectory::try_from(component),
                Err(ReportDirectoryError::Component)
            ));
        }
        for non_portable in ["reports\\runs", "reports:stream", "reports\0runs"] {
            assert!(matches!(
                ReportDirectory::try_from(non_portable),
                Err(ReportDirectoryError::NonPortable)
            ));
        }
        assert_eq!(
            ReportDirectory::try_from("artifacts/njutest")
                .map(|directory| directory.as_str().to_owned()),
            Ok("artifacts/njutest".to_owned())
        );
    }

    #[test]
    fn report_directory_deserialization_enforces_the_same_boundary() {
        for invalid in ["../outside", "reports//runs", "reports:stream"] {
            let text = format!("[reports]\ndirectory = {invalid:?}\n");
            assert!(
                Config::parse(&text, Path::new(".njutest.toml")).is_err(),
                "{invalid:?} must not enter a parsed configuration"
            );
        }
        let parsed = Config::parse(
            "[reports]\ndirectory = \"artifacts/njutest\"\n",
            Path::new(".njutest.toml"),
        );
        assert!(matches!(
            parsed,
            Ok(config) if config.reports.directory.as_str() == "artifacts/njutest"
        ));
    }
}
