// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.rust-mutants.toml`: optional, strict, and defaulted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_mutants::discover::SkipRule;
use rust_mutants::error::{self, ErrorCode};
use rust_mutants::glob::Pattern;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::run::Expectation;
use rust_mutants::session::{Locator, Timeout};
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
    /// What the copy a run works in leaves behind.
    pub snapshot: Snapshot,
}

/// What the copy a run works in leaves behind.
///
/// A run measures a copy of the tree, and a tree can hold things a copy has no
/// use for: a directory of test data measured in gigabytes, a file nobody
/// outside the machine should hold. This is where a project says what not to
/// copy. It is not where a project says what not to mutate — that is
/// `[project] include` and `[project] exclude`, which leave the file in the
/// tree and take it out of the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Snapshot {
    /// Workspace-relative globs naming what the copy does not carry.
    ///
    /// The file is not in the tree a run builds, so naming one the crate
    /// declares as a module leaves a tree that does not compile: the run
    /// refuses before it instruments anything, and the compiler's complaint is
    /// about a file that is missing because this said not to copy it.
    pub omit: Vec<String>,
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
            snapshot: Snapshot::default(),
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
    /// Workspace-relative globs naming files nothing is mutated in, which is what [`Project::include`] is the other half of.
    ///
    /// The file stays in the tree and is compiled like any other; what it
    /// loses is its mutants. A file the copy should not carry at all is
    /// `[snapshot] omit`, which is a different thing and says so.
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
    /// Write debug information into what a run builds.
    ///
    /// Off, because a run reads what a test harness printed and never a
    /// backtrace, and the debug information is most of what a build writes:
    /// six gigabytes against one for this repository's own engine, every byte
    /// of it generated, linked, and thrown away with the temporary directory.
    /// Turn it on to attach a debugger to a snapshot `--keep-temp` preserved.
    pub debug: bool,
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
            debug: self.debug,
        }
    }
}

/// How mutants are proposed and executed.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one switch a person writes in a file and one flag on the command line, and \
              a switch is a bool wherever it is stored"
)]
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
    /// Build once with LLVM coverage instrumentation and route by the regions it exported.
    ///
    /// The guards already say which of a target's tests reached each mutation,
    /// and they say it on a run the engine was making anyway, so this is off:
    /// instrumenting for coverage rebuilds every crate in the graph, which on
    /// a real workspace is the largest single thing a run could do. It is kept
    /// as an independent second opinion, and for the branch proofs of the
    /// bodies no marker could be written into.
    pub coverage: bool,
    /// Ask the guards, on the run that verifies the baseline, which of each target's tests reached them, and put a mutation only to those tests.
    ///
    /// It costs the run nothing it was not already spending. Turning it off is
    /// how a caller asks for the answer a run with nothing removed would give.
    pub touch: bool,
    /// After the run, ask the compiler whether each survivor's mutation is one it renders at all.
    ///
    /// It costs a tree of its own and one build per survivor, and it never
    /// says a mutation is equivalent: what it can say is that the compiler
    /// renders the two identically, which is a fact about the binaries.
    pub equivalence: bool,
    /// The mutants a reviewer declared equivalent, with the outcome the run must confirm.
    pub expect: Vec<Expect>,
    /// The places a reviewer decided are not worth measuring, each with the reason.
    pub skip: Vec<Skip>,
}

/// One `[[mutation.expect]]` entry, as a person writes it.
///
/// The mutant is named either by identity, which is exact and changes when
/// anything in the file does, or by a locator — path, item, rule, and the
/// bytes the edit replaces — which survives an edit elsewhere in the file.
/// Never both.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// The identity, or a prefix that names exactly one mutant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The workspace-relative path the mutation is in.
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
    /// How many mutations the locator names, when one reason is written for a set of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Why the outcome is what it is. Required.
    pub reason: String,
    /// The outcome the run must confirm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

impl Expect {
    /// How the claim is written back to a reader.
    #[must_use]
    pub fn name(&self) -> String {
        self.id.clone().unwrap_or_else(|| {
            format!(
                "{} {} {} {:?}",
                self.path.as_deref().unwrap_or_default(),
                self.item.as_deref().unwrap_or_default(),
                self.rule.as_deref().unwrap_or_default(),
                self.original.as_deref().unwrap_or_default()
            )
        })
    }

    /// Whether the entry names a mutant by where it is rather than by identity.
    #[must_use]
    pub const fn is_locator(&self) -> bool {
        self.path.is_some()
            || self.item.is_some()
            || self.rule.is_some()
            || self.original.is_some()
            || self.line.is_some()
            || self.count.is_some()
    }

    /// The outcome claimed, which is `survived` when the entry does not say.
    #[must_use]
    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
            .as_deref()
            .map_or(Some(Outcome::Survived), Outcome::parse)
    }

    /// The claim as the engine reads it.
    #[must_use]
    pub fn expectation(&self) -> Expectation {
        Expectation {
            id: self.id.clone(),
            locator: self.is_locator().then(|| Locator {
                path: self.path.clone().unwrap_or_default(),
                item: self.item.clone().unwrap_or_default(),
                rule: self.rule.clone().unwrap_or_default(),
                original: self.original.clone().unwrap_or_default(),
                line: self.line,
                count: self.count,
            }),
            reason: self.reason.clone(),
            outcome: self.outcome().unwrap_or(Outcome::Survived),
        }
    }
}

/// One `[[mutation.skip]]` entry, as a person writes it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Skip {
    /// The paths it speaks about, as a glob against the workspace-relative path.
    pub path: String,
    /// The lines it speaks about, as `from-to`, inclusive and 1-based. Only with a literal path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<String>,
    /// The item it speaks about, by a suffix of the item path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// Why its author wrote it. Required.
    pub reason: String,
}

impl Skip {
    /// The range the entry names, when it names one.
    #[must_use]
    pub fn range(&self) -> Option<(u32, u32)> {
        let text = self.lines.as_deref()?;
        let (from, to) = text.split_once('-')?;
        Some((from.trim().parse().ok()?, to.trim().parse().ok()?))
    }

    /// The entry as the engine reads it.
    ///
    /// # Errors
    /// Returns the pattern's own failure when the path is not a glob.
    pub fn rule(&self) -> Result<SkipRule, rust_mutants::glob::GlobError> {
        Ok(SkipRule {
            path: Pattern::compile(&self.path)?,
            lines: self.range(),
            item: self.item.clone(),
            reason: self.reason.clone(),
        })
    }
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
            touch: true,
            equivalence: false,
            expect: Vec::new(),
            skip: Vec::new(),
        }
    }
}

/// How the workspace is built and the tests are run.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one switch a person writes in a file and one flag on the command line, and \
              a switch is a bool wherever it is stored"
)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Execution {
    /// Never touch the network.
    pub offline: bool,
    /// Refuse to change `Cargo.lock`.
    pub locked: bool,
    /// Harness flags to pass through; see [`ALLOWED_TEST_ARGS`].
    pub test_binary_args: Vec<String>,
    /// Start every test process in a directory of its own rather than where cargo would.
    ///
    /// A test that writes into the directory it runs in writes into the tree
    /// being measured, and the run says `tree-written-during-measurement`
    /// about every mutation after it. Turning this on puts those writes
    /// outside the tree without changing a line of the suite, because what a
    /// test resolves against "here" moves with it — including a temporary
    /// directory it makes in the current directory on purpose.
    ///
    /// It is off by default because a test that reads a fixture by a path
    /// relative to where cargo starts it stops finding it. Which of the two a
    /// suite does is a thing its author knows and a run cannot.
    pub scratch_working_directory: bool,
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
    /// How many mutants to measure at once. Zero is as many as the machine has, capped at four.
    pub jobs: usize,
}

impl Default for Execution {
    fn default() -> Self {
        Self {
            offline: false,
            locked: false,
            doctests: true,
            test_binary_args: Vec::new(),
            scratch_working_directory: false,
            skip_targets: Vec::new(),
            jobs: 0,
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
    /// What the Stryker projection declares, which only its readers use.
    #[serde(default)]
    pub stryker: Stryker,
}

impl Default for Reports {
    fn default() -> Self {
        Self {
            directory: PathBuf::from(DEFAULT_REPORTS_DIRECTORY),
            keep: DEFAULT_REPORTS_KEEP,
            stryker: Stryker::default(),
        }
    }
}

/// The thresholds a Stryker reader colours by. Nothing in this engine decides anything by them: a verdict is a claim a reader can check, and a percentage is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Stryker {
    /// At or above this, a reader shows green.
    pub high: u32,
    /// Below this, a reader shows red.
    pub low: u32,
}

impl Default for Stryker {
    fn default() -> Self {
        Self { high: 80, low: 60 }
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

/// What a count on one expectation has to be for the claim to say something.
fn check_count(
    expectation: &Expect,
    name: &str,
    invalid: &impl Fn(String) -> ConfigError,
) -> Result<(), ConfigError> {
    if expectation.count == Some(0) {
        return Err(invalid(format!(
            "the expectation for {name:?} names no mutation; a count says how many mutations one \
             reason was written for, and none of them is not a claim"
        )));
    }
    if expectation.count.is_some() && expectation.id.is_some() {
        return Err(invalid(format!(
            "the expectation for {name:?} counts what an identity names; an identity is one \
             mutation, and a count is how many a locator stands for"
        )));
    }
    Ok(())
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
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for expectation in &self.mutation.expect {
            let name = expectation.name();
            match (&expectation.id, expectation.is_locator()) {
                (Some(id), false) if !id.trim().is_empty() => {}
                (Some(_), false) => {
                    return Err(invalid(
                        "an expectation names no mutant; write the identity or a prefix of it"
                            .to_owned(),
                    ));
                }
                (Some(_), true) => {
                    return Err(invalid(format!(
                        "the expectation for {name:?} names a mutant twice, by identity and by \
                         where it is; a claim names it once"
                    )));
                }
                (None, true) => {
                    for (field, value) in [
                        ("path", &expectation.path),
                        ("item", &expectation.item),
                        ("rule", &expectation.rule),
                        ("original", &expectation.original),
                    ] {
                        if value.as_ref().is_none_or(|text| text.trim().is_empty()) {
                            return Err(invalid(format!(
                                "the expectation for {name:?} names no {field}; a locator is a \
                                 path, an item, a rule and the bytes the edit replaces"
                            )));
                        }
                    }
                }
                (None, false) => {
                    return Err(invalid(
                        "an expectation names no mutant; write the identity or a prefix of it, \
                         or the path, item, rule and original of one"
                            .to_owned(),
                    ));
                }
            }
            check_count(expectation, &name, invalid)?;
            if expectation.reason.trim().is_empty() {
                return Err(invalid(format!(
                    "the expectation for {name:?} has a blank reason, which is a suppression \
                     rather than a claim a report can audit"
                )));
            }
            let Some(outcome) = expectation.outcome() else {
                return Err(invalid(format!(
                    "the expectation for {name:?} expects {:?}, which is not an outcome; write \
                     survived, killed, or timed_out",
                    expectation.outcome.as_deref().unwrap_or_default()
                )));
            };
            if !matches!(
                outcome,
                Outcome::Survived | Outcome::Killed | Outcome::TimedOut
            ) {
                return Err(invalid(format!(
                    "the expectation for {name:?} expects {}, which is not an outcome a run \
                     confirms; write survived, killed, or timed_out",
                    outcome.name()
                )));
            }
            if !seen.insert(name.clone()) {
                return Err(invalid(format!(
                    "two expectations name {name:?}; a mutant has one reason"
                )));
            }
        }
        self.check_skips(invalid)
    }

    fn check_skips(&self, invalid: &impl Fn(String) -> ConfigError) -> Result<(), ConfigError> {
        for skip in &self.mutation.skip {
            if skip.path.trim().is_empty() {
                return Err(invalid(
                    "a skip names no path; write a glob against the workspace-relative path"
                        .to_owned(),
                ));
            }
            if skip.reason.trim().is_empty() {
                return Err(invalid(format!(
                    "the skip for {:?} has a blank reason, which is a suppression rather than a \
                     decision a reviewer can read",
                    skip.path
                )));
            }
            if skip.lines.is_some() && skip.item.is_some() {
                return Err(invalid(format!(
                    "the skip for {:?} says where twice, by lines and by item; a skip says it once",
                    skip.path
                )));
            }
            if let Some(text) = &skip.lines {
                if skip.path.contains(['*', '?', '[']) {
                    return Err(invalid(format!(
                        "the skip for {:?} names lines of a glob; line forty of every file it \
                         matches is not a place anybody meant",
                        skip.path
                    )));
                }
                let Some((from, to)) = skip.range() else {
                    return Err(invalid(format!(
                        "the skip for {:?} writes its lines as {text:?}; write them as from-to",
                        skip.path
                    )));
                };
                if from == 0 || to < from {
                    return Err(invalid(format!(
                        "the skip for {:?} names lines {from} to {to}, which describes nothing",
                        skip.path
                    )));
                }
            }
            if let Err(error) = skip.rule() {
                return Err(invalid(format!(
                    "the skip for {:?} is not a pattern: {error}",
                    skip.path
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
        if !directory.components().all(|part| {
            matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            return Err(invalid(format!(
                "the report directory {} leaves the workspace; name one relative to its root",
                directory.display()
            )));
        }
        let Stryker { high, low } = self.reports.stryker;
        if high > 100 || low > 100 {
            return Err(invalid(format!(
                "the Stryker thresholds are percentages; {high} and {low} are not both between \
                 0 and 100"
            )));
        }
        if low > high {
            return Err(invalid(format!(
                "the Stryker thresholds read low {low} above high {high}; a reader shows green at \
                 or above high and red below low"
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
# include = []                   # workspace-relative globs a file must match to be mutated
# exclude = []                   # workspace-relative globs naming files nothing is mutated in
# allow_outside = []             # directories outside the root the build may read

[snapshot]
# omit = []                      # workspace-relative globs the copy does not carry at all;
#                                # a file a crate declares as a module leaves a tree that
#                                # does not compile, which is why it is not spelled `exclude`

[build]
# features = []                   # cargo features to turn on
# all_features = false            # every feature of every selected package
# no_default_features = false     # leave the default features off
# target = \"\"                     # target triple; empty = the host
# profile = \"\"                    # cargo profile; empty = each command's default
# jobs = 0                        # cargo compilation jobs; 0 = cargo decides
# debug = false                   # write debug information; off, because nothing here reads a backtrace

[mutation]
# tier = \"{tier}\"            # {tiers}
# operators = []                 # exactly these rules; empty = the tier
# timeout = \"{timeout}\"                # auto = 5x the target's own baseline, never below 30s
# build_timeout = \"\"             # empty = no bound
# verify = true                  # run the instrumented baseline before believing a mutant
# coverage = false               # build once with LLVM coverage and route by its regions as well
# touch = true                   # ask the guards which tests reached them, and run only those
# equivalence = false            # ask the compiler whether a survivor's mutation is one it renders

# A mutant a reviewer declared equivalent. The run confirms the claim and
# reports a stale expectation rather than hiding the mutant.
# [[mutation.expect]]
# id = \"\"                        # identity, or a prefix that names exactly one
# reason = \"\"                    # required
# outcome = \"survived\"           # survived | killed | timed_out

# The same claim, addressed by where the mutation is rather than by an
# identity the next edit to the file will change. Never both.
# [[mutation.expect]]
# path = \"src/lib.rs\"
# item = \"clamp\"                 # a suffix of the item path is enough
# rule = \"le-to-lt\"
# original = \"<=\"                # the bytes the edit replaces
# line = 42                       # a hint, when the rest names more than one
# reason = \"\"                    # required
# outcome = \"survived\"           # survived | killed | timed_out

# A place a reviewer decided is not worth measuring. The same decision a
# rust-mutants: skip comment makes, written where the code cannot be edited.
# [[mutation.skip]]
# path = \"src/scanner/**\"        # glob against the workspace-relative path
# lines = \"40-58\"                # inclusive; only with a literal path
# item = \"Scanner::skip_ws\"      # a suffix of the item path; not with lines
# reason = \"\"                    # required

[execution]
# offline = false
# locked = false
# doctests = true                # run a library's documented examples as a target
# skip_targets = []              # target ids never to start, as pkg/kind/name
# jobs = 0                        # mutants measured at once; 0 = the machine, capped at 4
# test_binary_args = []          # allowed: {allowed}
# scratch_working_directory = false # start each test process in a directory of its own,
#                                # so a test that writes where it runs does not write into
#                                # the tree being measured

[reports]
# directory = \"{directory}\"   # workspace-relative
# keep = {keep}                       # run directories kept

# What a Stryker reader colours by. Nothing here decides anything: a threshold
# is not a claim anybody can check, and the gate is an expectation with a reason.
# [reports.stryker]
# high = {high}                       # at or above this, a reader shows green
# low = {low}                        # below this, a reader shows red
",
        high = Stryker::default().high,
        low = Stryker::default().low,
        tier = Tier::Balanced.name(),
        tiers = Tier::ALL.map(Tier::name).join(" | "),
        timeout = AUTO,
        allowed = ALLOWED_TEST_ARGS.join(", "),
        directory = DEFAULT_REPORTS_DIRECTORY,
        keep = DEFAULT_REPORTS_KEEP,
    )
}
