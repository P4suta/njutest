// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running one test process per mutant, and reading what its exit status means.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cargo::{
    CargoError, CargoErrorKind, CompileKind, CompileOptions, Driver, Message, Package, Target,
    compile,
};
use crate::id::{is_digest, is_id};
use crate::instrument::{
    ACTIVE_ENV, CATALOG_ENV, STEP_NONCE_ENV, STEP_NOTICE_ENV, STEP_NOTICE_SCHEMA,
    STEP_PROTOCOL_EXIT, STEP_STATE_ENV, STEP_STATE_SCHEMA, STEPS_ENV, TOUCH_ENV,
};
use crate::outcome::Outcome;
use crate::runner::{
    Bound, Cancel, EXIT_CODE_UNAVAILABLE, ProcessExit, Progress, RunResult, Spec, Termination, run,
};
use crate::trace::{ExecRecord, Recorder};

/// Every variable the engine owns.
/// A test process sees exactly the ones this run set, never one an outer run left behind.
pub const RESERVED_ENV: [&str; 7] = [
    ACTIVE_ENV,
    CATALOG_ENV,
    TOUCH_ENV,
    STEPS_ENV,
    STEP_NOTICE_ENV,
    STEP_NONCE_ENV,
    STEP_STATE_ENV,
];

/// The variables a run composes for every test process it starts, which it therefore never lets one inherit.
pub const COMPOSED_ENV: [&str; 8] = [
    ACTIVE_ENV,
    CATALOG_ENV,
    TOUCH_ENV,
    STEPS_ENV,
    STEP_NOTICE_ENV,
    STEP_NONCE_ENV,
    STEP_STATE_ENV,
    crate::coverage::PROFILE_ENV,
];

/// How many quiet windows a step-counted execution may run for in all before the clock ends it anyway.
pub const QUIET_WINDOWS_PER_CEILING: u32 = 10;

/// The name a test process writes its coverage profile under, when the run is not the one measuring.
pub const SPILLED_PROFILE: &str = "spilled-coverage-%p-%m.profraw";

/// The kinds of target that carry tests the engine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum TargetKind {
    /// The library's own unit tests.
    Lib,
    /// A binary's own unit tests.
    Bin,
    /// An integration test.
    Test,
    /// An example built with `test = true`.
    Example,
    /// A procedural macro crate's own unit tests, which are an ordinary test binary.
    ProcMacro,
    /// A library's documentation examples, which cargo runs and rustdoc compiles.
    Doc,
}

impl TargetKind {
    /// The name used in a target id and in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lib => "lib",
            Self::Bin => "bin",
            Self::Test => "test",
            Self::Example => "example",
            Self::ProcMacro => "proc-macro",
            Self::Doc => "doc",
        }
    }

    /// The kind named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The kind of a cargo target, or `None` for one that carries no tests the engine runs (a build script, a bench).
    #[must_use]
    pub fn of(target: &Target) -> Option<Self> {
        if target.is_custom_build() || target.is_bench() {
            None
        } else if target.is_proc_macro() {
            Some(Self::ProcMacro)
        } else if target.is_lib() {
            Some(Self::Lib)
        } else if target.is_bin() {
            Some(Self::Bin)
        } else if target.is_test() {
            Some(Self::Test)
        } else if target.is_example() {
            Some(Self::Example)
        } else {
            None
        }
    }
}

/// The stable name of a test target: `package/kind/name`.
#[must_use]
pub fn target_id(package: &str, kind: TargetKind, name: &str) -> String {
    format!("{package}/{}/{name}", kind.name())
}

/// One built test binary.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TestTarget {
    /// `package/kind/name`.
    pub id: String,
    /// The package that owns it.
    pub package: String,
    /// What kind of target it is.
    pub kind: TargetKind,
    /// The target's name.
    pub name: String,
    /// The binary cargo built.
    pub executable: PathBuf,
    /// The directory it runs in: the package's manifest directory, which is what cargo uses and what a test reading a relative path expects.
    pub cwd: PathBuf,
    /// What cargo sets for this target that the parent environment does not have: `CARGO_MANIFEST_DIR`, `CARGO_PKG_*`, `CARGO_BIN_EXE_*`.
    pub cargo_env: Vec<(OsString, OsString)>,
    /// Whether the target is built with the libtest harness.
    pub harness: bool,
    /// What a run could not establish about this target, each named.
    pub limitations: Vec<String>,
    /// The arguments before the harness's own, for a target cargo runs rather than one the engine starts itself.
    /// Empty for a binary, and then `executable` is the binary.
    pub through: Vec<OsString>,
}

impl TestTarget {
    /// One built test binary, by everything cargo says about it that is not optional.
    ///
    /// The identity is derived rather than given: it was a sixth argument that had to equal `target_id(package, kind, name)` and nothing checked it,
    /// so a report could name a target that no run could route to.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "these five are what cargo says about a target and none of them has a \
                  sensible default: a builder that let one be forgotten would build a \
                  target that names no package or runs in no directory"
    )]
    pub fn new(
        package: impl Into<String>,
        kind: TargetKind,
        name: impl Into<String>,
        executable: PathBuf,
        cwd: PathBuf,
    ) -> Self {
        let package = package.into();
        let name = name.into();
        Self {
            id: target_id(&package, kind, &name),
            package,
            kind,
            name,
            executable,
            cwd,
            harness: true,
            limitations: Vec::new(),
            cargo_env: Vec::new(),
            through: Vec::new(),
        }
    }

    /// Whether the target is built with the libtest harness, which decides how its silence is read.
    #[must_use]
    pub const fn with_harness(mut self, harness: bool) -> Self {
        self.harness = harness;
        self
    }

    /// What a run could not establish about this target.
    #[must_use]
    pub fn with_limitations(mut self, limitations: Vec<String>) -> Self {
        self.limitations = limitations;
        self
    }

    /// What cargo sets for this target that the parent environment does not have.
    #[must_use]
    pub fn with_cargo_env(mut self, env: Vec<(OsString, OsString)>) -> Self {
        self.cargo_env = env;
        self
    }

    /// The arguments before the harness's own, for a target cargo runs rather than one the engine starts.
    #[must_use]
    pub fn with_through(mut self, through: Vec<OsString>) -> Self {
        self.through = through;
        self
    }
}

/// The libtest summary line of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Whether the line said `ok`.
    pub ok: bool,
    /// Tests that passed.
    pub passed: u32,
    /// Tests that failed.
    pub failed: u32,
    /// Tests that were ignored.
    pub ignored: u32,
    /// Benchmarks that were measured.
    pub measured: u32,
    /// Tests the filter removed.
    pub filtered_out: u32,
}

impl Summary {
    /// How many tests actually ran.
    #[must_use]
    pub const fn tests_run(&self) -> Option<u32> {
        self.passed.checked_add(self.failed)
    }

    /// Whether nothing ran at all, which is what a filter matching no test looks like: exit 0 with no evidence in it.
    #[must_use]
    pub const fn ran_nothing(&self) -> bool {
        self.passed == 0 && self.failed == 0
    }
}

/// What each test of one run said, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lines {
    /// Every test that passed, in the order the harness printed them.
    pub passed: Vec<String>,
    /// Every test that failed.
    pub failed: Vec<String>,
    /// Every test that was ignored.
    pub ignored: Vec<String>,
}

impl Lines {
    /// Whether the harness printed no verdict at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.passed.is_empty() && self.failed.is_empty() && self.ignored.is_empty()
    }
}

/// A libtest protocol stream was not exact UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the libtest output is not valid UTF-8")]
pub struct LibtestOutputError;

/// Reads every `test <name> ... <verdict>` line of a captured output.
///
/// # Errors
/// Refuses output that is not exact UTF-8 instead of inventing replacement characters in test identities.
pub fn parse_lines(output: &[u8]) -> Result<Lines, LibtestOutputError> {
    let text = std::str::from_utf8(output).map_err(|_not_utf8| LibtestOutputError)?;
    Ok(parse_lines_text(text))
}

fn parse_lines_text(text: &str) -> Lines {
    let mut lines = Lines::default();
    for line in text.lines() {
        let Some((name, verdict)) = verdict_of(line) else {
            continue;
        };
        match verdict {
            "ok" => lines.passed.push(name.to_owned()),
            "FAILED" => lines.failed.push(name.to_owned()),
            _ if verdict.starts_with("ignored") => lines.ignored.push(name.to_owned()),
            _ => {}
        }
    }
    lines
}

/// The name and verdict of one `test <name> ... <verdict>` line.
fn verdict_of(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_end().strip_prefix("test ")?;
    let (name, verdict) = rest.rsplit_once(" ... ")?;
    let name = name.trim();
    (!name.is_empty()).then_some((name, verdict.trim()))
}

/// Reads the last `test result:` line of a captured output.
///
/// # Errors
/// Refuses output that is not exact UTF-8.
pub fn parse_summary(output: &[u8]) -> Result<Option<Summary>, LibtestOutputError> {
    let text = std::str::from_utf8(output).map_err(|_not_utf8| LibtestOutputError)?;
    Ok(parse_summary_text(text))
}

fn parse_summary_text(text: &str) -> Option<Summary> {
    text.lines().rev().find_map(parse_summary_line)
}

fn parse_summary_line(line: &str) -> Option<Summary> {
    let rest = line.trim().strip_prefix("test result: ")?;
    let (verdict, counts) = rest.split_once('.')?;
    let mut summary = Summary {
        ok: verdict.trim() == "ok",
        passed: 0,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 0,
    };
    let mut seen: u8 = 0;
    for part in counts.split(';') {
        let part = part.trim();
        let Some((count, label)) = part.split_once(' ') else {
            continue;
        };
        let Ok(count) = count.parse::<u32>() else {
            continue;
        };
        let bit = match label.trim() {
            "passed" => {
                summary.passed = count;
                1
            }
            "failed" => {
                summary.failed = count;
                2
            }
            "ignored" => {
                summary.ignored = count;
                4
            }
            "measured" => {
                summary.measured = count;
                8
            }
            "filtered out" => {
                summary.filtered_out = count;
                16
            }
            _ => continue,
        };
        if seen & bit != 0 {
            return None;
        }
        seen |= bit;
    }
    (seen > 0 && summary.tests_run().is_some()).then_some(summary)
}

/// How a test process came to an end, which is one thing and not four flags.
///
/// Four booleans and an exit code could say a process was both unstarted and killed by a clock, and the precedence that made that impossible lived in the order of a chain of `if`s.
/// A process ends exactly one way, so the type says so and the policy reading it is a total match rather than a sequence somebody has to keep in the right order (ADR 0023).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Stopped {
    /// The process could not be started at all.
    NotStarted,
    /// The process ended on its own.
    Exited {
        /// The one way it exited.
        exit: ProcessExit,
    },
    /// This machine's wall-clock bound expired, which for a process counting its steps is the ceiling over one that never went quiet.
    TimedOut {
        /// How many step boundaries the process had raised, where the run could read its state.
        ///
        /// Above zero says the clock ended a computation the allowance would have ended, which is a race this machine won and another would not: the number to change is the allowance, not the bound.
        /// Zero says the computation was raising no boundary at all, so no allowance could have ended it and the clock is the only instrument there is (ADR 0023).
        /// `None` says the state could not be read, which is not a count of zero.
        raised: Option<u64>,
    },
    /// The process raised no step boundary for a whole quiet window, so nothing was moving through the mutated source.
    Stalled {
        /// How many step boundaries the process had raised before it went quiet, or `None` where its state could not be read.
        raised: Option<u64>,
    },
    /// The caller asked it to stop.
    Cancelled {
        /// Whether a child had started before cancellation was observed.
        started: bool,
    },
    /// The operating system did not yield a trustworthy final status.
    WaitFailed,
    /// The execution monitor stopped the tree and its notice was verified.
    StepLimitReached {
        /// The verified notice that caused the stop.
        notice: StepLimitNotice,
    },
    /// A monitor stop or notice did not satisfy the step protocol.
    StepProtocolFailed {
        /// The closed protocol stage that failed.
        reason: StepProtocolFailure,
    },
}

/// Why a bounded execution's nonce-correlated, structurally verified step protocol failed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StepProtocolFailure {
    /// The monitor path was not a regular file.
    MonitorInvalid {
        /// The monitor path.
        path: String,
    },
    /// The monitor path could not be inspected.
    MonitorInspect {
        /// The monitor path.
        path: String,
        /// The operating-system diagnostic.
        detail: String,
    },
    /// The generated runtime reported that it could not publish a notice.
    Publication {},
    /// A monitor stop had no completed notice.
    NoticeMissing {},
    /// The opened notice was not a regular file.
    NoticeNotRegular {
        /// The notice path.
        path: String,
    },
    /// Notice metadata could not be read.
    NoticeMetadata {
        /// The notice path.
        path: String,
        /// The operating-system diagnostic.
        detail: String,
    },
    /// The notice could not be opened without following a link.
    NoticeOpen {
        /// The notice path.
        path: String,
        /// The operating-system diagnostic.
        detail: String,
    },
    /// Notice bytes could not be read.
    NoticeRead {
        /// The notice path.
        path: String,
        /// The operating-system diagnostic.
        detail: String,
    },
    /// The notice exceeded the protocol's fixed byte ceiling.
    NoticeTooLarge {
        /// The notice path.
        path: String,
        /// The fixed protocol ceiling in bytes.
        limit: usize,
    },
    /// The notice was not UTF-8.
    NoticeNotUtf8 {
        /// The notice path.
        path: String,
    },
    /// The notice contained no record.
    NoticeEmpty {},
    /// The notice was not the exact newline-terminated one-line wire form emitted by the runtime.
    NoticeNonCanonical {},
    /// The notice contained more than one record.
    NoticeExtraRecord {},
    /// The notice named another execution.
    ExecutionMismatch {},
    /// The notice allowance was malformed.
    InvalidLimit {},
    /// The notice observed count was malformed.
    InvalidObserved {},
    /// The notice did not carry the exact expected boundary.
    BoundaryMismatch {},
    /// A consumed notice or partial notice could not be removed.
    Cleanup {
        /// The path that could not be removed.
        path: String,
        /// The operating-system diagnostic.
        detail: String,
    },
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum StoppedWire {
    NotStarted {},
    Exited { exit: ProcessExit },
    TimedOut { raised: Option<u64> },
    Stalled { raised: Option<u64> },
    Cancelled { started: bool },
    WaitFailed {},
    StepLimitReached { notice: StepLimitNotice },
    StepProtocolFailed { reason: StepProtocolFailure },
}

impl<'de> serde::Deserialize<'de> for Stopped {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(
            match <StoppedWire as serde::Deserialize>::deserialize(deserializer)? {
                StoppedWire::NotStarted {} => Self::NotStarted,
                StoppedWire::Exited { exit } => Self::Exited { exit },
                StoppedWire::TimedOut { raised } => Self::TimedOut { raised },
                StoppedWire::Stalled { raised } => Self::Stalled { raised },
                StoppedWire::Cancelled { started } => Self::Cancelled { started },
                StoppedWire::WaitFailed {} => Self::WaitFailed,
                StoppedWire::StepLimitReached { notice } => Self::StepLimitReached { notice },
                StoppedWire::StepProtocolFailed { reason } => Self::StepProtocolFailed { reason },
            },
        )
    }
}

/// A runtime notice verified against the execution that supplied its fresh path and nonce.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StepLimitNotice {
    nonce: String,
    catalog: String,
    mutant: String,
    limit: u64,
    observed: u64,
}

/// The closed set of source boundaries a step notice can count.
///
/// Macro expansions and dependency code are not rewritten by the workspace instrumenter.
/// A computation that enters either without re-entering an instrumented function or closure can therefore end only by itself or by the wall-clock supervisor, whose outcome is [`crate::outcome::Outcome::Waited`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepBoundaryScope {
    /// Non-const function entries, loop bodies, async blocks, and every closure invocation in mutable workspace source.
    InstrumentedWorkspaceSource,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StepNoticeWire {
    nonce: String,
    catalog: String,
    mutant: String,
    limit: u64,
    observed: u64,
}

#[derive(Debug, thiserror::Error)]
enum StepNoticeError {
    #[error("a step notice nonce is not 16 lowercase hexadecimal bytes")]
    InvalidNonce,
    #[error("a step notice catalog is not a canonical SHA-256 digest")]
    InvalidCatalog,
    #[error("a step notice mutant is not a canonical full mutant id")]
    InvalidMutant,
    #[error("a step notice allowance is zero")]
    ZeroLimit,
    #[error("a step notice observed count is not exactly one past its allowance")]
    InvalidBoundary,
}

impl<'de> serde::Deserialize<'de> for StepLimitNotice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = <StepNoticeWire as serde::Deserialize>::deserialize(deserializer)?;
        Self::checked(wire).map_err(serde::de::Error::custom)
    }
}

impl StepLimitNotice {
    fn checked(wire: StepNoticeWire) -> Result<Self, StepNoticeError> {
        let StepNoticeWire {
            nonce,
            catalog,
            mutant,
            limit,
            observed,
        } = wire;
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(StepNoticeError::InvalidNonce);
        }
        if !is_digest(&catalog) {
            return Err(StepNoticeError::InvalidCatalog);
        }
        if !is_id(&mutant) {
            return Err(StepNoticeError::InvalidMutant);
        }
        if limit == 0 {
            return Err(StepNoticeError::ZeroLimit);
        }
        if limit.checked_add(1) != Some(observed) {
            return Err(StepNoticeError::InvalidBoundary);
        }
        Ok(Self {
            nonce,
            catalog,
            mutant,
            limit,
            observed,
        })
    }

    /// A deterministic valid notice for schema and projection tests.
    #[cfg(any(test, feature = "testkit", kani))]
    #[must_use]
    pub fn specimen() -> Self {
        Self {
            nonce: "00000000000000000000000000000000".to_owned(),
            catalog: "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
            mutant: "1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            limit: 10,
            observed: 11,
        }
    }

    /// The per-execution nonce that prevents stale or cross-run attribution.
    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// The catalog whose generated guard published the notice.
    #[must_use]
    pub fn catalog(&self) -> &str {
        &self.catalog
    }

    /// The full mutant identity selected for the execution.
    #[must_use]
    pub fn mutant(&self) -> &str {
        &self.mutant
    }

    /// The configured allowance.
    #[must_use]
    pub const fn limit(&self) -> u64 {
        self.limit
    }

    /// The first count outside the allowance.
    #[must_use]
    pub const fn observed(&self) -> u64 {
        self.observed
    }

    /// Which execution boundaries this deterministic count covers.
    #[must_use]
    pub const fn scope(&self) -> StepBoundaryScope {
        match self {
            Self {
                nonce: _,
                catalog: _,
                mutant: _,
                limit: _,
                observed: _,
            } => StepBoundaryScope::InstrumentedWorkspaceSource,
        }
    }
}

impl Stopped {
    /// How a generic supervised run came to an end.
    /// Execution-specific code replaces a monitored stop with its verified protocol fact.
    #[must_use]
    pub fn of(result: &RunResult) -> Self {
        match &result.termination {
            Termination::NotStarted { .. } => Self::NotStarted,
            Termination::Exited(exit) => Self::Exited { exit: *exit },
            Termination::TimedOut => Self::TimedOut { raised: None },
            Termination::Stalled => Self::Stalled { raised: None },
            Termination::StoppedByMonitor => Self::StepProtocolFailed {
                reason: StepProtocolFailure::NoticeMissing {},
            },
            Termination::MonitorFailed { failure } => Self::StepProtocolFailed {
                reason: StepProtocolFailure::from_monitor(failure),
            },
            Termination::Cancelled { started } => Self::Cancelled { started: *started },
            Termination::WaitFailed { .. } => Self::WaitFailed,
        }
    }
}

impl StepProtocolFailure {
    fn from_monitor(failure: &crate::runner::MonitorFailure) -> Self {
        match failure {
            crate::runner::MonitorFailure::InvalidType { path } => Self::MonitorInvalid {
                path: path.display().to_string(),
            },
            crate::runner::MonitorFailure::Inspect { path, source } => Self::MonitorInspect {
                path: path.display().to_string(),
                detail: source.to_string(),
            },
        }
    }
}

/// What a run of a test binary established before mutation policy reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// Its single terminal fact.
    pub stopped: Stopped,
    /// Whether the runtime said the binary was built from another catalog.
    pub stale_catalog: bool,
}

impl Observation {
    fn of(result: &RunResult, step: Option<&ExpectedStep>) -> Self {
        let stopped = observed_stop(result, step);
        Self {
            stopped,
            stale_catalog: said(&result.output, crate::instrument::STALE_CATALOG_MARKER),
        }
    }
}

/// Whether `output` holds `needle`.
fn said(output: &[u8], needle: &str) -> bool {
    output
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// What the supervisor alone knows about the notice one execution may publish.
#[derive(Debug)]
struct ExpectedStep {
    path: PathBuf,
    state_path: PathBuf,
    nonce: String,
    catalog: String,
    mutant: String,
    limit: u64,
}

/// A step side-channel record that cannot be safely attributed to this execution.
#[derive(Debug, thiserror::Error)]
enum StepSetupError {
    #[error("the step allowance {limit} does not fit the counter of this target platform")]
    LimitTooWide { limit: u64 },
    #[error("the step allowance {limit} leaves no value for the first step past it")]
    BoundaryOverflow { limit: u64 },
    #[error("a step-bounded execution requires a private scratch directory")]
    ScratchRequired,
    #[error("could not create a fresh step-notice nonce: {error}")]
    NonceUnavailable { error: getrandom::Error },
    #[error("the fresh step-notice path {path} already exists")]
    FreshPathExists { path: PathBuf },
    #[error("could not inspect the fresh step-notice path {path}: {source}")]
    FreshPathInspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not create fresh step state {path}: {source}")]
    StateCreate {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not initialize fresh step state {path}: {source}")]
    StateWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not sync fresh step state {path}: {source}")]
    StateSync {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the active catalog is not a canonical SHA-256 digest")]
    InvalidCatalog,
    #[error("the active mutant is not a canonical full mutant id")]
    InvalidMutant,
}

#[derive(Debug, thiserror::Error)]
enum NoticeError {
    #[error("could not open step notice {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not inspect step notice {path}: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("step notice {path} is not a regular file")]
    NotRegular { path: PathBuf },
    #[error("could not read step notice {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("step notice {path} exceeds {limit} bytes")]
    TooLarge { path: PathBuf, limit: usize },
    #[error("step notice {path} is not UTF-8")]
    NotUtf8 { path: PathBuf },
    #[error("could not remove consumed step notice {path}: {source}")]
    Remove {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the step notice is empty")]
    Empty,
    #[error("the step notice is not in the one-line canonical wire form")]
    NonCanonical,
    #[error("the step notice has more than one record")]
    ExtraRecord,
    #[error("the step notice does not name this execution exactly")]
    ExecutionMismatch,
    #[error("the step notice has no valid allowance")]
    InvalidLimit,
    #[error("the step notice has no valid observed count")]
    InvalidObserved,
    #[error("the step notice does not carry the expected boundary")]
    BoundaryMismatch,
}

impl NoticeError {
    fn failure(&self) -> StepProtocolFailure {
        match self {
            Self::Metadata { path, source } => StepProtocolFailure::NoticeMetadata {
                path: path.display().to_string(),
                detail: source.to_string(),
            },
            Self::NotRegular { path } => StepProtocolFailure::NoticeNotRegular {
                path: path.display().to_string(),
            },
            Self::Read { path, source } => StepProtocolFailure::NoticeRead {
                path: path.display().to_string(),
                detail: source.to_string(),
            },
            Self::TooLarge { path, limit } => StepProtocolFailure::NoticeTooLarge {
                path: path.display().to_string(),
                limit: *limit,
            },
            Self::NotUtf8 { path } => StepProtocolFailure::NoticeNotUtf8 {
                path: path.display().to_string(),
            },
            Self::Open { path, source } => StepProtocolFailure::NoticeOpen {
                path: path.display().to_string(),
                detail: source.to_string(),
            },
            Self::Remove { path, source } => StepProtocolFailure::Cleanup {
                path: path.display().to_string(),
                detail: source.to_string(),
            },
            Self::Empty => StepProtocolFailure::NoticeEmpty {},
            Self::NonCanonical => StepProtocolFailure::NoticeNonCanonical {},
            Self::ExtraRecord => StepProtocolFailure::NoticeExtraRecord {},
            Self::ExecutionMismatch => StepProtocolFailure::ExecutionMismatch {},
            Self::InvalidLimit => StepProtocolFailure::InvalidLimit {},
            Self::InvalidObserved => StepProtocolFailure::InvalidObserved {},
            Self::BoundaryMismatch => StepProtocolFailure::BoundaryMismatch {},
        }
    }
}

impl ExpectedStep {
    fn new(context: &Context<'_>, scratch: Option<&Path>) -> Result<Option<Self>, StepSetupError> {
        let (Some((mutant, catalog)), Some(limit)) = (context.active, context.steps) else {
            return Ok(None);
        };
        if limit == 0 {
            return Ok(None);
        }
        let runtime_limit =
            usize::try_from(limit).map_err(|_error| StepSetupError::LimitTooWide { limit })?;
        if runtime_limit.checked_add(1).is_none() {
            return Err(StepSetupError::BoundaryOverflow { limit });
        }
        if !is_digest(catalog) {
            return Err(StepSetupError::InvalidCatalog);
        }
        if !is_id(mutant) {
            return Err(StepSetupError::InvalidMutant);
        }
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|error| StepSetupError::NonceUnavailable { error })?;
        let nonce = hex::encode(bytes);
        let directory = scratch.ok_or(StepSetupError::ScratchRequired)?;
        let path = directory.join(format!("rust-mutants-step-{nonce}.notice"));
        let state_path = directory.join(format!("rust-mutants-step-{nonce}.state"));
        match std::fs::symlink_metadata(&path) {
            Ok(_metadata) => return Err(StepSetupError::FreshPathExists { path }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(StepSetupError::FreshPathInspect { path, source });
            }
        }
        let mut state = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&state_path)
            .map_err(|source| StepSetupError::StateCreate {
                path: state_path.clone(),
                source,
            })?;
        let initial =
            format!("{STEP_STATE_SCHEMA}\t{nonce}\t{catalog}\t{mutant}\t{limit}\tdormant\t0\n");
        state
            .write_all(initial.as_bytes())
            .map_err(|source| StepSetupError::StateWrite {
                path: state_path.clone(),
                source,
            })?;
        state
            .sync_data()
            .map_err(|source| StepSetupError::StateSync {
                path: state_path.clone(),
                source,
            })?;
        Ok(Some(Self {
            path,
            state_path,
            nonce,
            catalog: catalog.to_owned(),
            mutant: mutant.to_owned(),
            limit,
        }))
    }

    fn add_environment(&self, env: &mut Vec<(OsString, OsString)>) {
        env.push((
            OsString::from(STEP_NOTICE_ENV),
            self.path.as_os_str().to_owned(),
        ));
        env.push((OsString::from(STEP_NONCE_ENV), OsString::from(&self.nonce)));
        env.push((
            OsString::from(STEP_STATE_ENV),
            self.state_path.as_os_str().to_owned(),
        ));
    }

    /// How many boundaries the process had raised when it stopped, from the state it shares.
    ///
    /// A clock that ends a computation raising this count ended one the allowance would have ended, which is a race rather than a measurement.
    /// A clock that ends one raising nothing ended a computation that was not passing through instrumented source at all, and there the clock is the only instrument there is (ADR 0023).
    /// `None` says the state could not be read, which is not the same as a count of zero.
    fn raised(&self) -> Option<u64> {
        let bytes = match crate::runner::read_side_channel(&self.state_path) {
            Ok(bytes) => bytes,
            Err(_the_state_is_not_readable) => return None,
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_the_state_is_not_text) => return None,
        };
        let mut fields = text.trim_end().split('\t');
        let spent = fields.next_back()?;
        match spent.parse::<u64>() {
            Ok(spent) => Some(spent),
            Err(_the_state_does_not_end_in_a_count) => None,
        }
    }

    fn read(&self) -> Result<Option<StepLimitNotice>, NoticeError> {
        const MAX_NOTICE_BYTES: usize = 16 * 1024;
        const MAX_NOTICE_BYTES_U64: u64 = 16 * 1024;

        let file = match open_notice_without_following(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(NoticeError::Open {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        let metadata = file.metadata().map_err(|source| NoticeError::Metadata {
            path: self.path.clone(),
            source,
        })?;
        if !metadata.file_type().is_file() {
            return Err(NoticeError::NotRegular {
                path: self.path.clone(),
            });
        }
        if metadata.len() > MAX_NOTICE_BYTES_U64 {
            return Err(NoticeError::TooLarge {
                path: self.path.clone(),
                limit: MAX_NOTICE_BYTES,
            });
        }
        let capacity = usize::try_from(metadata.len()).map_err(|_error| NoticeError::TooLarge {
            path: self.path.clone(),
            limit: MAX_NOTICE_BYTES,
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(MAX_NOTICE_BYTES_U64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| NoticeError::Read {
                path: self.path.clone(),
                source,
            })?;
        if bytes.len() > MAX_NOTICE_BYTES {
            return Err(NoticeError::TooLarge {
                path: self.path.clone(),
                limit: MAX_NOTICE_BYTES,
            });
        }
        let text = String::from_utf8(bytes).map_err(|_error| NoticeError::NotUtf8 {
            path: self.path.clone(),
        })?;
        self.parse_notice(&text).map(Some)
    }

    /// Parses the one canonical record from an already bounded regular file.
    fn parse_notice(&self, text: &str) -> Result<StepLimitNotice, NoticeError> {
        if text.is_empty() {
            return Err(NoticeError::Empty);
        }
        let line = text.strip_suffix('\n').ok_or(NoticeError::NonCanonical)?;
        if line.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
            return Err(NoticeError::ExtraRecord);
        }
        let mut fields = line.split('\t');
        let schema = fields.next();
        let nonce = fields.next();
        let catalog = fields.next();
        let mutant = fields.next();
        let limit = fields.next();
        let observed = fields.next();
        if fields.next().is_some()
            || schema != Some(STEP_NOTICE_SCHEMA)
            || nonce != Some(self.nonce.as_str())
            || catalog != Some(self.catalog.as_str())
            || mutant != Some(self.mutant.as_str())
        {
            return Err(NoticeError::ExecutionMismatch);
        }
        let limit_field = limit.ok_or(NoticeError::InvalidLimit)?;
        let limit = limit_field
            .parse::<u64>()
            .map_err(|_error| NoticeError::InvalidLimit)?;
        if limit.to_string() != limit_field {
            return Err(NoticeError::InvalidLimit);
        }
        let observed_field = observed.ok_or(NoticeError::InvalidObserved)?;
        let observed = observed_field
            .parse::<u64>()
            .map_err(|_error| NoticeError::InvalidObserved)?;
        if observed.to_string() != observed_field {
            return Err(NoticeError::InvalidObserved);
        }
        if limit != self.limit || self.limit.checked_add(1) != Some(observed) {
            return Err(NoticeError::BoundaryMismatch);
        }
        let notice = StepLimitNotice {
            nonce: self.nonce.clone(),
            catalog: self.catalog.clone(),
            mutant: self.mutant.clone(),
            limit,
            observed,
        };
        Ok(notice)
    }

    fn clear(&self) -> Result<(), NoticeError> {
        remove_notice(&self.path)?;
        let partial = self.path.with_extension("notice.partial");
        remove_notice(&partial)?;
        remove_notice(&self.state_path)
    }
}

#[cfg(unix)]
fn open_notice_without_following(path: &Path) -> std::io::Result<std::fs::File> {
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(std::fs::File::from(descriptor))
}

#[cfg(windows)]
fn open_notice_without_following(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn remove_notice(path: &Path) -> Result<(), NoticeError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(NoticeError::Remove {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn observed_stop(result: &RunResult, step: Option<&ExpectedStep>) -> Stopped {
    if matches!(
        result.termination,
        Termination::NotStarted { .. } | Termination::Cancelled { .. }
    ) {
        if let Some(step) = step
            && let Err(reason) = step.clear()
        {
            return Stopped::StepProtocolFailed {
                reason: reason.failure(),
            };
        }
        return Stopped::of(result);
    }
    let Some(expected) = step else {
        return Stopped::of(result);
    };
    let notice = expected.read();
    let raised = expected.raised();
    if let Err(reason) = expected.clear() {
        return Stopped::StepProtocolFailed {
            reason: reason.failure(),
        };
    }
    match notice {
        Ok(Some(notice)) => Stopped::StepLimitReached { notice },
        Ok(None)
            if matches!(
                result.termination,
                Termination::Exited(ProcessExit::Code(STEP_PROTOCOL_EXIT))
            ) =>
        {
            Stopped::StepProtocolFailed {
                reason: StepProtocolFailure::Publication {},
            }
        }
        Ok(None) if matches!(result.termination, Termination::StoppedByMonitor) => {
            Stopped::StepProtocolFailed {
                reason: StepProtocolFailure::NoticeMissing {},
            }
        }
        Ok(None) if matches!(result.termination, Termination::TimedOut) => {
            Stopped::TimedOut { raised }
        }
        Ok(None) if matches!(result.termination, Termination::Stalled) => {
            Stopped::Stalled { raised }
        }
        Ok(None) => Stopped::of(result),
        Err(reason) => Stopped::StepProtocolFailed {
            reason: reason.failure(),
        },
    }
}

/// What one run of a test binary establishes about the mutant that was active during it.
/// See the module documentation for the order.
#[must_use]
pub const fn outcome_of(
    observed: &Observation,
    summary: Option<Summary>,
    harness: bool,
) -> Outcome {
    let exit = match &observed.stopped {
        Stopped::NotStarted | Stopped::WaitFailed | Stopped::StepProtocolFailed { .. } => {
            return Outcome::Errored;
        }
        Stopped::TimedOut { .. } | Stopped::Stalled { .. } => return Outcome::Waited,
        Stopped::Cancelled { .. } => return Outcome::NotRun,
        Stopped::StepLimitReached { .. } => return Outcome::StepLimitReached,
        Stopped::Exited { exit } => *exit,
    };
    if observed.stale_catalog {
        return Outcome::Errored;
    }
    let code = match exit {
        ProcessExit::Code(code) => code,
        ProcessExit::Signal(_) => return Outcome::Killed,
        ProcessExit::Unknown => return Outcome::NotRun,
    };
    if code != 0 {
        return Outcome::Killed;
    }
    if !harness {
        return Outcome::Survived;
    }
    match summary {
        Some(summary) if !summary.ran_nothing() => Outcome::Survived,
        _ => Outcome::Inconclusive,
    }
}

/// Where the build put each binary, by package and by target name.
fn binaries_built(
    messages: &[Message],
) -> Result<BTreeMap<String, BTreeMap<String, PathBuf>>, CargoError> {
    let mut found: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for message in messages {
        let Message::CompilerArtifact(artifact) = message else {
            continue;
        };
        let Some(executable) = &artifact.executable else {
            continue;
        };
        if artifact.profile.test || !artifact.target.is_bin() {
            continue;
        }
        match found
            .entry(artifact.package_id.clone())
            .or_default()
            .entry(artifact.target.name.clone())
        {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(executable.clone());
            }
            std::collections::btree_map::Entry::Occupied(slot) if slot.get() == executable => {}
            std::collections::btree_map::Entry::Occupied(slot) => {
                return Err(CargoError::new(
                    CargoErrorKind::MessageUnparsable,
                    format!(
                        "cargo named both {} and {} as binary {} of package {}",
                        slot.get().display(),
                        executable.display(),
                        artifact.target.name,
                        artifact.package_id
                    ),
                ));
            }
        }
    }
    Ok(found)
}

/// What a package's own build script left for every unit of that package: where it wrote, and what it put in the environment.
fn built_by_a_script(messages: &[Message], package_id: &str) -> Vec<(OsString, OsString)> {
    let mut found = Vec::new();
    for message in messages {
        let Message::BuildScriptExecuted(script) = message else {
            continue;
        };
        if script.package_id != package_id {
            continue;
        }
        if let Some(out_dir) = &script.out_dir {
            found.push((OsString::from("OUT_DIR"), out_dir.as_os_str().to_owned()));
        }
        for (name, value) in &script.env {
            found.push((OsString::from(name), OsString::from(value)));
        }
    }
    found
}

/// The environment one test process runs with: the base the workspace was opened with, the variables cargo sets for the target, the activation, and a temporary directory of the worker's own.
///
/// # Errors
/// Returns an I/O error when the toolchain's target-library directory cannot be inspected exactly.
pub fn environment(
    context: &Context<'_>,
    target: &TestTarget,
    scratch: Option<&Path>,
) -> std::io::Result<Vec<(OsString, OsString)>> {
    let (base, active, cargo) = (context.base_env, context.active, context.cargo);
    let mut env: BTreeMap<OsString, OsString> = base
        .iter()
        .filter(|(name, _)| {
            !COMPOSED_ENV
                .iter()
                .any(|composed| name == OsStr::new(composed))
        })
        .cloned()
        .collect();
    env.extend(target.cargo_env.iter().cloned());
    if let Some(cargo) = cargo {
        env.insert(OsString::from("CARGO"), cargo.as_os_str().to_owned());
    }
    if let Some(sysroot) = context.sysroot {
        let (name, value) = library_path(sysroot, base)?;
        env.insert(name, value);
    }
    if let Some((id, catalog)) = active {
        env.insert(OsString::from(ACTIVE_ENV), OsString::from(id));
        env.insert(OsString::from(CATALOG_ENV), OsString::from(catalog));
        if let Some(steps) = context.steps {
            env.insert(OsString::from(STEPS_ENV), OsString::from(steps.to_string()));
        }
    }
    if let Some(touch) = context.touch {
        env.insert(OsString::from(TOUCH_ENV), touch.log.as_os_str().to_owned());
        env.insert(OsString::from(CATALOG_ENV), OsString::from(touch.catalog));
    }
    match (context.profile, scratch) {
        (Some(profile), _) => {
            env.insert(
                OsString::from(crate::coverage::PROFILE_ENV),
                profile.as_os_str().to_owned(),
            );
        }
        (None, Some(scratch)) => {
            env.insert(
                OsString::from(crate::coverage::PROFILE_ENV),
                scratch.join(SPILLED_PROFILE).into_os_string(),
            );
        }
        (None, None) => {}
    }
    if let Some(scratch) = scratch {
        for name in ["TMPDIR", "TMP", "TEMP"] {
            env.insert(OsString::from(name), scratch.as_os_str().to_owned());
        }
    }
    Ok(env.into_iter().collect())
}

/// The variable a dynamically linked test binary is found through, and what it should hold.
fn library_path(
    sysroot: &Path,
    base: &[(OsString, OsString)],
) -> std::io::Result<(OsString, OsString)> {
    let name = if cfg!(target_os = "macos") {
        "DYLD_FALLBACK_LIBRARY_PATH"
    } else if cfg!(windows) {
        "PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let separator = OsString::from(if cfg!(windows) { ";" } else { ":" });
    let mut value = sysroot.join("lib").into_os_string();
    for triple in rustlib_targets(sysroot)? {
        value.push(&separator);
        value.push(triple.into_os_string());
    }
    if let Some(existing) = crate::vars::var(base, name).filter(|existing| !existing.is_empty()) {
        value.push(&separator);
        value.push(existing);
    }
    Ok((OsString::from(name), value))
}

/// Every `lib/rustlib/<triple>/lib` the toolchain holds, which is where the target's own `libstd` is.
fn rustlib_targets(sysroot: &Path) -> std::io::Result<Vec<PathBuf>> {
    let root = sysroot.join("lib").join("rustlib");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_file() {
            continue;
        }
        if !kind.is_dir() {
            return Err(std::io::Error::other(format!(
                "{} is neither a regular rustlib manifest nor a target directory",
                entry.path().display()
            )));
        }
        let directory = entry.path();
        for child in std::fs::read_dir(&directory)? {
            let child = child?;
            if child.file_name() != "lib" {
                continue;
            }
            if !child.file_type()?.is_dir() {
                return Err(std::io::Error::other(format!(
                    "{} is not a regular target library directory",
                    child.path().display()
                )));
            }
            found.push(child.path());
        }
    }
    found.sort();
    Ok(found)
}

/// One execution to make.
#[derive(Debug, Clone)]
pub struct ExecRequest<'a> {
    target: &'a TestTarget,
    tests: Vec<String>,
    args: Vec<String>,
    timeout: Option<Duration>,
    scratch: Option<PathBuf>,
    /// Whether the process starts in its scratch directory rather than in the one cargo would give it.
    scratch_cwd: bool,
}

impl<'a> ExecRequest<'a> {
    /// Runs every test of `target`.
    #[must_use]
    pub const fn new(target: &'a TestTarget) -> Self {
        Self {
            target,
            tests: Vec::new(),
            args: Vec::new(),
            timeout: None,
            scratch: None,
            scratch_cwd: false,
        }
    }

    /// Runs exactly the named test.
    #[must_use]
    pub fn with_test(mut self, test: impl Into<String>) -> Self {
        self.tests = vec![test.into()];
        self
    }

    /// Runs exactly the named tests, which one process does in one go.
    #[must_use]
    pub fn with_tests(mut self, tests: impl IntoIterator<Item = String>) -> Self {
        self.tests = tests.into_iter().collect();
        self
    }

    /// The tests this runs, or nothing when it runs every one of the target's.
    #[must_use]
    pub fn tests(&self) -> &[String] {
        &self.tests
    }

    /// Passes further arguments to the harness.
    #[must_use]
    pub fn with_args(mut self, args: impl IntoIterator<Item = String>) -> Self {
        self.args = args.into_iter().collect();
        self
    }

    /// Bounds the run.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Points the process's temporary directory at a directory of its own.
    #[must_use]
    pub fn with_scratch(mut self, scratch: impl Into<PathBuf>) -> Self {
        self.scratch = Some(scratch.into());
        self
    }

    /// Starts the process in its scratch directory rather than where cargo would.
    #[must_use]
    pub const fn in_scratch(mut self, within: bool) -> Self {
        self.scratch_cwd = within;
        self
    }

    /// The target this runs.
    #[must_use]
    pub const fn target(&self) -> &TestTarget {
        self.target
    }

    /// The command line the binary receives.
    /// Every named test is passed as a filter with `--exact`, so a name that is a prefix of another cannot drag it in.
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let mut argv = vec![self.target.executable.clone().into_os_string()];
        if !self.target.through.is_empty() {
            argv.extend(self.target.through.iter().cloned());
            argv.push(OsString::from("--"));
        }
        argv.extend(self.tests.iter().map(OsString::from));
        if !self.tests.is_empty() && self.target.through.is_empty() {
            argv.push(OsString::from("--exact"));
        }
        argv.extend(self.args.iter().map(OsString::from));
        argv
    }
}

/// What a test process runs with: the environment the workspace was opened with, and the mutant to activate (its identity and the catalog it came from), or `None` for the instrumented baseline.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The environment the workspace was opened with.
    pub base_env: &'a [(OsString, OsString)],
    /// The cargo that built the tree, which cargo itself puts in `CARGO` for every process it runs.
    pub cargo: Option<&'a Path>,
    /// The toolchain directory a dynamically linked test binary finds `libstd` under.
    /// `None` starts it with whatever the environment already said.
    pub sysroot: Option<&'a Path>,
    /// The mutant to activate: `(identity, catalog digest)`.
    pub active: Option<(&'a str, &'a str)>,
    /// How many instrumented workspace boundaries the process may cross after the selected guard activates before it is stopped.
    /// `None` counts nothing.
    ///
    /// A mutant that does not terminate has to be stopped by something, and a count is a number every machine agrees on where a clock is not.
    /// It is spent per process because the process is what a run activates a mutant in and what it would otherwise kill by that clock.
    pub steps: Option<u64>,
    /// Where the guards append which of the process's threads reached them, and the catalog the record is about.
    /// `None` runs a process whose guards record nothing.
    pub touch: Option<Touching<'a>>,
    /// Where a coverage-instrumented process writes what it executed.
    /// `None` runs a process that measures nothing.
    pub profile: Option<&'a Path>,
}

/// Where the guards of one process append what they reached, and the catalog the record is about.
#[derive(Debug, Clone, Copy)]
pub struct Touching<'a> {
    /// The file to append to.
    pub log: &'a Path,
    /// The catalog every guard that may write to it was generated from.
    pub catalog: &'a str,
}

/// The one fact a mutant execution established.
///
/// A finite step boundary carries its verified notice in the same variant.
/// There is no representation for `StepLimitReached` without evidence, or for evidence attached to any other outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutantConclusion {
    /// The execution did not run to an answer.
    NotRun,
    /// A selected test failed.
    Killed,
    /// Every selected test passed.
    Survived,
    /// The runtime crossed the configured finite guard-take boundary.
    StepLimitReached {
        /// The notice structurally verified and correlated to this execution.
        notice: StepLimitNotice,
    },
    /// The wall-clock bound expired and reproduced.
    Waited,
    /// The execution did not establish either side of the question.
    Inconclusive,
    /// The execution apparatus failed.
    Errored,
}

impl MutantConclusion {
    /// The public outcome vocabulary used by aggregation and reports.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        match self {
            Self::NotRun => Outcome::NotRun,
            Self::Killed => Outcome::Killed,
            Self::Survived => Outcome::Survived,
            Self::StepLimitReached { .. } => Outcome::StepLimitReached,
            Self::Waited => Outcome::Waited,
            Self::Inconclusive => Outcome::Inconclusive,
            Self::Errored => Outcome::Errored,
        }
    }

    /// The verified step notice, exactly when this is a finite boundary.
    #[must_use]
    pub const fn step_notice(&self) -> Option<&StepLimitNotice> {
        match self {
            Self::StepLimitReached { notice } => Some(notice),
            Self::NotRun
            | Self::Killed
            | Self::Survived
            | Self::Waited
            | Self::Inconclusive
            | Self::Errored => None,
        }
    }

    fn of(observed: &Observation, summary: Option<Summary>, harness: bool) -> Self {
        if let Stopped::StepLimitReached { notice } = &observed.stopped {
            return Self::StepLimitReached {
                notice: notice.clone(),
            };
        }
        match outcome_of(observed, summary, harness) {
            Outcome::NotRun => Self::NotRun,
            Outcome::Killed => Self::Killed,
            Outcome::Survived => Self::Survived,
            Outcome::Waited => Self::Waited,
            Outcome::Inconclusive => Self::Inconclusive,
            Outcome::Errored | Outcome::StepLimitReached => Self::Errored,
        }
    }

    fn reconciled(self, outcome: Outcome) -> Self {
        match outcome {
            Outcome::NotRun => Self::NotRun,
            Outcome::Killed => Self::Killed,
            Outcome::Survived => Self::Survived,
            Outcome::StepLimitReached => match self {
                Self::StepLimitReached { notice } => Self::StepLimitReached { notice },
                Self::NotRun
                | Self::Killed
                | Self::Survived
                | Self::Waited
                | Self::Inconclusive
                | Self::Errored => Self::Errored,
            },
            Outcome::Waited => Self::Waited,
            Outcome::Inconclusive => Self::Inconclusive,
            Outcome::Errored => Self::Errored,
        }
    }
}

/// What one mutant execution established and the observations around it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MutantResult {
    /// The single, evidence-carrying conclusion.
    pub conclusion: MutantConclusion,
    /// The target that ran.
    pub target: String,
    /// The exit status, or [`EXIT_CODE_UNAVAILABLE`].
    pub exit_code: i32,
    /// How long it took, supervision included.
    pub duration: Duration,
    /// The tail of the combined output.
    pub output: Vec<u8>,
    /// Which protocol the target answered in, which decides whether a summary could be held to anything.
    pub protocol: Protocol,
    /// The harness's summary line, when it printed one.
    pub summary: Option<Summary>,
    /// The signal the process died from, on the platforms that have them.
    pub signal: Option<i32>,
    /// Every test that failed, by name, which is what a report hands a person reading a kill.
    pub failed_tests: Vec<String>,
    /// Every test that passed, by name, which is every test that could have noticed the mutation and did not.
    pub passed_tests: Vec<String>,
    /// Every test the harness was told to skip.
    pub ignored_tests: Vec<String>,
}

/// The protocol a test process answered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// libtest, which names every test's result and closes with a summary counting them.
    Libtest,
    /// A harness that answers by its exit code and names no test.
    Custom,
    /// No process answered, so no protocol was spoken.
    Unanswered,
}

/// Whether the tests a run was read as passing are the harness's answer rather than the parser's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// libtest's named passing tests come to its own summary's count.
    Whole,
    /// libtest's named passing tests do not come to its summary's count, or it printed no summary: a line the suite wrote past the capture read as a result, or split one.
    Short,
    /// The protocol names no tests and prints no summary, so there is nothing to fall short of.
    Unspoken,
}

impl MutantResult {
    /// How many tests ran, when the summary said: read from the summary itself, so there is no second copy to disagree with it.
    #[must_use]
    pub fn tests_run(&self) -> Option<u32> {
        self.summary.and_then(|summary| summary.tests_run())
    }

    /// Whether the tests this run was read as passing are the harness's answer, asked of the protocol it answered in, so a harness with no summary is never held to one.
    #[must_use]
    pub fn reading(&self) -> Reading {
        match self.protocol {
            Protocol::Libtest => {
                let whole = self.tests_run().is_some_and(|ran| {
                    usize::try_from(ran).is_ok_and(|ran| ran == self.passed_tests.len())
                });
                if whole {
                    Reading::Whole
                } else {
                    Reading::Short
                }
            }
            Protocol::Custom | Protocol::Unanswered => Reading::Unspoken,
        }
    }

    /// An execution-shaped apparatus failure produced before a child can answer.
    pub(crate) fn apparatus_error(target: &str, message: String) -> Self {
        Self {
            conclusion: MutantConclusion::Errored,
            target: target.to_owned(),
            exit_code: EXIT_CODE_UNAVAILABLE,
            duration: Duration::ZERO,
            output: message.into_bytes(),
            protocol: Protocol::Unanswered,
            summary: None,
            signal: None,
            failed_tests: Vec::new(),
            passed_tests: Vec::new(),
            ignored_tests: Vec::new(),
        }
    }

    /// The conclusion projected into the public outcome vocabulary.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        self.conclusion.outcome()
    }

    /// The verified finite-boundary notice, exactly when the conclusion is one.
    #[must_use]
    pub const fn step_notice(&self) -> Option<&StepLimitNotice> {
        self.conclusion.step_notice()
    }

    /// Reconciles an isolated retry without allowing it to detach step evidence.
    pub(crate) fn reconcile_outcome(&mut self, outcome: Outcome) {
        self.conclusion = self.conclusion.clone().reconciled(outcome);
    }
}

/// Whether the target said anything about the mutation, which is what decides whether the next target is asked.
#[must_use]
pub const fn answered(outcome: Outcome) -> bool {
    !matches!(outcome, Outcome::Inconclusive | Outcome::StepLimitReached)
}

/// The bound one execution runs under: a quiet window under a ceiling where it counts its steps, and the bound it was given where it does not.
fn watched(timeout: Option<Duration>, step: Option<&ExpectedStep>) -> (Bound, Option<Progress>) {
    match (timeout, step) {
        (Some(quiet), Some(step)) => (
            Bound::After(quiet.saturating_mul(QUIET_WINDOWS_PER_CEILING)),
            Some(Progress {
                path: step.state_path.clone(),
                quiet,
            }),
        ),
        (Some(bound), None) => (Bound::After(bound), None),
        (None, _) => (Bound::Unbounded, None),
    }
}

/// Runs one test process and reads what it means.
#[must_use]
pub fn exec(
    request: &ExecRequest<'_>,
    context: &Context<'_>,
    cancel: &Cancel,
    trace: &Recorder,
) -> MutantResult {
    let target = request.target;
    let step = match ExpectedStep::new(context, request.scratch.as_deref()) {
        Ok(step) => step,
        Err(error) => {
            let message = error.to_string();
            trace.note("step-protocol", &message);
            return MutantResult::apparatus_error(&target.id, message);
        }
    };
    let (bound, progress) = watched(request.timeout, step.as_ref());
    let mut spec = Spec::new(request.argv(), bound);
    spec.progress = progress;
    spec.dir = Some(match (&request.scratch, request.scratch_cwd) {
        (Some(scratch), true) => scratch.clone(),
        _ => target.cwd.clone(),
    });
    let mut env = match environment(context, target, request.scratch.as_deref()) {
        Ok(env) => env,
        Err(error) => {
            let message = format!("the toolchain environment could not be inspected: {error}");
            trace.note("execution-environment", &message);
            return MutantResult::apparatus_error(&target.id, message);
        }
    };
    if let Some(step) = &step {
        step.add_environment(&mut env);
        spec.stop_file = Some(step.path.clone());
    }
    spec.env = Some(env);
    let result = run(&spec, cancel);
    let observation = Observation::of(&result, step.as_ref());
    let record = ExecRecord::of(&spec, &result).map(|mut record| {
        record.stopped.clone_from(&observation.stopped);
        record
    });
    trace.exec_result(record);
    let (summary, lines, protocol_exact) =
        match (target.harness, std::str::from_utf8(&result.output)) {
            (true, Ok(text)) => (parse_summary_text(text), parse_lines_text(text), true),
            (true, Err(_not_utf8)) => (None, Lines::default(), false),
            (false, _) => (None, Lines::default(), true),
        };
    let signal = result.signal();
    MutantResult {
        conclusion: if protocol_exact {
            MutantConclusion::of(&observation, summary, target.harness)
        } else {
            MutantConclusion::Errored
        },
        target: target.id.clone(),
        exit_code: result.conventional_exit_code(),
        duration: result.duration,
        output: result.output,
        protocol: if target.harness {
            Protocol::Libtest
        } else {
            Protocol::Custom
        },
        summary,
        signal,
        failed_tests: lines.failed,
        passed_tests: lines.passed,
        ignored_tests: lines.ignored,
    }
}

/// Configures [`build`].
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// `--target-dir`.
    pub target_dir: Option<PathBuf>,
    /// Pass `--locked`.
    pub locked: bool,
    /// Pass `--offline`.
    pub offline: bool,
    /// The member packages whose test binaries are wanted.
    /// Empty is the whole workspace.
    pub packages: Vec<String>,
    /// What the project is compiled as: its features, target, profile, and how many jobs cargo may use.
    pub build: crate::cargo::BuildConfig,
}

/// Builds the test binaries of a tree and reports them.
///
/// # Errors
/// Whatever stopped cargo from building, and a message stream that could not be read.
pub fn build(
    driver: &Driver<'_>,
    packages: &[Package],
    options: &BuildOptions,
) -> Result<Vec<TestTarget>, CargoError> {
    let compiled = compile(
        driver,
        &CompileOptions {
            kind: CompileKind::Tests,
            packages: options.packages.clone(),
            target_dir: options.target_dir.clone(),
            locked: options.locked,
            offline: options.offline,
            timeout: None,
            env: Vec::new(),
            build: options.build.clone(),
        },
    )?;
    if !compiled.success {
        return Err(CargoError::new(
            CargoErrorKind::CommandFailed,
            "the test binaries could not be built",
        ));
    }
    targets_of(&compiled.messages, packages, options.target_dir.as_deref())
}

/// The targets a run may start, which is every one `skipped` does not name.
///
/// Two paths build a target list from the same build messages: the one that prepares the mutants, and the coverage reachability measurement that runs before it.
/// Only the first applied this, so a target named in `[execution] skip_targets` was started once, under coverage, against a page that promises it is never started.
/// The rule lives here so a third path cannot be written without it.
#[must_use]
pub fn startable(targets: &[TestTarget], skipped: &[String]) -> Vec<TestTarget> {
    targets
        .iter()
        .filter(|target| !skipped.iter().any(|one| one == &target.id))
        .cloned()
        .collect()
}

/// The test binaries a build produced, in target id order.
///
/// # Errors
/// Refuses a cargo message stream that names two different executable paths for the same package target.
pub fn targets_of(
    messages: &[Message],
    packages: &[Package],
    target_dir: Option<&Path>,
) -> Result<Vec<TestTarget>, CargoError> {
    let binaries = binaries_built(messages)?;
    let mut harnesses: BTreeMap<String, BTreeMap<(String, String), bool>> = BTreeMap::new();
    let mut targets = Vec::new();
    for message in messages {
        let Message::CompilerArtifact(artifact) = message else {
            continue;
        };
        let Some(executable) = &artifact.executable else {
            continue;
        };
        if !artifact.profile.test {
            continue;
        }
        let Some(kind) = TargetKind::of(&artifact.target) else {
            continue;
        };
        let Some(package) = packages
            .iter()
            .find(|package| package.id == artifact.package_id)
        else {
            continue;
        };
        let empty_binaries = BTreeMap::new();
        let package_binaries = match binaries.get(&artifact.package_id) {
            Some(binaries) => binaries,
            None => &empty_binaries,
        };
        let mut env = cargo_environment(package, kind, target_dir, package_binaries);
        env.extend(built_by_a_script(messages, &artifact.package_id));
        let held = match harnesses.entry(package.id.clone()) {
            std::collections::btree_map::Entry::Occupied(held) => held.into_mut(),
            std::collections::btree_map::Entry::Vacant(empty) => {
                empty.insert(crate::cargo::manifest::harnesses(&package.manifest_path)?)
            }
        };
        let harness = held
            .get(&(kind.name().to_owned(), artifact.target.name.clone()))
            .copied();
        let harness = match harness {
            Some(harness) => harness,
            None => true,
        };
        targets.push(
            TestTarget::new(
                package.name.clone(),
                kind,
                artifact.target.name.clone(),
                executable.clone(),
                package.manifest_dir().to_path_buf(),
            )
            .with_harness(harness)
            .with_limitations(if harness {
                Vec::new()
            } else {
                vec![crate::limitation::CUSTOM_HARNESS.to_owned()]
            })
            .with_cargo_env(env),
        );
    }
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    targets.dedup_by(|a, b| a.id == b.id);
    Ok(targets)
}

/// Every test target the members declare, as ids, without building one of them.
#[must_use]
pub fn declared_targets(members: &[&Package]) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    for package in members {
        for target in &package.targets {
            if let Some(kind) = TargetKind::of(target) {
                ids.extend(std::iter::once(target_id(
                    &package.name,
                    kind,
                    &target.name,
                )));
            }
            if target.is_lib() && !target.is_proc_macro() && target.doctest {
                ids.extend(std::iter::once(target_id(
                    &package.name,
                    TargetKind::Doc,
                    &target.name,
                )));
            }
        }
    }
    ids
}

/// One target for each library whose documentation cargo would run, which is a target this engine does not start itself.
#[must_use]
pub fn documentation_targets(
    members: &[&Package],
    cargo: &Path,
    arguments: &[OsString],
) -> Vec<TestTarget> {
    let mut targets = Vec::new();
    for package in members {
        for target in &package.targets {
            if !target.is_lib() || target.is_proc_macro() || !target.doctest {
                continue;
            }
            let mut through = vec![OsString::from("test"), OsString::from("--doc")];
            through.extend(arguments.iter().cloned());
            through.push(OsString::from("--package"));
            through.push(OsString::from(&package.name));
            targets.push(
                TestTarget::new(
                    package.name.clone(),
                    TargetKind::Doc,
                    target.name.clone(),
                    cargo.to_path_buf(),
                    package.manifest_dir().to_path_buf(),
                )
                .with_through(through)
                .with_limitations(vec![crate::limitation::DOCTESTS_ROUTED_BY_FILE.to_owned()]),
            );
        }
    }
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    targets.dedup_by(|a, b| a.id == b.id);
    targets
}

/// What cargo sets for a test process, reproduced from the metadata.
fn cargo_environment(
    package: &Package,
    kind: TargetKind,
    target_dir: Option<&Path>,
    binaries: &BTreeMap<String, PathBuf>,
) -> Vec<(OsString, OsString)> {
    let mut env = package_environment(package);
    if matches!(kind, TargetKind::Test | TargetKind::Example) {
        if let Some(target_dir) = target_dir {
            env.push((
                OsString::from("CARGO_TARGET_TMPDIR"),
                target_dir.join("tmp").into_os_string(),
            ));
        }
        for target in &package.targets {
            if !target.is_bin() {
                continue;
            }
            if let Some(built) = binaries.get(&target.name) {
                env.push((
                    OsString::from(format!("CARGO_BIN_EXE_{}", target.name)),
                    built.as_os_str().to_owned(),
                ));
            }
        }
    }
    env
}

/// What cargo tells every unit of a package about the package.
#[must_use]
pub fn package_environment(package: &Package) -> Vec<(OsString, OsString)> {
    let (major, minor, patch, pre) = version_parts(&package.version);
    let said = |value: Option<&str>| OsString::from(value.unwrap_or_default());
    let named =
        |value: Option<&Path>| value.map_or_else(OsString::new, |one| one.as_os_str().to_owned());
    vec![
        (
            OsString::from("CARGO_MANIFEST_DIR"),
            package.manifest_dir().as_os_str().to_owned(),
        ),
        (
            OsString::from("CARGO_MANIFEST_PATH"),
            package.manifest_path.as_os_str().to_owned(),
        ),
        (
            OsString::from("CARGO_PKG_NAME"),
            OsString::from(&package.name),
        ),
        (
            OsString::from("CARGO_PKG_VERSION"),
            OsString::from(&package.version),
        ),
        (
            OsString::from("CARGO_PKG_VERSION_MAJOR"),
            OsString::from(major),
        ),
        (
            OsString::from("CARGO_PKG_VERSION_MINOR"),
            OsString::from(minor),
        ),
        (
            OsString::from("CARGO_PKG_VERSION_PATCH"),
            OsString::from(patch),
        ),
        (OsString::from("CARGO_PKG_VERSION_PRE"), OsString::from(pre)),
        (
            OsString::from("CARGO_PKG_AUTHORS"),
            OsString::from(package.authors.join(":")),
        ),
        (
            OsString::from("CARGO_PKG_DESCRIPTION"),
            said(package.description.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_HOMEPAGE"),
            said(package.homepage.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_REPOSITORY"),
            said(package.repository.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_LICENSE"),
            said(package.license.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_LICENSE_FILE"),
            named(package.license_file.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_RUST_VERSION"),
            said(package.rust_version.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_README"),
            named(package.readme.as_deref()),
        ),
    ]
}

/// A semantic version cut the way cargo cuts it: three numbers and whatever follows the first hyphen.
fn version_parts(version: &str) -> (&str, &str, &str, &str) {
    let (numbers, pre) = version.split_once('-').unwrap_or((version, ""));
    let numbers = match numbers.split_once('+') {
        Some((without_build, _)) => without_build,
        None => numbers,
    };
    let mut parts = numbers.split('.');
    (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        pre,
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;
    use std::time::Duration;

    use njutest_devkit::result::{
        OptionState::Present,
        ResultState::{Refused, Returned},
        option_state, result_state,
    };

    use super::{
        Context, ExpectedStep, NoticeError, Observation, QUIET_WINDOWS_PER_CEILING, STEP_STATE_ENV,
        STEP_STATE_SCHEMA, StepBoundaryScope, StepLimitNotice, StepProtocolFailure, StepSetupError,
        Stopped, observed_stop, outcome_of, rustlib_targets, watched,
    };
    use crate::outcome::Outcome;
    use crate::runner::{Bound, ProcessExit, RunResult, Termination};

    const CATALOG_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const CATALOG_B: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const MUTANT_A: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const MUTANT_B: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

    macro_rules! returned {
        ($expression:expr, $context:expr) => {{
            let result = $expression;
            assert_eq!(result_state(&result), Returned, "{}: {result:?}", $context);
            let Ok(value) = result else { return };
            value
        }};
    }

    macro_rules! present {
        ($expression:expr, $context:expr) => {{
            let optional = $expression;
            assert_eq!(option_state(optional.as_ref()), Present, "{}", $context);
            let Some(value) = optional else { return };
            value
        }};
    }

    fn expected(directory: &Path) -> ExpectedStep {
        let mut bytes = [0u8; 16];
        let filled = getrandom::fill(&mut bytes);
        assert_eq!(result_state(&filled), Returned, "a fresh nonce: {filled:?}");
        ExpectedStep {
            path: directory.join("step.notice"),
            state_path: directory.join("step.state"),
            nonce: hex::encode(bytes),
            catalog: CATALOG_A.to_owned(),
            mutant: MUTANT_A.to_owned(),
            limit: 10,
        }
    }

    fn result(termination: Termination) -> RunResult {
        RunResult {
            termination,
            duration: Duration::ZERO,
            output: Vec::new(),
            stdout: Vec::new(),
            stdout_truncated: false,
        }
    }

    #[test]
    fn rustlib_target_discovery_distinguishes_support_directories_from_targets() {
        let sysroot = returned!(tempfile::tempdir(), "sysroot");
        let rustlib = sysroot.path().join("lib/rustlib");
        let target = rustlib.join("aarch64-example-none/lib");
        let support = rustlib.join("etc");
        let created = std::fs::create_dir_all(&target);
        assert_eq!(result_state(&created), Returned, "target: {created:?}");
        let created = std::fs::create_dir_all(&support);
        assert_eq!(result_state(&created), Returned, "support: {created:?}");
        let written = std::fs::write(support.join("debugger.py"), b"support");
        assert_eq!(
            result_state(&written),
            Returned,
            "support file: {written:?}"
        );

        let targets = returned!(rustlib_targets(sysroot.path()), "discover targets");
        assert_eq!(targets, [target]);
    }

    fn publish(step: &ExpectedStep, record: &str) {
        let written = std::fs::write(&step.path, record);
        assert_eq!(
            result_state(&written),
            Returned,
            "write notice: {written:?}"
        );
    }

    fn assert_absent(path: &Path) {
        let error = std::fs::symlink_metadata(path);
        assert_eq!(
            result_state(&error),
            Refused,
            "path must be absent: {error:?}"
        );
        let Err(error) = error else { return };
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    fn notice_record(header: (&str, &str), binding: (&str, &str), boundary: (u64, u64)) -> String {
        let (schema, nonce) = header;
        let (catalog, mutant) = binding;
        let (limit, observed) = boundary;
        format!("{schema}\t{nonce}\t{catalog}\t{mutant}\t{limit}\t{observed}\n")
    }

    #[test]
    fn the_platform_counter_must_have_a_value_past_the_allowance() {
        let Ok(limit) = u64::try_from(usize::MAX) else {
            return;
        };
        let context = Context {
            base_env: &[],
            cargo: None,
            sysroot: None,
            active: Some((MUTANT_A, CATALOG_A)),
            steps: Some(limit),
            touch: None,
            profile: None,
        };

        assert!(matches!(
            ExpectedStep::new(&context, None),
            Err(StepSetupError::BoundaryOverflow { limit: refused }) if refused == limit
        ));
    }

    #[test]
    fn step_setup_creates_one_exact_private_state_and_clear_removes_it() {
        let scratch = returned!(tempfile::tempdir(), "scratch");
        let context = Context {
            base_env: &[],
            cargo: None,
            sysroot: None,
            active: Some((MUTANT_A, CATALOG_A)),
            steps: Some(10),
            touch: None,
            profile: None,
        };
        let step = returned!(ExpectedStep::new(&context, Some(scratch.path())), "setup");
        let step = present!(step, "bounded execution");
        let state = returned!(std::fs::read_to_string(&step.state_path), "state");
        assert_eq!(
            state,
            format!(
                "{STEP_STATE_SCHEMA}\t{}\t{CATALOG_A}\t{MUTANT_A}\t10\tdormant\t0\n",
                step.nonce
            )
        );
        let mut environment = Vec::new();
        step.add_environment(&mut environment);
        assert!(environment.contains(&(
            OsString::from(STEP_STATE_ENV),
            step.state_path.as_os_str().to_owned()
        )));
        let cleared = step.clear();
        assert_eq!(result_state(&cleared), Returned, "clear: {cleared:?}");
        assert_absent(&step.path);
        assert_absent(&step.path.with_extension("notice.partial"));
        assert_absent(&step.state_path);
    }

    #[test]
    fn deserialization_cannot_construct_an_invalid_step_notice() {
        let valid = format!(
            r#"{{"nonce":"00000000000000000000000000000001","catalog":"{CATALOG_A}","mutant":"{MUTANT_A}","limit":10,"observed":11}}"#
        );
        let notice: StepLimitNotice =
            returned!(crate::strictjson::decode_str(&valid), "valid notice");
        assert_eq!((notice.limit(), notice.observed()), (10, 11));
        assert_eq!(
            notice.scope(),
            StepBoundaryScope::InstrumentedWorkspaceSource
        );

        for invalid in [
            format!(
                r#"{{"nonce":"not-a-nonce","catalog":"{CATALOG_A}","mutant":"{MUTANT_A}","limit":10,"observed":11}}"#
            ),
            format!(
                r#"{{"nonce":"00000000000000000000000000000001","catalog":"short","mutant":"{MUTANT_A}","limit":10,"observed":11}}"#
            ),
            format!(
                r#"{{"nonce":"00000000000000000000000000000001","catalog":"{CATALOG_A}","mutant":"short","limit":10,"observed":11}}"#
            ),
            format!(
                r#"{{"nonce":"00000000000000000000000000000001","catalog":"{CATALOG_A}","mutant":"{MUTANT_A}","limit":0,"observed":1}}"#
            ),
            format!(
                r#"{{"nonce":"00000000000000000000000000000001","catalog":"{CATALOG_A}","mutant":"{MUTANT_A}","limit":10,"observed":12}}"#
            ),
            format!(
                r#"{{"nonce":"00000000000000000000000000000001","catalog":"{CATALOG_A}","mutant":"{MUTANT_A}","limit":10,"observed":11,"extra":true}}"#
            ),
        ] {
            let decoded = crate::strictjson::decode_str::<StepLimitNotice>(&invalid);
            assert_eq!(
                result_state(&decoded),
                Refused,
                "accepted {invalid}: {decoded:?}"
            );
        }
    }

    #[test]
    fn a_notice_is_accepted_only_for_the_exact_execution_and_boundary() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        publish(
            &step,
            &notice_record(
                ("rust-mutants-step-notice-v1", step.nonce.as_str()),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
        );

        let notice = returned!(step.read(), "valid protocol");
        let notice = present!(notice, "one notice");
        assert_eq!(notice.nonce(), step.nonce);
        assert_eq!(notice.catalog(), step.catalog);
        assert_eq!(notice.mutant(), step.mutant);
        assert_eq!((notice.limit(), notice.observed()), (10, 11));
    }

    #[test]
    fn stale_malformed_or_mismatched_notices_fail_closed() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        for record in invalid_notice_records(&step.nonce) {
            publish(&step, &record);
            let read = step.read();
            assert_eq!(
                result_state(&read),
                Refused,
                "accepted {record:?}: {read:?}"
            );
        }
    }

    fn invalid_notice_records(nonce: &str) -> Vec<String> {
        let canonical = notice_record(
            ("rust-mutants-step-notice-v1", nonce),
            (CATALOG_A, MUTANT_A),
            (10, 11),
        );
        vec![
            String::new(),
            canonical.trim_end_matches('\n').to_owned(),
            canonical.replace('\n', "\r\n"),
            canonical.replace("\t10\t11\n", "\t010\t11\n"),
            canonical.replace("\t10\t11\n", "\t10\t+11\n"),
            canonical.replace("\t10\t11\n", "\t10\t 11\n"),
            notice_record(
                ("rust-mutants-step-notice-v0", nonce),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
            notice_record(
                (
                    "rust-mutants-step-notice-v1",
                    "ffffffffffffffffffffffffffffffff",
                ),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
            notice_record(
                ("rust-mutants-step-notice-v1", nonce),
                (CATALOG_B, MUTANT_A),
                (10, 11),
            ),
            notice_record(
                ("rust-mutants-step-notice-v1", nonce),
                (CATALOG_A, MUTANT_B),
                (10, 11),
            ),
            notice_record(
                ("rust-mutants-step-notice-v1", nonce),
                (CATALOG_A, MUTANT_A),
                (9, 10),
            ),
            notice_record(
                ("rust-mutants-step-notice-v1", nonce),
                (CATALOG_A, MUTANT_A),
                (10, 12),
            ),
            format!("{canonical}extra\n"),
        ]
    }

    #[test]
    fn a_notice_must_be_a_small_regular_file() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let created = std::fs::create_dir_all(&step.path);
        assert_eq!(
            result_state(&created),
            Returned,
            "notice directory: {created:?}"
        );
        assert!(matches!(
            step.read(),
            Err(NoticeError::Open { .. } | NoticeError::NotRegular { .. })
        ));
        let removed = std::fs::remove_dir(&step.path);
        assert_eq!(
            result_state(&removed),
            Returned,
            "remove notice directory: {removed:?}"
        );

        let written = std::fs::write(&step.path, vec![b'x'; 16 * 1024 + 1]);
        assert_eq!(
            result_state(&written),
            Returned,
            "oversized notice: {written:?}"
        );
        assert!(matches!(step.read(), Err(NoticeError::TooLarge { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn a_notice_symlink_is_never_followed() {
        use std::os::unix::fs::symlink;

        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let elsewhere = directory.path().join("elsewhere");
        let written = std::fs::write(
            &elsewhere,
            notice_record(
                ("rust-mutants-step-notice-v1", step.nonce.as_str()),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
        );
        assert_eq!(
            result_state(&written),
            Returned,
            "symlink target: {written:?}"
        );
        let linked = symlink(&elsewhere, &step.path);
        assert_eq!(
            result_state(&linked),
            Returned,
            "notice symlink: {linked:?}"
        );

        assert!(matches!(
            step.read(),
            Err(NoticeError::Open { .. } | NoticeError::NotRegular { .. })
        ));
    }

    #[test]
    fn cleanup_failure_is_a_protocol_failure_not_a_silent_success() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        publish(
            &step,
            &notice_record(
                ("rust-mutants-step-notice-v1", step.nonce.as_str()),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
        );
        let created = std::fs::create_dir_all(step.path.with_extension("notice.partial"));
        assert_eq!(
            result_state(&created),
            Returned,
            "partial directory: {created:?}"
        );

        assert!(matches!(
            observed_stop(&result(Termination::StoppedByMonitor), Some(&step)),
            Stopped::StepProtocolFailed {
                reason: StepProtocolFailure::Cleanup { .. }
            }
        ));
    }

    #[test]
    fn tagged_termination_types_reject_extra_fields() {
        let stopped =
            crate::strictjson::decode_str::<Stopped>(r#"{"kind":"timed-out","extra":true}"#);
        assert_eq!(result_state(&stopped), Refused, "stopped: {stopped:?}");
        let exited = crate::strictjson::decode_str::<ProcessExit>(
            r#"{"kind":"code","value":0,"extra":true}"#,
        );
        assert_eq!(result_state(&exited), Refused, "exit: {exited:?}");
    }

    #[test]
    fn only_a_verified_notice_can_turn_a_monitor_stop_into_a_step_fact() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let stopped = result(Termination::StoppedByMonitor);

        assert!(matches!(
            observed_stop(&stopped, Some(&step)),
            Stopped::StepProtocolFailed {
                reason: StepProtocolFailure::NoticeMissing {}
            }
        ));
        publish(&step, "not a notice\n");
        assert!(matches!(
            observed_stop(&stopped, Some(&step)),
            Stopped::StepProtocolFailed {
                reason: StepProtocolFailure::ExecutionMismatch {}
            }
        ));
        publish(
            &step,
            &notice_record(
                ("rust-mutants-step-notice-v1", step.nonce.as_str()),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
        );
        assert!(matches!(
            observed_stop(&stopped, Some(&step)),
            Stopped::StepLimitReached { notice }
                if notice.limit() == 10 && notice.observed() == 11
        ));
        assert_absent(&step.path);
    }

    #[test]
    fn a_complete_notice_wins_over_a_simultaneous_wall_clock_deadline() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        publish(
            &step,
            &notice_record(
                ("rust-mutants-step-notice-v1", step.nonce.as_str()),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
        );

        assert!(matches!(
            observed_stop(&result(Termination::TimedOut), Some(&step)),
            Stopped::StepLimitReached { notice }
                if notice.limit() == 10 && notice.observed() == 11
        ));
        assert_absent(&step.path);
    }

    #[test]
    fn an_unaccompanied_wall_clock_deadline_remains_waited() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());

        assert_eq!(
            observed_stop(&result(Termination::TimedOut), Some(&step)),
            Stopped::TimedOut { raised: None }
        );
    }

    #[test]
    fn a_wall_clock_deadline_says_how_far_the_count_had_got() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let written = std::fs::write(
            &step.state_path,
            format!(
                "{STEP_STATE_SCHEMA}\t0000000000000000000000000000000a\t{CATALOG_A}\t{MUTANT_A}\t10\tactive\t7\n"
            ),
        );
        assert_eq!(result_state(&written), Returned, "state: {written:?}");

        assert_eq!(
            observed_stop(&result(Termination::TimedOut), Some(&step)),
            Stopped::TimedOut { raised: Some(7) },
            "the count is read before the state it lives in is cleared away"
        );
        assert_absent(&step.state_path);
    }

    #[test]
    fn a_quiet_window_says_how_far_the_count_had_got_before_it_went_quiet() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let written = std::fs::write(
            &step.state_path,
            format!(
                "{STEP_STATE_SCHEMA}\t0000000000000000000000000000000b\t{CATALOG_A}\t{MUTANT_A}\t10\tactive\t4\n"
            ),
        );
        assert_eq!(result_state(&written), Returned, "state: {written:?}");

        assert_eq!(
            observed_stop(&result(Termination::Stalled), Some(&step)),
            Stopped::Stalled { raised: Some(4) }
        );
        assert_absent(&step.state_path);
    }

    #[test]
    fn only_an_execution_counting_its_steps_is_watched_for_progress() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let second = Duration::from_secs(1);

        let (bound, progress) = watched(Some(second), None);
        assert_eq!(
            bound,
            Bound::After(second),
            "a baseline keeps its bound exactly"
        );
        assert!(progress.is_none(), "and nothing watches it for progress");

        let (bound, progress) = watched(Some(second), Some(&step));
        assert_eq!(bound, Bound::After(second * QUIET_WINDOWS_PER_CEILING));
        assert_eq!(
            progress.map(|progress| (progress.path, progress.quiet)),
            Some((step.state_path.clone(), second)),
            "a counted execution is watched through its step state for a window of its bound"
        );

        let (bound, progress) = watched(None, Some(&step));
        assert_eq!(
            bound,
            Bound::Unbounded,
            "an unbounded execution stays unbounded"
        );
        assert!(progress.is_none());
    }

    #[test]
    fn cancellation_dominates_and_clears_even_a_valid_notice() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        publish(
            &step,
            &notice_record(
                ("rust-mutants-step-notice-v1", step.nonce.as_str()),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
        );
        let cancelled = result(Termination::Cancelled { started: true });

        assert_eq!(
            observed_stop(&cancelled, Some(&step)),
            Stopped::Cancelled { started: true }
        );
        assert_absent(&step.path);
    }

    #[test]
    fn a_notice_cannot_claim_that_a_process_which_never_started_reached_a_step() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        publish(
            &step,
            &notice_record(
                ("rust-mutants-step-notice-v1", step.nonce.as_str()),
                (CATALOG_A, MUTANT_A),
                (10, 11),
            ),
        );
        let unstarted = result(Termination::NotStarted {
            error: crate::runner::RunnerError::SpecInvalid {
                message: "has no argument vector",
            },
        });

        assert_eq!(observed_stop(&unstarted, Some(&step)), Stopped::NotStarted);
        assert_absent(&step.path);
    }

    #[test]
    fn an_unaccompanied_reserved_status_is_an_ordinary_nonzero_exit() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let exited = result(Termination::Exited(ProcessExit::Code(95)));
        let observed = Observation {
            stopped: observed_stop(&exited, Some(&step)),
            stale_catalog: false,
        };

        assert_eq!(
            observed.stopped,
            Stopped::Exited {
                exit: ProcessExit::Code(95)
            }
        );
        assert_eq!(outcome_of(&observed, None, false), Outcome::Killed);
    }

    #[test]
    fn the_runtime_protocol_status_is_special_only_for_a_step_bounded_execution() {
        let directory = returned!(tempfile::tempdir(), "tempdir");
        let step = expected(directory.path());
        let exited = result(Termination::Exited(ProcessExit::Code(
            crate::instrument::STEP_PROTOCOL_EXIT,
        )));

        assert!(matches!(
            observed_stop(&exited, Some(&step)),
            Stopped::StepProtocolFailed {
                reason: StepProtocolFailure::Publication {}
            }
        ));
        assert_eq!(
            observed_stop(&exited, None),
            Stopped::Exited {
                exit: ProcessExit::Code(crate::instrument::STEP_PROTOCOL_EXIT)
            }
        );
    }
}
