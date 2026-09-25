// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A prepared workspace: every accepted mutant instrumented into one build, and the test binaries that build produced.

mod carry;
pub(crate) mod prepare;
mod route;
mod verify;

pub use carry::Tree as CarriedTree;
pub use prepare::{prepare, rewrite_needed};
use prepare::{pristine, selection};
use verify::verify;
pub use verify::{Baseline, BaselineCacheError, Measured, Passing, Verified};

pub use route::{
    Asked, BRANCH_NEVER_TAKEN, Discharge, Fallback, Granularity, NEVER_INFECTED, Proof, Reaches,
    Route, RouteAccountingError, Routing, Timing,
};

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use crate::EngineError;
use crate::catalog::{Catalog, Mutant};
use crate::discover::{self, DiscoverOptions, FileReport, SkipClaim};
use crate::execute::{
    self, Context, ExecRequest, MutantConclusion, MutantResult, Protocol, Reading, TargetKind,
    TestTarget, target_id,
};
use crate::glob::Pattern;
use crate::rule::Tier;
use crate::runner::Cancel;
use crate::snapshot::Drift;
use crate::syntax::{LineIndex, Position, Skip};
use crate::trace::MutantExecRecord;
use crate::validate::{Rejection, Validated};
use crate::workspace::{SessionError, Workspace};

/// Where a mutation is and what it edits, which is how a reviewer names one that outlives an edit elsewhere in the file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locator {
    /// The workspace-relative path with forward slashes.
    pub path: String,
    /// The item, as a reader writes it.
    /// A suffix is enough: `clamp`, `Type::method`, `mod::path::Type::method`.
    pub item: String,
    /// The rule's name.
    pub rule: String,
    /// The bytes the edit replaces, as text.
    pub original: String,
    /// The line, as a hint that separates two mutations the rest would name together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// How many mutations this names, when it names a set of them on one reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

impl Locator {
    /// The locator a reader writes on a command line, or nothing when the text is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (rest, line) = match text.rsplit_once('@') {
            Some((rest, digits)) => match digits.parse::<u32>() {
                Ok(line) => (rest, Some(line)),
                Err(_not_a_line) => return None,
            },
            None => (text, None),
        };
        let (path, rest) = rest.split_once(':')?;
        let (item, rule) = rest.rsplit_once(':')?;
        if path.is_empty() || item.is_empty() || rule.is_empty() {
            return None;
        }
        Some(Self {
            path: path.to_owned(),
            item: item.to_owned(),
            rule: rule.to_owned(),
            original: String::new(),
            line,
            count: None,
        })
    }
}

/// Why a locator named no one mutation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LocateError {
    /// The catalog holds no such mutation.
    #[error("no mutation of this catalog is the one described")]
    Nothing,
    /// The catalog holds more than one, and the line did not separate them.
    #[error("{} mutations of this catalog are the one described; add the line to say which: {}", display_ids.len(), display_ids.join(", "))]
    Several {
        /// What it could have meant.
        display_ids: Vec<String>,
    },
    /// The locator names a set of a stated size, and the catalog holds another number of them.
    #[error("this names {} mutations of the catalog and the claim is written for {wanted}: {}", display_ids.len(), display_ids.join(", "))]
    Counted {
        /// How many the claim was written for.
        wanted: u32,
        /// What the catalog holds instead.
        display_ids: Vec<String>,
    },
}

/// Whether an item path is the one a locator names, which a suffix says.
fn names(item: &str, wanted: &str) -> bool {
    item == wanted || item.ends_with(&format!("::{wanted}"))
}

/// Whether a control records what its guards reached, so a caller that confirms a kill can learn whether the baseline's reach held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Observing {
    /// Record, and compare each whole target's unions with its baseline's.
    Reach,
    /// Record nothing.
    Nothing,
}

/// What a control is started with beyond what the baseline was: variables set over its environment, a program it is started through, and harness arguments after the baseline's.
///
/// Held apart from [`Request`], which every execution takes, so that only a control can be perturbed and no mutant execution is ever run under conditions its baseline was not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Perturbation {
    /// Variables set over the base environment, each replacing one of the same name, from the closed set a control may be perturbed in.
    pub environment: Vec<(execute::Variable, std::ffi::OsString)>,
    /// A program the test binary is started through, which replaces itself with it.
    pub launcher: Option<execute::Launcher>,
    /// How the harness schedules the tests.
    pub schedule: execute::Schedule,
}

impl Perturbation {
    /// Nothing beyond what the baseline was started with.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            environment: Vec::new(),
            launcher: None,
            schedule: execute::Schedule::AsConfigured,
        }
    }
}

/// How one control is run: whether it records what it reached, and what it is started with beyond what its baseline was.
#[derive(Debug, Clone, Copy)]
pub struct Conditions<'a> {
    /// Whether it records, and compares with its baseline.
    pub observing: Observing,
    /// What it is started with beyond what the baseline was.
    pub perturbation: &'a Perturbation,
}

/// What a control came to, and what it established about each whole target's baseline reach where it was asked to record.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Controlled {
    /// What the original code said.
    pub result: MutantResult,
    /// Whether each whole target it recorded reached what its baseline did, in the order they ran.
    pub observed: Vec<Observed>,
}

/// What one control established about one target's baseline reach.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Observed {
    /// The target, by the identity its baseline is recorded under.
    pub target: String,
    /// Whether its reach held.
    pub steadiness: crate::touch::Steadiness,
}

/// The directory beside the copy a run without some of the tree's files keeps them in until it puts them back.
const ASIDE_NAME: &str = "aside";

/// The file a control's guards append what they reached to, in the control's own scratch.
const CONTROL_TOUCH_LOG: &str = "touch.log";

/// The log a mutant execution asked to record what it entered writes to, in its own scratch.
const ENTERED_LOG: &str = "entered.log";

/// One control process's question: which target, asked how, for how long.
struct Once<'a> {
    request: &'a Request,
    target: &'a TestTarget,
    timeout: Duration,
    perturbation: &'a Perturbation,
}

/// What preparing does about a target whose baseline does not pass with nothing active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failing {
    /// End the preparation and name the target.
    /// A person who asked for a measurement wants to hear that there was nothing to measure.
    Refuse,
    /// Report it and leave the target out of every route, so a caller with a verdict of its own can give it.
    Exclude,
}

/// What reach measurement established about one mutation.
///
/// This is deliberately not `Option<bool>`: not measuring a place is a fact distinct from measuring it and observing that no target reached it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Reachability {
    /// Neither coverage nor the instrumented guards measured the place.
    Unmeasured,
    /// At least one measured target reached the place.
    Reached,
    /// Measurement covered the relevant targets and none reached the place.
    Unreached,
}

/// How many times the active mutant's guard may be taken before its process is stopped, when nobody says.
///
/// Fifty million takes of one site is a number a test written by a person does not approach, and every machine agrees on the number itself.
/// What they do not agree on is what it costs: this was sized for the in-memory counter the durable step protocol replaced, where it passed in about a second.
/// Measured since, a take is about half a microsecond on one developer machine with the per-take `fsync` lifted out, so fifty million spend about twenty-five seconds against a thirty-second derived floor -- and were 7.3ms each on Windows before that, where they would have spent days.
/// So the ceiling does not reliably stop an unbounded execution before the clock does, and which of the two answers is a fact about the machine (ADR 0023).
/// Lowering it is what a project does today; taking the clock out of the decision where the count can answer is what would fix it.
/// A person who has a test that really does drive one site that hard raises it, and a run that would rather have only the clock sets it to zero.
pub const DEFAULT_MUTANT_STEPS: u64 = 50_000_000;

/// Configures [`Workspace::prepare`].
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one switch a person sets on the command line or in the configuration file, \
              and a switch is a bool wherever it is stored"
)]
pub struct PrepareOptions {
    /// Which tier of rules to apply when `operators` is empty.
    pub tier: Tier,
    /// Exactly these rules, by name.
    /// Empty means the tier.
    pub operators: Vec<String>,
    /// Start every test process in a directory of its own rather than where cargo would.
    pub scratch_working_directory: bool,
    /// Patterns a file must match to be mutable.
    pub include: Vec<Pattern>,
    /// Patterns that remove a file again.
    pub exclude: Vec<Pattern>,
    /// The member packages to mutate.
    /// Empty means every member.
    pub packages: Vec<String>,
    /// The places a reviewer configured the run to pass over, each with the reason they gave.
    pub skips: Vec<discover::SkipRule>,
    /// Where to remember successful measurements of this exact tree.
    pub measurements: Option<PathBuf>,
    /// Run every test target once with nothing active, and refuse to hand back a session whose instrumented baseline does not pass.
    pub verify: bool,
    /// Ask the guards, on that same run, which of each target's tests reached them, so a mutation is put to the tests that reached it rather than to every test of every target that did.
    pub touch: bool,
    /// Build and run the tree once with coverage instrumentation, so a mutant is only ever run against the targets that reached it.
    pub coverage: bool,
    /// Ask the compiler which mutations change nothing outside the branch they sit in, so a target that never ran that branch is not run against them.
    pub branch_proofs: bool,
    /// What to do about a target whose baseline does not pass.
    pub failing: Failing,
    /// How many validation rounds before falling back to bisection.
    pub max_rounds: u32,
    /// How long a build may take.
    pub build_timeout: Option<Duration>,
    /// How long one mutant execution may take, when the caller does not say.
    pub mutant_timeout: Timeout,
    /// How many times the active mutant's guard may be taken before its process is stopped.
    ///
    /// A mutant that does not terminate has to be stopped by something, and a clock is the wrong something: the same mutant on a loaded machine and on a quiet one is two verdicts.
    /// A count of guard takes is the number every machine agrees on, and the guard of the selected mutant sits where the mutation does — so a loop whose condition was mutated takes it once an iteration and an unbounded execution is counted as it runs.
    ///
    /// It is an allowance rather than a measurement of the tree: nothing is known in advance about how often a test reaches a site, so the number is a ceiling a person may lower and the default is one no test written by a person approaches.
    /// `None`, and zero, count nothing and leave the timeout as the only bound.
    pub mutant_steps: Option<u64>,
    /// Run a library's documented examples as a target of their own.
    pub doctests: bool,
    /// What the project is compiled as: its features, target, profile, and how many jobs cargo may use.
    pub build: crate::cargo::BuildConfig,
    /// The arguments every test binary of this session is started with, the baseline included.
    pub harness_args: Vec<String>,
    /// Targets never to start, by the id a report names them with.
    pub skip_targets: Vec<String>,
    /// Which mutants to place in the compiled tree when a later run is already known to ask about only part of the catalog.
    pub validation_filter: Option<crate::run::Filter>,
}

impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            tier: Tier::Balanced,
            operators: Vec::new(),
            scratch_working_directory: false,
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
            skips: Vec::new(),
            measurements: None,
            harness_args: Vec::new(),
            verify: true,
            touch: true,
            failing: Failing::Refuse,
            coverage: true,
            branch_proofs: true,
            max_rounds: crate::validate::DEFAULT_MAX_ROUNDS,
            build_timeout: None,
            mutant_timeout: Timeout::Auto,
            mutant_steps: Some(DEFAULT_MUTANT_STEPS),
            doctests: true,
            build: crate::cargo::BuildConfig::default(),
            skip_targets: Vec::new(),
            validation_filter: None,
        }
    }
}

/// One mutant execution to make.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Request {
    /// The mutant, by full identity or by any prefix of at least four hex characters that names exactly one.
    pub mutant: String,
    /// The target to run it against.
    /// `None` runs every target until one kills it, which is what "does any test catch this?"
    /// means.
    pub target: Option<String>,
    /// One test to run, by its libtest path.
    /// `None` runs the whole target.
    pub test: Option<String>,
    /// Further arguments for the harness.
    pub args: Vec<String>,
    /// How long the process may take.
    /// `None` uses the session's default.
    pub timeout: Option<Duration>,
    /// What each execution records of the items its process entered.
    pub entered: Recording,
}

/// What a mutant execution records of what its process entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recording {
    /// Nothing, which costs nothing.
    Off,
    /// The union of items the whole process entered, which a carried answer rests on (ADR 0041).
    Items,
}

impl Request {
    /// A request for one mutant, named by its identity or by a prefix that names exactly one.
    #[must_use]
    pub fn new(mutant: impl Into<String>) -> Self {
        Self {
            mutant: mutant.into(),
            target: None,
            test: None,
            args: Vec::new(),
            timeout: None,
            entered: Recording::Off,
        }
    }

    /// Records the items each execution's process entered.
    #[must_use]
    pub const fn recording(mut self, entered: Recording) -> Self {
        self.entered = entered;
        self
    }

    /// Runs it against one target rather than against every one until something notices.
    #[must_use]
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Runs one test of the target, or the whole target again.
    #[must_use]
    pub fn test(mut self, test: Option<String>) -> Self {
        self.test = test;
        self
    }

    /// Further arguments for the harness.
    #[must_use]
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// How long the process may take, or the session's own bound.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Repeats the exact test, arguments, and bound against the target whose clock result is being confirmed.
    /// A different target cannot confirm it.
    fn retrying_target(&self, target: &str) -> Self {
        self.clone().with_target(target)
    }
}
/// A prepared workspace.
#[derive(Debug)]
pub struct Session {
    workspace: Workspace,
    catalog: Catalog,
    /// Every file the walk considered, in path order.
    files: Vec<FileReport>,
    skips: Vec<Skip>,
    claims: Vec<SkipClaim>,
    validated: Validated,
    /// The catalog indices compiler validation was asked about.
    /// Every other index is an explicitly unvalidated candidate, never an accepted one.
    eligible: BTreeSet<u32>,
    targets: Vec<TestTarget>,
    scratch: PathBuf,
    /// Whether every test process starts in its own scratch rather than where cargo would.
    scratch_working_directory: bool,
    /// How many executions this session has started, which is what names each one's own temporary directory.
    executions: std::sync::Mutex<u64>,
    /// The process that leads each execution this session has started, recorded as it starts, whose children are that execution's rather than any other's.
    leaders: crate::orphan::Leaders,
    mutant_timeout: Timeout,
    mutant_steps: Option<u64>,
    /// The arguments every test binary of this session is started with, unless one execution names its own.
    harness_args: Vec<String>,
    /// The files as they were before instrumentation, so a position can be counted in the file a person would open rather than in the rewrite.
    sources: BTreeMap<String, String>,
    /// Which package each mutant belongs to.
    packages: BTreeMap<u32, String>,
    /// The item each mutant sits in, by catalog index.
    items: BTreeMap<u32, String>,
    /// Every cataloged item's portable name, by item index.
    item_refs: Vec<crate::touch::ItemRef>,
    /// The branch proof of every mutant that has one, by catalog index.
    proofs: BTreeMap<u32, crate::syntax::branch::Proof>,
    reached: crate::reach::Reached,
    /// What the one run of every target with nothing active established, empty when nothing was verified.
    verified: Verified,
    /// The answers and counter produced while establishing filtered test sets,
    /// kept under one lock so a report cannot observe a counter detached from the routing state that produced it.
    established: std::sync::Mutex<EstablishmentState>,
    /// What the tree gained or lost while the proof layers ran, which is what a test wrote before anything was instrumented.
    written_by_a_test: Vec<Drift>,
    /// Everything the pristine build read: the digest that keys the outcome store, and what each unit read.
    closure: Closure,
    /// What the build read that no survey of the tree sees.
    inputs: crate::select::Inputs,
    /// The digest of the manifests, the lock file, and the cargo configuration the build read.
    manifests: String,
}

/// What the carry rule has taken of a session so far: its tree, once, and each target's reach as it is first asked about.
#[derive(Debug)]
pub(crate) struct Carrying {
    pub(crate) tree: std::sync::OnceLock<carry::Tree>,
    pub(crate) held: std::sync::Mutex<BTreeMap<String, bool>>,
    pub(crate) believed: std::sync::Mutex<BelievedRecords>,
}

impl Carrying {
    /// Nothing taken yet.
    pub(crate) const fn fresh() -> Self {
        Self {
            tree: std::sync::OnceLock::new(),
            held: std::sync::Mutex::new(BTreeMap::new()),
            believed: std::sync::Mutex::new(BTreeMap::new()),
        }
    }
}

/// Every carried record a run believed, with the plan it was held to, by the full identity of its mutant.
pub(crate) type BelievedRecords =
    BTreeMap<String, (crate::carry::Carried, Vec<crate::carry::Planned>)>;

/// Everything the pristine build read, as the outcome store keys it and as each unit's skeleton is taken over.
#[derive(Debug)]
pub(crate) struct Closure {
    /// The digest of every file, variable, and build script output any unit read.
    pub(crate) digest: String,
    /// Each unit and what it read, spelled by class.
    pub(crate) units: Vec<crate::skeleton::UnitSource>,
    /// What the carry rule has taken of it so far.
    pub(crate) carrying: Carrying,
}

/// What narrowing a target's tests left: the ones that could still notice the mutation, or the proof that took the last of them away.
enum Narrowed {
    /// The target stays in the route, asked for these tests.
    Reaching(Reaches),
    /// Nothing of the target could have noticed, and this names what says so.
    Discharged(Proof),
}

impl Session {
    /// Everything discovery cataloged, refusals included.
    #[must_use]
    pub const fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The indices of the mutants that compile, ascending.
    #[must_use]
    pub fn accepted(&self) -> &[u32] {
        &self.validated.accepted
    }

    /// The candidates the compiler refused, with its own words.
    #[must_use]
    pub fn rejections(&self) -> &[Rejection] {
        &self.validated.rejections
    }

    /// Whether compiler validation considered this catalog index.
    #[must_use]
    pub fn was_validated(&self, index: u32) -> bool {
        self.eligible.contains(&index)
    }

    /// Every place discovery passed over, with its reason.
    #[must_use]
    pub fn skips(&self) -> &[Skip] {
        &self.skips
    }

    /// Every file the walk considered, in path order, whatever it yielded.
    #[must_use]
    pub fn files(&self) -> &[FileReport] {
        &self.files
    }

    /// Every `rust-mutants: skip` marker of a file this session measures, in (path, line) order.
    #[must_use]
    pub fn claims(&self) -> &[SkipClaim] {
        &self.claims
    }

    /// The test binaries this session built.
    #[must_use]
    pub fn targets(&self) -> &[TestTarget] {
        &self.targets
    }

    /// Whether a process of the tree may have run without the environment the run gave it while something ran from the first time to the second, which a directory that cannot be read cannot rule out.
    fn orphaned(
        &self,
        before: Option<&BTreeSet<crate::orphan::Orphan>>,
        (started, ended): (std::time::SystemTime, std::time::SystemTime),
        leader: Option<u32>,
    ) -> Result<bool, EngineError> {
        let others = self
            .leaders
            .every()
            .map_err(|_poisoned| SessionError::ScratchStatePoisoned)?;
        Ok(
            match (
                before,
                crate::orphan::left(std::path::Path::new(self.workspace.watched())),
            ) {
                (Some(before), Ok(orphans)) => orphans.iter().any(|orphan| {
                    !before.contains(orphan)
                        && orphan.during(started, ended)
                        && crate::orphan::ours(orphan, leader, &others)
                }),
                (None, Ok(_)) | (_, Err(_)) => true,
            },
        )
    }

    /// Every process that has said so far that it lost the run's environment, or nothing where the directory cannot be read, which an execution compares against to find the ones left while it ran.
    fn orphans(&self) -> Option<BTreeSet<crate::orphan::Orphan>> {
        match crate::orphan::left(std::path::Path::new(self.workspace.watched())) {
            Ok(orphans) => Some(orphans.into_iter().collect()),
            Err(_unreadable) => None,
        }
    }

    /// Whether a process of `target`'s tree ran without the environment the run gave it, so a survival it reports is not one.
    #[must_use]
    pub fn uncontrolled(&self, target: &str) -> bool {
        self.verified
            .touched
            .limitations
            .iter()
            .any(|one| one.split_once(':') == Some((crate::limitation::UNCONTROLLED_CHILD, target)))
    }

    /// The digest of the pristine sources every unit of this build compiled.
    #[must_use]
    pub fn closure(&self) -> &str {
        &self.closure.digest
    }

    /// What the build read that no survey of the tree sees: files outside the copy, and the variables the compiler read.
    #[must_use]
    pub const fn inputs(&self) -> &crate::select::Inputs {
        &self.inputs
    }

    /// The digest of the manifests, the lock file, and the cargo configuration the build read.
    #[must_use]
    pub fn manifests(&self) -> &str {
        &self.manifests
    }

    /// The digest of the tree as it was instrumented.
    #[must_use]
    pub fn workspace_digest(&self) -> &str {
        self.workspace.workspace_digest()
    }

    /// The directory every build of this session writes into, which is where the proof layers left their own files.
    #[must_use]
    pub fn target_dir(&self) -> &std::path::Path {
        &self.workspace.target_dir
    }

    /// The root of the copy everything runs in.
    #[must_use]
    pub fn snapshot_root(&self) -> &std::path::Path {
        self.workspace.snapshot_root()
    }

    /// Where a mutant's edit is in the file a person would open, counted in the pristine bytes rather than in the rewrite.
    #[must_use]
    pub fn position(&self, mutant: &Mutant) -> Option<Position> {
        let source = self.sources.get(&mutant.candidate.path)?;
        let index = match LineIndex::new(source) {
            Ok(index) => index,
            Err(_unrepresentable) => return None,
        };
        match index.position(mutant.candidate.span.start) {
            Ok(position) => Some(position),
            Err(_inconsistent) => None,
        }
    }

    /// The package a mutant belongs to.
    #[must_use]
    pub fn package_of(&self, index: u32) -> Option<&str> {
        self.packages.get(&index).map(String::as_str)
    }

    /// The packages this session was told to measure, which is where a reader looks for a test to write.
    #[must_use]
    pub fn packages(&self) -> Vec<String> {
        let named: BTreeSet<&String> = self.packages.values().collect();
        named.into_iter().cloned().collect()
    }

    /// The item a mutant sits in, as a reader writes it.
    #[must_use]
    pub fn item_of(&self, index: u32) -> Option<&str> {
        self.items.get(&index).map(String::as_str)
    }

    /// The one mutant a locator names.
    ///
    /// # Errors
    /// Returns [`LocateError::Nothing`] when the catalog holds no such mutation and [`LocateError::Several`] when it holds more than one and the line does not separate them.
    pub fn locate(&self, locator: &Locator) -> Result<&Mutant, LocateError> {
        match self.locate_all(locator)?.as_slice() {
            [one] => Ok(one),
            several => Err(LocateError::Several {
                display_ids: several
                    .iter()
                    .map(|mutant| {
                        self.position(mutant).map_or_else(
                            || mutant.display_id.to_string(),
                            |at| format!("{}@{}", mutant.display_id, at.line),
                        )
                    })
                    .collect(),
            }),
        }
    }

    /// Every mutation of the catalog a locator names, which is one unless it states a count.
    ///
    /// # Errors
    /// Returns [`LocateError::Nothing`] when the catalog holds no such mutation, [`LocateError::Several`] when it holds more than one and neither the line nor a count separates them, and [`LocateError::Counted`] when a count is stated and another number of them is what the catalog holds.
    pub fn locate_all(&self, locator: &Locator) -> Result<Vec<&Mutant>, LocateError> {
        let matching: Vec<&Mutant> = self
            .catalog
            .mutants()
            .iter()
            .filter(|mutant| {
                mutant.candidate.path == locator.path
                    && mutant.candidate.rule.name == locator.rule
                    && (locator.original.is_empty()
                        || mutant.candidate.original == locator.original.as_bytes())
                    && self
                        .item_of(mutant.index)
                        .is_some_and(|item| names(item, &locator.item))
            })
            .collect();
        let narrowed = match locator.line {
            Some(line) if matching.len() > 1 => matching
                .iter()
                .copied()
                .filter(|mutant| self.position(mutant).is_some_and(|at| at.line == line))
                .collect(),
            _ => matching,
        };
        let named = |several: &[&Mutant]| -> Vec<String> {
            several
                .iter()
                .map(|mutant| {
                    self.position(mutant).map_or_else(
                        || mutant.display_id.to_string(),
                        |at| format!("{}@{}", mutant.display_id, at.line),
                    )
                })
                .collect()
        };
        match (narrowed.as_slice(), locator.count) {
            ([], _) => Err(LocateError::Nothing),
            (several, Some(wanted))
                if usize::try_from(wanted).is_ok_and(|wanted| several.len() == wanted) =>
            {
                Ok(narrowed)
            }
            (several, Some(wanted)) => Err(LocateError::Counted {
                wanted,
                display_ids: named(several),
            }),
            ([one], None) => Ok(vec![one]),
            (several, None) => Err(LocateError::Several {
                display_ids: named(several),
            }),
        }
    }

    /// The source of one of the tree's mutable files, as it was before instrumentation.
    #[must_use]
    pub fn source(&self, path: &str) -> Option<&[u8]> {
        self.sources.get(path).map(String::as_bytes)
    }

    /// Every mutable source as it stood before this session instrumented it,
    /// in stable path order.
    ///
    /// A proof layer must restore the complete set, rather than only the file containing its subject, before it treats a copy of the prepared tree as pristine proof context.
    pub fn pristine_sources(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.sources
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_bytes()))
    }

    /// Everything this session is, as a document: what it catalogued, what it will run, and what it was all read from.
    #[must_use]
    pub fn describe(&self) -> Description {
        Description {
            catalog: self.catalog.clone(),
            targets: self
                .targets
                .iter()
                .map(|target| TargetDescription {
                    id: target.id.clone(),
                    package: target.package.clone(),
                    kind: target.kind.name().to_owned(),
                    name: target.name.clone(),
                    harness: target.harness,
                    limitations: target.limitations.clone(),
                })
                .collect(),
            workspace_digest: self.workspace_digest().to_owned(),
            catalog_digest: self.catalog.digest().to_owned(),
            toolchain: self.workspace.toolchain().rustc_version().summary.clone(),
        }
    }

    /// The toolchain that compiled the tree, as it names itself.
    #[must_use]
    pub const fn toolchain(&self) -> &crate::cargo::Toolchain {
        self.workspace.toolchain()
    }

    /// The branch proof of one mutant, when the compiler vouched for one.
    #[must_use]
    pub fn branch(&self, index: u32) -> Option<&crate::syntax::branch::Proof> {
        self.proofs.get(&index)
    }

    /// How many mutants carry a branch proof.
    #[must_use]
    pub fn proven(&self) -> usize {
        self.proofs.len()
    }

    /// What the coverage pass measured, empty when it did not run.
    #[must_use]
    pub const fn reached(&self) -> &crate::reach::Reached {
        &self.reached
    }

    /// What the guards recorded on the baseline run: which of each target's tests reached which mutant.
    #[must_use]
    pub const fn touched(&self) -> &crate::touch::Touched {
        &self.verified.touched
    }

    /// Which item bodies are sealed, each body's digest, and each unit's skeleton, as the pristine build left them.
    #[must_use]
    pub fn skeletons(&self) -> crate::skeleton::Skeletons {
        let items: Vec<(&crate::touch::Item, &crate::touch::ItemRef)> = self
            .verified
            .touched
            .items
            .iter()
            .zip(&self.item_refs)
            .collect();
        crate::skeleton::evidence(&self.closure.units, &items)
    }

    /// What the one run of every target with nothing active established, target by target.
    #[must_use]
    pub const fn verified(&self) -> &Verified {
        &self.verified
    }

    /// How many tests one target's baseline ran, which is what asking the whole of it about one mutation costs.
    #[must_use]
    pub fn tests_of(&self, target: &str) -> u32 {
        self.verified
            .targets
            .get(target)
            .map_or(1, |measured| measured.baseline().tests)
            .max(1)
    }

    /// How many tests this session started to establish that a set of them answers on its own.
    ///
    /// # Errors
    /// Returns a typed session failure when a panic poisoned the routing accounting state.
    pub fn established_tests(&self) -> Result<u64, EngineError> {
        self.established
            .lock()
            .map(|state| state.tests_started)
            .map_err(|_poisoned| SessionError::RoutingStatePoisoned.into())
    }

    /// What measurement established about whether any target reached this mutant.
    #[must_use]
    pub fn reaches(&self, mutant: &Mutant) -> Reachability {
        match self.covering(mutant) {
            None => Reachability::Unmeasured,
            Some(targets) if targets.is_empty() => Reachability::Unreached,
            Some(_targets) => Reachability::Reached,
        }
    }

    /// The targets an execution of this mutant runs, or nothing when nothing narrows it.
    fn covering(&self, mutant: &Mutant) -> Option<Vec<String>> {
        self.route(mutant).narrowing()
    }

    /// The documentation targets a mutation is routed to, which is by the file it is in.
    fn documenting(&self, mutant: &Mutant) -> Vec<&str> {
        let Some(package) = self.package_of(mutant.index) else {
            return Vec::new();
        };
        self.targets
            .iter()
            .filter(|target| target.kind == TargetKind::Doc && target.package == package)
            .filter(|target| {
                !target
                    .limitations
                    .iter()
                    .any(|one| one == crate::limitation::DOCTESTS_NONE)
            })
            .map(|target| target.id.as_str())
            .collect()
    }

    /// The budget one execution of `target` is given, and where it came from.
    ///
    /// # Errors
    /// Refuses when deriving a timeout from the target baseline would overflow [`Duration`].
    pub fn timeout_for(
        &self,
        request: &Request,
        target: &str,
    ) -> Result<(Duration, TimeoutSource), SessionError> {
        match request.timeout {
            Some(chosen) => Ok((chosen, TimeoutSource::Configured)),
            None => self.mutant_timeout.of(self.baseline(target)),
        }
    }

    /// How long one target's own baseline took, when it was verified.
    #[must_use]
    pub fn baseline(&self, target: &str) -> Option<Duration> {
        self.verified
            .targets
            .get(target)
            .map(|measured| measured.baseline().duration)
    }

    /// What one target costs: how long its own baseline took, and how many tests that was the cost of.
    #[must_use]
    pub fn timing(&self, target: &str) -> Timing {
        Timing::new(
            self.baseline(target).unwrap_or(DEFAULT_MUTANT_TIMEOUT),
            self.tests_of(target),
        )
    }

    /// The longest a target's own baseline took, which is what an estimate of a run's cost rests on.
    #[must_use]
    pub fn slowest_baseline(&self) -> Duration {
        self.verified
            .targets
            .values()
            .map(|measured| measured.baseline().duration)
            .max()
            .unwrap_or(Duration::from_secs(1))
    }

    /// Which targets could notice this mutation, and what the answer rests on.
    #[must_use]
    pub fn route(&self, mutant: &Mutant) -> Route {
        let targets: Vec<&str> = self
            .targets
            .iter()
            .map(|target| target.id.as_str())
            .collect();
        let measurable: Vec<&str> = self
            .targets
            .iter()
            .filter(|target| target.kind != TargetKind::Doc)
            .map(|target| target.id.as_str())
            .collect();
        let among = Routing {
            targets: &targets,
            measurable: &measurable,
            also_reaching: &self.documenting(mutant),
        };
        if self.verified.touched.measured() {
            return self.discharging(
                mutant,
                Route::by_touch(&self.verified.touched, mutant.index, &among),
            );
        }
        let Some(position) = self.position(mutant) else {
            return Route::All {
                reaching: targets.iter().map(|target| (*target).to_owned()).collect(),
                fallback: Fallback::PositionUnknown,
            };
        };
        let decided = Route::decide(
            &self.reached,
            std::path::Path::new(&mutant.candidate.path),
            crate::coverage::Point {
                line: position.line,
                column: position.byte_column,
            },
            &among,
        );
        self.discharging(mutant, decided)
    }

    /// What one execution a caller did not decide a route for is narrowed to.
    fn chosen(&self, request: &Request, mutant: &Mutant, asking: Asking) -> Chosen {
        Chosen::of(request, &self.route(mutant), asking)
    }

    /// The tests of `target` this execution runs, or nothing when it runs every test the target has.
    fn filtering(
        &self,
        target: &TestTarget,
        chosen: &Chosen,
        cancel: &Cancel,
    ) -> Result<Option<Vec<String>>, EngineError> {
        let Some(named) = chosen.tests_of(&target.id) else {
            return Ok(None);
        };
        if self.usable(target, named, cancel)?.is_none() {
            return Ok(None);
        }
        Ok(Some(named.to_vec()))
    }

    /// How long the named tests of `target` take on their own with nothing active, or nothing when running them on their own is not the same question as running the target.
    fn usable(
        &self,
        target: &TestTarget,
        tests: &[String],
        cancel: &Cancel,
    ) -> Result<Option<Duration>, EngineError> {
        let key = (target.id.clone(), tests.to_vec());
        let mut established = self
            .established
            .lock()
            .map_err(|_poisoned| SessionError::RoutingStatePoisoned)?;
        if let Some(answer) = established.answers.get(&key) {
            return Ok(*answer);
        }
        let context = Context {
            leaders: Some(&self.leaders),
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: None,
            touch: None,
            steps: None,
            profile: None,
        };
        let timeout = self.mutant_timeout.of(self.baseline(&target.id))?.0;
        let request = ExecRequest::new(target)
            .with_tests(tests.to_vec())
            .with_timeout(Some(timeout))
            .with_scratch(self.exec_scratch()?)
            .in_scratch(self.scratch_working_directory);
        let result = execute::exec(&request, &context, cancel, &self.workspace.trace);
        let asked = u32::try_from(tests.len())
            .map_err(|_overflow| SessionError::RoutingCountTooLarge { count: tests.len() })?;
        established.tests_started = established
            .tests_started
            .checked_add(u64::from(asked))
            .ok_or(SessionError::RoutingCountExhausted)?;
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let ran = result.tests_run();
        let answer = (result.outcome() == crate::outcome::Outcome::Survived && ran == Some(asked))
            .then_some(result.duration);
        if answer.is_none() {
            let why = match ran {
                Some(count) if count == asked => "did not pass on its own".to_owned(),
                Some(count) => format!("named {asked} tests and ran {count} of them"),
                None => format!("named {asked} tests but reported no executed-test count"),
            };
            self.workspace.trace.note(
                TEST_ROUTING_UNSOUND,
                &format!(
                    "{}: the set {} ({}), so every test of it runs instead of the ones a \
                     measurement named",
                    target.id,
                    why,
                    result.outcome().name()
                ),
            );
        }
        if established.answers.insert(key, answer).is_some() {
            return Err(SessionError::RoutingAnswerAlreadyEstablished.into());
        }
        drop(established);
        Ok(answer)
    }

    /// The same route with every target a proof removes moved out of what could notice the mutation.
    fn discharging(&self, mutant: &Mutant, route: Route) -> Route {
        let Route::Block {
            reaching,
            mut discharged,
            fallback,
        } = route
        else {
            return route;
        };
        let mut kept = Vec::new();
        for one in reaching {
            let target = one.target.clone();
            if let Some(proof) = self.proof_against(mutant, &target) {
                discharged.push(Discharge { target, proof });
                continue;
            }
            match self.narrowed(mutant, one) {
                Narrowed::Reaching(reaches) => kept.push(reaches),
                Narrowed::Discharged(proof) => discharged.push(Discharge { target, proof }),
            }
        }
        if kept.is_empty() && !discharged.is_empty() {
            return Route::Discharged { discharged };
        }
        Route::Block {
            reaching: kept,
            discharged,
            fallback,
        }
    }

    /// The proof that this target cannot have noticed this mutation, when a layer has one.
    fn proof_against(&self, mutant: &Mutant, target: &str) -> Option<Proof> {
        if self.never_took_the_branch(mutant, target) {
            return Some(BRANCH_NEVER_TAKEN);
        }
        if self.never_differed(mutant.index, target) {
            return Some(NEVER_INFECTED);
        }
        None
    }

    /// Whether every run of this target's guard answered the same on both of its branches.
    fn never_differed(&self, index: u32, target: &str) -> bool {
        self.verified.touched.narrowing.compared.contains(&index)
            && self
                .verified
                .touched
                .targets
                .get(target)
                .is_some_and(|touches| !touches.infected.any(index))
    }

    /// The same target asked for only the tests of it that could still have noticed the mutation, or nothing when none of them could.
    fn narrowed(&self, mutant: &Mutant, one: Reaches) -> Narrowed {
        let Asked::These(tests) = &one.tests else {
            return Narrowed::Reaching(one);
        };
        let Some(touches) = self.verified.touched.targets.get(&one.target) else {
            return Narrowed::Reaching(one);
        };
        let entered: Vec<String> = tests
            .iter()
            .filter(|test| self.entered_the_body(mutant.index, touches, test))
            .cloned()
            .collect();
        if entered.is_empty() {
            return Narrowed::Discharged(BRANCH_NEVER_TAKEN);
        }
        let differed: Vec<String> = entered
            .into_iter()
            .filter(|test| self.saw_a_difference(mutant.index, touches, test))
            .collect();
        if differed.is_empty() {
            return Narrowed::Discharged(NEVER_INFECTED);
        }
        Narrowed::Reaching(Reaches {
            target: one.target,
            tests: Asked::These(differed),
        })
    }

    /// Whether this test entered the body a branch proof about `mutant` names, where a mutant with no such proof and a body with no marker say nothing.
    fn entered_the_body(
        &self,
        index: u32,
        touches: &crate::touch::TargetTouches,
        test: &str,
    ) -> bool {
        self.verified
            .touched
            .narrowing
            .bodies
            .get(&index)
            .is_none_or(|marker| touches.bodies.by(test, *marker))
    }

    /// Whether this test saw the two branches of the guard part, where a guard the built tree does not compare says nothing.
    fn saw_a_difference(
        &self,
        index: u32,
        touches: &crate::touch::TargetTouches,
        test: &str,
    ) -> bool {
        !self.verified.touched.narrowing.compared.contains(&index)
            || touches.infected.by(test, index)
    }

    /// Whether nothing of `target` ran the body the branch proof of `mutant` names.
    fn never_took_the_branch(&self, mutant: &Mutant, target: &str) -> bool {
        let Some(proof) = self.branch(mutant.index) else {
            return false;
        };
        if let Some(marker) = self.verified.touched.narrowing.bodies.get(&mutant.index)
            && let Some(touches) = self.verified.touched.targets.get(target)
            && !touches.bodies.any(*marker)
        {
            return true;
        }
        self.reached.targets.get(target).is_some_and(|covered| {
            crate::prove::discharges(
                proof,
                std::path::Path::new(&mutant.candidate.path),
                &covered.iter().cloned().collect::<Vec<_>>(),
            )
        })
    }

    /// The recording this session writes to, which is the one the workspace was opened with.
    #[must_use]
    pub const fn trace(&self) -> &crate::trace::Recorder {
        &self.workspace.trace
    }

    /// The name of the directory the source root sits in, which is what a report calls the workspace.
    ///
    /// # Errors
    /// Returns an engine error when the platform spelling cannot cross the catalog's UTF-8 boundary without changing its bytes.
    pub fn root_name(&self) -> Result<String, EngineError> {
        let Some(name) = self.workspace.root().file_name() else {
            return Ok(String::new());
        };
        crate::id::slashed(std::path::Path::new(name))
            .map_err(|source| SessionError::WorkspacePathNotUtf8 { source })
            .map_err(EngineError::from)
    }

    /// The mutant a name refers to: an identity, a prefix of one, or a locator.
    ///
    /// # Errors
    /// [`SessionError::UnknownMutant`] when nothing matches, when several do,
    /// or when what was given is neither a locator nor a usable prefix.
    pub fn resolve(&self, name: &str) -> Result<&Mutant, EngineError> {
        let Some(locator) = Locator::parse(name) else {
            return self.catalog.resolve_prefix(name).map_err(|error| {
                EngineError::from(SessionError::UnknownMutant {
                    message: error.to_string(),
                })
            });
        };
        let found = self.locate_all(&locator).map_err(|error| {
            EngineError::from(SessionError::UnknownMutant {
                message: error.to_string(),
            })
        })?;
        found.first().copied().ok_or_else(|| {
            EngineError::from(SessionError::UnknownMutant {
                message: format!("no mutation of {name}"),
            })
        })
    }

    /// A catalogued mutant this instrumented build actually contains.
    fn executable(&self, prefix: &str) -> Result<&Mutant, EngineError> {
        let mutant = self.resolve(prefix)?;
        if self.validated.accepted.binary_search(&mutant.index).is_ok() {
            return Ok(mutant);
        }
        let why = if self.was_validated(mutant.index) {
            "the compiler refused it"
        } else {
            "the preparation filter left it unvalidated"
        };
        Err(EngineError::from(SessionError::UnknownMutant {
            message: format!(
                "{} is in the catalog but not in this instrumented build: {why}",
                mutant.display_id
            ),
        }))
    }

    /// A temporary directory of this execution's own, so two executions at once cannot meet in one another's files.
    fn exec_scratch(&self) -> Result<PathBuf, EngineError> {
        let mut next = self
            .executions
            .lock()
            .map_err(|_poisoned| SessionError::ScratchStatePoisoned)?;
        let at = *next;
        let after = at
            .checked_add(1)
            .ok_or(SessionError::ScratchSequenceExhausted)?;
        let own = self.scratch.join(at.to_string());
        std::fs::DirBuilder::new().create(&own).map_err(|source| {
            SessionError::ScratchCreateFailed {
                path: own.clone(),
                source,
            }
        })?;
        *next = after;
        drop(next);
        Ok(own)
    }

    /// Runs one mutant and reports what the tests said.
    ///
    /// # Errors
    /// [`SessionError::UnknownMutant`], [`SessionError::UnknownTarget`], and [`SessionError::NoTargets`].
    pub fn exec(&self, request: &Request, cancel: &Cancel) -> Result<MutantResult, EngineError> {
        let mutant = self.executable(&request.mutant)?;
        let chosen = self.chosen(request, mutant, Asking::Anything);
        Ok(self
            .execute(
                request,
                Running {
                    alone: false,
                    chosen: &chosen,
                    mutant,
                },
                cancel,
            )?
            .taken)
    }

    /// What one mutant is, decided: executed, and when a budget expired, confirmed with the machine to itself.
    ///
    /// # Errors
    /// [`SessionError::UnknownMutant`], [`SessionError::UnknownTarget`], and [`SessionError::NoTargets`].
    pub fn judge(
        &self,
        request: &Request,
        quiet: &crate::run::Quiet,
        cancel: &Cancel,
    ) -> Result<Judgement, EngineError> {
        let mutant = self.executable(&request.mutant)?;
        let route = self.route(mutant);
        let chosen = Chosen::of(request, &route, Asking::ThisRun);
        let running = |alone: bool| Running {
            alone,
            chosen: &chosen,
            mutant,
        };
        let ran = quiet.shared(|| self.execute(request, running(false), cancel))??;
        let (first, mut asked) = (ran.taken, ran.asked);
        let (timeout, timeout_source) = self.timeout_for(request, &first.target)?;
        let attempts = match InitialAttempt::classify(first, cancel.is_cancelled()) {
            InitialAttempt::Final(first) => AttemptLedger::single(first),
            InitialAttempt::Retry(first) => {
                let retry = request.retrying_target(&first.result().target);
                let repeated = quiet.alone(|| self.execute(&retry, running(true), cancel))??;
                asked.extend(repeated.asked);
                AttemptLedger::with_retry(first, repeated.taken, cancel.is_cancelled())?
            }
        };
        let judgement = Judgement {
            attempts,
            asked,
            timeout,
            timeout_source,
            route,
        };
        let result = judgement.result();
        self.workspace.trace.route(
            judgement.route.record(
                mutant,
                judgement
                    .route
                    .executed(&result.target, result.outcome().detected()),
            ),
        );
        Ok(judgement)
    }

    fn execute(
        &self,
        request: &Request,
        how: Running<'_>,
        cancel: &Cancel,
    ) -> Result<Ran, EngineError> {
        let Running {
            alone,
            chosen,
            mutant,
        } = how;
        let targets = self.selected(request.target.as_deref())?;
        let targets = match chosen {
            Chosen::Everything => targets,
            Chosen::Narrowed { only, .. } => {
                let routed: Vec<&TestTarget> = targets
                    .into_iter()
                    .filter(|target| only.iter().any(|one| one == &target.id))
                    .collect();
                if routed.is_empty() {
                    return Ok(Ran {
                        taken: unreached(),
                        asked: Vec::new(),
                    });
                }
                routed
            }
        };
        let mut asked: Vec<MutantResult> = Vec::new();
        for target in targets {
            let (timeout, source) = self.timeout_for(request, &target.id)?;
            let scratch = self.exec_scratch()?;
            let log = match request.entered {
                Recording::Off => None,
                Recording::Items => Some(scratch.join(ENTERED_LOG)),
            };
            let context = self.mutant_context(mutant, log.as_deref());
            let mut exec = ExecRequest::new(target)
                .with_args(self.arguments(request))
                .with_timeout(Some(timeout))
                .with_scratch(scratch)
                .in_scratch(self.scratch_working_directory);
            if let Some(test) = &request.test {
                exec = exec.with_test(test.clone());
            } else if let Some(named) = self.filtering(target, chosen, cancel)? {
                exec = exec.with_tests(named);
            }
            let before = self.orphans();
            let started = std::time::SystemTime::now();
            let mut result = execute::exec(&exec, &context, cancel, &self.workspace.trace);
            result.entered = self.entered_by(log.as_deref(), &result, cancel);
            let ended = std::time::SystemTime::now();
            let unseen = self.uncontrolled(&target.id)
                || self.orphaned(before.as_ref(), (started, ended), result.leader)?;
            if unseen {
                if result.conclusion == MutantConclusion::Survived {
                    result.conclusion = MutantConclusion::Unobserved;
                }
                if let Some(entered) = result.entered.as_mut() {
                    entered.completeness = crate::touch::Completeness::Cut;
                }
            }
            self.record_mutant_exec(Executed {
                mutant,
                target,
                result: &result,
                timeout,
                source,
                alone,
            })?;
            asked.push(result.clone());
            if result.outcome().detected() || cancel.is_cancelled() {
                return Ok(Ran {
                    taken: result,
                    asked,
                });
            }
        }
        let taken = verdict_result(&asked).ok_or_else(|| {
            EngineError::from(SessionError::NoTargets {
                packages: self.packages(),
            })
        })?;
        Ok(Ran { taken, asked })
    }

    /// What one execution of `mutant` runs with: the mutant active, and a log of what it entered where one was asked for.
    fn mutant_context<'a>(
        &'a self,
        mutant: &'a Mutant,
        log: Option<&'a std::path::Path>,
    ) -> Context<'a> {
        Context {
            leaders: Some(&self.leaders),
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: Some((mutant.id.as_str(), self.catalog.digest())),
            touch: log.map(|log| execute::Touching {
                log,
                catalog: self.catalog.digest(),
                scope: execute::TouchScope::Items,
            }),
            steps: self.mutant_steps,
            profile: None,
        }
    }

    /// The items one execution's whole process entered, read from the log it was asked to keep, or nothing where it kept none that can be read.
    fn entered_by(
        &self,
        log: Option<&std::path::Path>,
        result: &MutantResult,
        cancel: &Cancel,
    ) -> Option<crate::touch::Entered> {
        let log = log?;
        if result.exit_code == crate::instrument::TOUCH_UNAVAILABLE_EXIT {
            return None;
        }
        let text = match std::fs::read_to_string(log) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_unreadable) => return None,
        };
        let (Ok(mutants), Ok(items)) = (
            u32::try_from(self.catalog.mutants().len()),
            u32::try_from(self.item_refs.len()),
        ) else {
            return None;
        };
        let bounds = crate::touch::Bounds { mutants, items };
        let touches = match crate::touch::read(&text, self.catalog.digest(), bounds) {
            Ok(touches) => touches,
            Err(_malformed) => return None,
        };
        let items = touches
            .entered
            .union()
            .into_iter()
            .map(|index| match usize::try_from(index) {
                Ok(at) => self.item_refs.get(at).cloned(),
                Err(_beyond_this_target) => None,
            })
            .collect::<Option<BTreeSet<_>>>()?;
        let completeness = match result.conclusion {
            _ if cancel.is_cancelled() => crate::touch::Completeness::Cut,
            MutantConclusion::Killed | MutantConclusion::Survived => {
                crate::touch::Completeness::Whole
            }
            MutantConclusion::NotRun
            | MutantConclusion::StepLimitReached { .. }
            | MutantConclusion::Waited
            | MutantConclusion::Inconclusive
            | MutantConclusion::Unobserved
            | MutantConclusion::Errored => crate::touch::Completeness::Cut,
        };
        let written = text
            .lines()
            .filter(|line| line.starts_with(crate::touch::ENTERED_RECORD))
            .count();
        let Ok(records) = u32::try_from(written) else {
            return None;
        };
        Some(crate::touch::Entered {
            items,
            completeness,
            records,
        })
    }

    fn record_mutant_exec(&self, executed: Executed<'_>) -> Result<(), EngineError> {
        let Executed {
            mutant,
            target,
            result,
            timeout,
            source,
            alone,
        } = executed;
        self.workspace.trace.mutant_exec(MutantExecRecord {
            id: mutant.id.to_string(),
            index: mutant.index,
            target: target.id.clone(),
            outcome: result.outcome().name().to_owned(),
            step_notice: result.step_notice().cloned(),
            exit_code: result.exit_code,
            duration_ms: duration_ms(result.duration)?,
            tests_run: result.tests_run(),
            signal: result.signal,
            failed_tests: result.failed_tests.clone(),
            timeout_ms: duration_ms(timeout)?,
            timeout_source: source.name().to_owned(),
            alone,
            entered_records: result.entered.as_ref().map(|entered| entered.records),
        });
        Ok(())
    }

    /// The arguments one execution's test binary is started with.
    fn arguments(&self, request: &Request) -> Vec<String> {
        if request.args.is_empty() {
            self.harness_args.clone()
        } else {
            request.args.clone()
        }
    }

    /// Runs one target whole with nothing active and the files `absent` taken out of the copy, then puts each back exactly as it was: what a test reads of the tree, as opposed to what it runs, is what such a run answers differently.
    ///
    /// It takes the session exclusively, since nothing else may run while the copy is missing files.
    ///
    /// # Errors
    /// What [`Self::control`] refuses, and a file that could not be moved aside or put back.
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "the copy is missing files while this runs, and taking the session exclusively is \
                  what keeps any other execution from running against it"
    )]
    pub fn control_without(
        &mut self,
        request: &Request,
        cancel: &Cancel,
        absent: &[String],
    ) -> Result<MutantResult, EngineError> {
        let root = self.workspace.snapshot_root().to_path_buf();
        let aside = self.workspace.snapshot.dir().join(ASIDE_NAME);
        std::fs::create_dir_all(&aside).map_err(|source| SessionError::WriteFailed {
            path: aside.display().to_string(),
            source,
        })?;
        let mut moved = Vec::new();
        let mut taken = Ok(());
        for (index, path) in absent.iter().enumerate() {
            let (from, to) = (root.join(path), aside.join(index.to_string()));
            if let Err(source) = std::fs::rename(&from, &to) {
                taken = Err(SessionError::WriteFailed {
                    path: from.display().to_string(),
                    source,
                });
                break;
            }
            moved.push((from, to));
        }
        let ran = match taken {
            Ok(()) => self
                .control(request, cancel, Observing::Nothing)
                .map(|ran| ran.result),
            Err(error) => Err(error.into()),
        };
        for (from, to) in moved.iter().rev() {
            std::fs::rename(to, from).map_err(|source| SessionError::WriteFailed {
                path: from.display().to_string(),
                source,
            })?;
        }
        ran
    }

    /// Runs one target with no mutant active under `conditions`: recording what it reached where they ask, and started with what they add to the baseline's start.
    ///
    /// # Errors
    /// As [`Session::control`].
    pub fn control_perturbed(
        &self,
        request: &Request,
        conditions: Conditions<'_>,
        cancel: &Cancel,
    ) -> Result<Controlled, EngineError> {
        self.controlled(request, conditions, cancel)
    }

    /// Runs one target with no mutant active: the original control, recording what it reached where `observing` asks.
    ///
    /// # Errors
    /// [`SessionError::UnknownTarget`] and [`SessionError::NoTargets`].
    pub fn control(
        &self,
        request: &Request,
        cancel: &Cancel,
        observing: Observing,
    ) -> Result<Controlled, EngineError> {
        let none = Perturbation::none();
        self.controlled(
            request,
            Conditions {
                observing,
                perturbation: &none,
            },
            cancel,
        )
    }

    /// A control of `request` under `perturbation`, which is what [`Session::control`] and [`Session::control_perturbed`] both are.
    fn controlled(
        &self,
        request: &Request,
        Conditions {
            observing,
            perturbation,
        }: Conditions<'_>,
        cancel: &Cancel,
    ) -> Result<Controlled, EngineError> {
        let targets = self.selected(request.target.as_deref())?;
        let mut asked = Vec::new();
        let mut observed = Vec::new();
        for target in targets {
            let (timeout, source) = self.timeout_for(request, &target.id)?;
            let own = self.exec_scratch()?;
            let log = (observing == Observing::Reach
                && request.test.is_none()
                && verify::recordable(target))
            .then(|| own.join(CONTROL_TOUCH_LOG));
            let once = Once {
                request,
                target,
                timeout,
                perturbation,
            };
            let mut result = self.control_once(&once, (&own, log.as_deref()), cancel);
            let unrecorded =
                log.is_some() && result.exit_code == crate::instrument::TOUCH_UNAVAILABLE_EXIT;
            if unrecorded {
                self.workspace.trace.note(
                    crate::touch::UNRECORDED,
                    &format!(
                        "{}: the control could not write what its guards reached, so it is run \
                         again with nothing to record and whether its baseline reach holds is \
                         not measured",
                        target.id
                    ),
                );
                result = self.control_once(&once, (&self.exec_scratch()?, None), cancel);
            }
            self.workspace.trace.mutant_exec(MutantExecRecord {
                entered_records: None,
                id: String::new(),
                index: u32::MAX,
                target: target.id.clone(),
                outcome: result.outcome().name().to_owned(),
                step_notice: result.step_notice().cloned(),
                exit_code: result.exit_code,
                duration_ms: duration_ms(result.duration)?,
                tests_run: result.tests_run(),
                signal: result.signal,
                failed_tests: result.failed_tests.clone(),
                timeout_ms: duration_ms(timeout)?,
                timeout_source: source.name().to_owned(),
                alone: false,
            });
            if let Some(log) = log.as_deref() {
                let steadiness = if unrecorded {
                    crate::touch::Steadiness::NotMeasured(crate::touch::Unmeasured::Unrecorded)
                } else {
                    self.steadiness(target, log, &result)?
                };
                observed.push(Observed {
                    target: target.id.clone(),
                    steadiness,
                });
            }
            if cancel.is_cancelled()
                || (result.outcome() != crate::outcome::Outcome::Survived && spoke(&result))
            {
                return Ok(Controlled { result, observed });
            }
            asked.push(result);
        }
        let result = aggregate_result(&asked).cloned().ok_or_else(|| {
            EngineError::from(SessionError::NoTargets {
                packages: self.packages(),
            })
        })?;
        Ok(Controlled { result, observed })
    }

    /// One process of a control, in `own` scratch, recording into `log` when there is one.
    fn control_once(
        &self,
        once: &Once<'_>,
        (own, log): (&std::path::Path, Option<&std::path::Path>),
        cancel: &Cancel,
    ) -> MutantResult {
        let perturbation = once.perturbation;
        let context = Context {
            leaders: Some(&self.leaders),
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: None,
            touch: log.map(|log| execute::Touching {
                scope: execute::TouchScope::Everything,
                log,
                catalog: self.catalog.digest(),
            }),
            steps: None,
            profile: None,
        };
        if perturbation.schedule != execute::Schedule::AsConfigured && !once.target.harness {
            return MutantResult::apparatus_error(
                &once.target.id,
                format!(
                    "{:?} is a libtest argument, and {} does not run under libtest",
                    perturbation.schedule, once.target.id
                ),
            );
        }
        let mut arguments = self.arguments(once.request);
        arguments.extend(
            perturbation
                .schedule
                .arguments()
                .iter()
                .map(|argument| (*argument).to_owned()),
        );
        let mut exec = ExecRequest::new(once.target)
            .with_args(arguments)
            .with_timeout(Some(once.timeout))
            .with_scratch(own)
            .in_scratch(self.scratch_working_directory)
            .with_overlay(perturbation.environment.clone())
            .with_launcher(perturbation.launcher);
        if let Some(test) = &once.request.test {
            exec = exec.with_test(test.clone());
        }
        execute::exec(&exec, &context, cancel, &self.workspace.trace)
    }

    /// Whether the control that wrote `log` reached what the baseline of `target` did, over the same passing tests.
    fn steadiness(
        &self,
        target: &TestTarget,
        log: &std::path::Path,
        result: &MutantResult,
    ) -> Result<crate::touch::Steadiness, EngineError> {
        use crate::touch::{Steadiness, Unmeasured};
        if result.outcome() != crate::outcome::Outcome::Survived {
            return Ok(Steadiness::NotMeasured(Unmeasured::ControlFailed));
        }
        let unreadable = |why: &dyn std::fmt::Display| {
            self.workspace.trace.note(
                crate::touch::UNREADABLE,
                &format!("{}: the control's record: {why}", target.id),
            );
            Steadiness::NotMeasured(Unmeasured::Unreadable)
        };
        let text = match crate::limitation::appended(std::fs::read_to_string(log)) {
            Ok(text) => text,
            Err(error) => return Ok(unreadable(&error)),
        };
        let mutants = self.catalog.mutants().len();
        let count =
            u32::try_from(mutants).map_err(|_outside_range| SessionError::TraceCountTooLarge {
                subject: "catalog mutants in a touch record",
                count: mutants,
            })?;
        let cataloged = self.verified.touched.items.len();
        let items = u32::try_from(cataloged).map_err(|_outside_range| {
            SessionError::TraceCountTooLarge {
                subject: "cataloged items in a touch record",
                count: cataloged,
            }
        })?;
        let bounds = crate::touch::Bounds {
            mutants: count,
            items,
        };
        let recorded = match crate::touch::read(&text, self.catalog.digest(), bounds) {
            Ok(recorded) => recorded,
            Err(error) => return Ok(unreadable(&error)),
        };
        let control = crate::touch::TargetTouches::of(recorded, &result.passed_tests);
        self.workspace.trace.touch(verify::touch_record(
            &target.id,
            crate::trace::Measurement::Control,
            &control,
            crate::trace::SummaryRecord::of(result),
        )?);
        let retried = format!(
            "{}:{}",
            crate::limitation::BASELINE_PASSED_ON_RETRY,
            target.id
        );
        if self.verified.touched.limitations.contains(&retried) {
            return Ok(Steadiness::NotMeasured(Unmeasured::BaselineRetried));
        }
        let unparsed = format!(
            "{}:{}",
            crate::limitation::BASELINE_PASSED_UNPARSED,
            target.id
        );
        if result.reading() == Reading::Short
            || self.verified.touched.limitations.contains(&unparsed)
        {
            return Ok(Steadiness::NotMeasured(Unmeasured::Unparsed));
        }
        let Some(baseline) = self.verified.touched.targets.get(&target.id) else {
            return Ok(Steadiness::NotMeasured(Unmeasured::NoBaseline));
        };
        let passed = |ran: &[String]| ran.iter().cloned().collect::<BTreeSet<String>>();
        if passed(&baseline.ran) != passed(&control.ran) {
            return Ok(Steadiness::NotMeasured(Unmeasured::OtherTests));
        }
        Ok(match crate::touch::unions_differ(baseline, &control) {
            Some(moved) => Steadiness::Moved(moved),
            None => Steadiness::Held,
        })
    }

    /// Runs the targets that can observe anything with no mutant active,
    /// skipping harness targets whose own summary says they ran no tests: a target with nothing to say cannot veto a control that passed everywhere it ran.
    /// Verdict aggregation over mutations keeps the universal claim; this is the boundary a candidate check stands on.
    ///
    /// # Errors
    /// [`SessionError::NoTargets`] when every selected target ran nothing.
    pub fn control_observing(
        &self,
        request: &Request,
        cancel: &Cancel,
    ) -> Result<MutantResult, EngineError> {
        let targets = self.selected(request.target.as_deref())?;
        let context = Context {
            leaders: Some(&self.leaders),
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: None,
            touch: None,
            steps: None,
            profile: None,
        };
        let mut asked = Vec::new();
        for target in targets {
            let (timeout, source) = self.timeout_for(request, &target.id)?;
            let mut exec = ExecRequest::new(target)
                .with_args(self.arguments(request))
                .with_timeout(Some(timeout))
                .with_scratch(self.exec_scratch()?)
                .in_scratch(self.scratch_working_directory);
            if let Some(test) = &request.test {
                exec = exec.with_test(test.clone());
            }
            let result = execute::exec(&exec, &context, cancel, &self.workspace.trace);
            self.workspace.trace.mutant_exec(MutantExecRecord {
                entered_records: None,
                id: String::new(),
                index: u32::MAX,
                target: target.id.clone(),
                outcome: result.outcome().name().to_owned(),
                step_notice: result.step_notice().cloned(),
                exit_code: result.exit_code,
                duration_ms: duration_ms(result.duration)?,
                tests_run: result.tests_run(),
                signal: result.signal,
                failed_tests: result.failed_tests.clone(),
                timeout_ms: duration_ms(timeout)?,
                timeout_source: source.name().to_owned(),
                alone: false,
            });
            if cancel.is_cancelled() {
                return Ok(result);
            }
            if result.outcome() != crate::outcome::Outcome::Survived && spoke(&result) {
                return Ok(result);
            }
            if result
                .summary
                .as_ref()
                .is_some_and(execute::Summary::ran_nothing)
            {
                continue;
            }
            asked.push(result);
        }
        aggregate_result(&asked).cloned().ok_or_else(|| {
            EngineError::from(SessionError::NoTargets {
                packages: self.packages(),
            })
        })
    }

    /// Every way the snapshot stopped matching the tree that was instrumented: what a test wrote into the tree every later mutant is measured against.
    ///
    /// # Errors
    /// The snapshot's walk failures and refusals.
    pub fn changes(&self) -> Result<Vec<Drift>, EngineError> {
        let mut found = self.written_by_a_test.clone();
        found.extend(self.workspace.snapshot.redigest()?);
        found.sort_by(|a, b| a.rel_path().cmp(b.rel_path()));
        found.dedup();
        Ok(found)
    }

    /// Removes the snapshot, or preserves it, and reports what was kept.
    ///
    /// # Errors
    /// A snapshot directory that could not be removed.
    pub fn close(self) -> Result<Vec<PathBuf>, EngineError> {
        self.workspace.close()
    }

    /// The targets a request names, or every target.
    fn selected(&self, name: Option<&str>) -> Result<Vec<&TestTarget>, EngineError> {
        if self.targets.is_empty() {
            return Err(EngineError::from(SessionError::NoTargets {
                packages: self.packages(),
            }));
        }
        let Some(name) = name else {
            let answering: Vec<&TestTarget> = self
                .targets
                .iter()
                .filter(|target| {
                    !target
                        .limitations
                        .iter()
                        .any(|one| one == crate::limitation::DOCTESTS_NONE)
                })
                .collect();
            if answering.is_empty() {
                return Err(EngineError::from(SessionError::NoTargets {
                    packages: self.packages(),
                }));
            }
            return Ok(answering);
        };
        let matching: Vec<&TestTarget> = self
            .targets
            .iter()
            .filter(|target| target.id == name || target.name == name)
            .collect();
        if matching.is_empty() {
            return Err(EngineError::from(SessionError::UnknownTarget {
                name: name.to_owned(),
                available: self.targets.iter().map(|one| one.id.clone()).collect(),
            }));
        }
        Ok(matching)
    }
}

/// Catalogs what would be mutated, without instrumenting anything.
///
/// # Errors
/// The pristine gate and the failures of discovery.
pub fn preview(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<discover::Discovery, EngineError> {
    let trace = workspace.trace.clone();
    let phase = trace.phase("preview");
    let checked = pristine(workspace, options, cancel)?;
    let discovery = discover::discover(
        &discover::Input {
            root: workspace.snapshot_root(),
            metadata: &workspace.metadata,
            units: &checked.units,
        },
        &DiscoverOptions {
            selection: selection(options)?,
            include: options.include.clone(),
            exclude: options.exclude.clone(),
            packages: options.packages.clone(),
            skips: options.skips.clone(),
        },
        &trace,
    )?;
    phase.end();
    Ok(discovery)
}

/// One test target, as a document says what it is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct TargetDescription {
    /// The identity a route and a report name it by.
    pub id: String,
    /// The package it belongs to.
    pub package: String,
    /// `lib`, `bin`, `test`, `bench`, `example`, or `doc`.
    pub kind: String,
    /// The target's own name.
    pub name: String,
    /// Whether it is built with the libtest harness, which is what lets silence be read.
    pub harness: bool,
    /// What a run cannot establish about it, each named.
    pub limitations: Vec<String>,
}

/// Everything a session is, as a document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct Description {
    /// Every mutant, refusals included.
    pub catalog: Catalog,
    /// Every target the run will start.
    pub targets: Vec<TargetDescription>,
    /// The frozen identity of the tree it was read from.
    pub workspace_digest: String,
    /// The identity of the catalog.
    pub catalog_digest: String,
    /// The toolchain, as it names itself.
    pub toolchain: String,
}

/// How one execution is run: which targets, and whether it had the machine to itself.
#[derive(Debug, Clone, Copy)]
struct Running<'a> {
    /// Whether nothing else this run started was running beside it.
    alone: bool,
    /// What the route this execution rests on narrowed it to.
    chosen: &'a Chosen,
    /// The mutation, resolved once by whoever asked rather than again here.
    mutant: &'a Mutant,
}

#[derive(Clone, Copy)]
struct Executed<'a> {
    mutant: &'a Mutant,
    target: &'a TestTarget,
    result: &'a MutantResult,
    timeout: Duration,
    source: TimeoutSource,
    alone: bool,
}

/// Whose evidence an execution is narrowed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asking {
    /// What the tests say, which is the question a caller with its own evidence asks.
    Anything,
    /// What this run established, which is the question its own verdict answers.
    ThisRun,
}

/// What one execution was narrowed to, once the route it rests on has been decided.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Chosen {
    /// Nothing narrows it: every target the request selects runs every test it has.
    Everything,
    /// Exactly these targets, each asked for exactly the tests it is paired with.
    Narrowed {
        /// The targets to run.
        only: Vec<String>,
        /// Each target this narrowing keeps, and which of its tests the mutation is put to.
        asked: Vec<Reaches>,
    },
}

impl Chosen {
    /// What `route` narrows this request to, which is nothing when the request named a target itself.
    fn of(request: &Request, route: &Route, asking: Asking) -> Self {
        if request.target.is_some() {
            return Self::Everything;
        }
        match asking {
            Asking::Anything => route
                .narrowing()
                .map_or(Self::Everything, |only| Self::Narrowed {
                    only,
                    asked: route.asked(),
                }),
            Asking::ThisRun => Self::Narrowed {
                only: route
                    .reaching()
                    .into_iter()
                    .map(ToOwned::to_owned)
                    .collect(),
                asked: route.asked(),
            },
        }
    }

    /// The tests of `target` this narrowing names, or nothing when it runs every test the target has.
    fn tests_of(&self, target: &str) -> Option<&[String]> {
        match self {
            Self::Everything => None,
            Self::Narrowed { asked, .. } => asked
                .iter()
                .find(|one| one.target == target)
                .map(|one| one.tests.named())
                .filter(|named| !named.is_empty()),
        }
    }
}

/// What running one mutation against the targets a route chose came to.
#[derive(Debug, Clone)]
struct Ran {
    /// The answer the run takes: the first detection, or the last target that spoke.
    taken: MutantResult,
    /// Every target that was asked, in the order they were asked.
    asked: Vec<MutantResult>,
}

/// What a run notes when a set of tests does not answer on its own, so the whole target ran instead.
pub const TEST_ROUTING_UNSOUND: &str = "test-routing-unsound";

/// What each set of tests a route named answers on its own: how long it took with nothing active, or nothing when running it on its own is not the same question as running its target.
type Established = BTreeMap<(String, Vec<String>), Option<Duration>>;

/// Filtered-test facts and the exact amount of work used to establish them.
#[derive(Debug)]
struct EstablishmentState {
    answers: Established,
    tests_started: u64,
}

impl EstablishmentState {
    /// Nothing established and no test started yet.
    const fn fresh() -> Self {
        Self {
            answers: BTreeMap::new(),
            tests_started: 0,
        }
    }
}

/// A non-empty ledger of one mutant's executions.
///
/// The first execution is structurally mandatory.
/// A retry can only be added from a waited attempt, and its conclusion is reconciled before it enters the ledger.
/// The effective result and the retry bit are consequently projections, not independently writable state.
///
/// A caller cannot fabricate an empty or unreconciled ledger:
///
/// ```compile_fail
/// use rust_mutants::session::AttemptLedger;
///
/// let _empty = AttemptLedger {};
/// ```
#[derive(Debug, Clone)]
pub struct AttemptLedger {
    first: MutantResult,
    retry: Option<MutantResult>,
    duration: Duration,
}

impl AttemptLedger {
    const fn single(first: MutantResult) -> Self {
        let duration = first.duration;
        Self {
            first,
            retry: None,
            duration,
        }
    }

    fn with_retry(
        first: WaitedAttempt,
        mut repeated: MutantResult,
        cancelled: bool,
    ) -> Result<Self, SessionError> {
        repeated.reconcile_outcome(retry_outcome(repeated.outcome(), cancelled));
        let first = first.into_result();
        let duration = first
            .duration
            .checked_add(repeated.duration)
            .ok_or(SessionError::ExecutionDurationOverflow)?;
        Ok(Self {
            first,
            retry: Some(repeated),
            duration,
        })
    }

    /// The conclusion this ordered ledger establishes.
    #[must_use]
    pub const fn result(&self) -> &MutantResult {
        match &self.retry {
            Some(repeated) => repeated,
            None => &self.first,
        }
    }

    /// Consumes the ledger and returns the conclusion it establishes.
    #[must_use]
    pub fn into_result(self) -> MutantResult {
        match self.retry {
            Some(repeated) => repeated,
            None => self.first,
        }
    }

    /// Every execution in causal order.
    pub fn iter(&self) -> impl Iterator<Item = &MutantResult> {
        std::iter::once(&self.first).chain(self.retry.iter())
    }

    /// The number of executions, which is one or two by construction.
    #[must_use]
    pub const fn attempt_count(&self) -> usize {
        match self.retry {
            Some(_) => 2,
            None => 1,
        }
    }

    /// Whether the first wait was checked by a serial retry.
    #[must_use]
    pub const fn retried(&self) -> bool {
        self.retry.is_some()
    }

    /// How long every execution took together.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }
}

/// A first attempt whose only possible conclusion is a wall-clock wait.
#[derive(Debug, Clone)]
struct WaitedAttempt(MutantResult);

impl WaitedAttempt {
    const fn result(&self) -> &MutantResult {
        &self.0
    }

    fn into_result(self) -> MutantResult {
        self.0
    }
}

/// Whether the first execution is final or requires the isolated retry.
#[derive(Debug, Clone)]
enum InitialAttempt {
    Final(MutantResult),
    Retry(WaitedAttempt),
}

impl InitialAttempt {
    fn classify(first: MutantResult, cancelled: bool) -> Self {
        if first.outcome() == crate::outcome::Outcome::Waited && !cancelled {
            Self::Retry(WaitedAttempt(first))
        } else {
            Self::Final(first)
        }
    }
}

/// What one mutant's judgement is made of: every attempt it took, and the budget each was given.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Judgement {
    /// The non-empty, ordered execution ledger.
    /// Its last entry is the result.
    pub attempts: AttemptLedger,
    /// Every target that was actually asked, in the order they were asked, with what each answered.
    /// A target that reaches a mutation and is absent from this was never given the chance: one before it detected, or the run was cancelled.
    pub asked: Vec<MutantResult>,
    /// The budget the target that answered was given.
    pub timeout: Duration,
    /// Where that budget came from.
    pub timeout_source: TimeoutSource,
    /// Which targets could have noticed the mutation, and what removed the ones that could not.
    pub route: Route,
}

impl Judgement {
    /// What the run establishes about the mutant.
    #[must_use]
    pub const fn result(&self) -> &MutantResult {
        self.attempts.result()
    }

    /// Whether an expired budget was confirmed with the machine to itself.
    #[must_use]
    pub const fn retried(&self) -> bool {
        self.attempts.retried()
    }

    /// How long every execution of this mutant took together.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.attempts.duration()
    }
}

/// What a run waits for one mutant execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeout {
    /// A multiple of what the target's own baseline took, never below [`MINIMUM_DERIVED_TIMEOUT`].
    Auto,
    /// Exactly this long, whatever the baseline said.
    Fixed(Duration),
}

/// Where the budget one execution was given came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutSource {
    /// Somebody chose it.
    Configured,
    /// The run derived it from what the target's own baseline took.
    Derived,
}

impl TimeoutSource {
    /// The word a report and a recording use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Derived => "derived",
        }
    }
}

/// What a derived budget is a multiple of what the baseline took.
pub const TIMEOUT_MULTIPLE: u32 = 5;

/// The shortest budget a run derives, below which a machine's own noise decides the answer.
pub const MINIMUM_DERIVED_TIMEOUT: Duration = Duration::from_secs(30);

/// What a run waits when nothing was verified, so there is no baseline to be a multiple of.
pub const DEFAULT_MUTANT_TIMEOUT: Duration = Duration::from_secs(300);

/// The budget derived from one target's own baseline.
///
/// # Errors
/// Refuses when multiplying the baseline by [`TIMEOUT_MULTIPLE`] exceeds the largest [`Duration`].
pub fn derived(baseline: Duration) -> Result<Duration, SessionError> {
    baseline
        .checked_mul(TIMEOUT_MULTIPLE)
        .map(|duration| duration.max(MINIMUM_DERIVED_TIMEOUT))
        .ok_or(SessionError::DerivedTimeoutOverflow { baseline })
}

impl Timeout {
    /// The budget and where it came from, for a target whose baseline took `baseline`.
    ///
    /// # Errors
    /// Refuses when an automatic timeout derived from `baseline` exceeds the largest [`Duration`].
    pub fn of(self, baseline: Option<Duration>) -> Result<(Duration, TimeoutSource), SessionError> {
        match self {
            Self::Fixed(chosen) => Ok((chosen, TimeoutSource::Configured)),
            Self::Auto => Ok((
                match baseline {
                    Some(measured) => derived(measured)?,
                    None => DEFAULT_MUTANT_TIMEOUT,
                },
                TimeoutSource::Derived,
            )),
        }
    }
}

fn duration_ms(duration: Duration) -> Result<u64, SessionError> {
    u64::try_from(duration.as_millis())
        .map_err(|_overflow| SessionError::DurationMillisOverflow { duration })
}

/// Whether a target said anything at all.
const fn spoke(result: &MutantResult) -> bool {
    match result.conclusion {
        MutantConclusion::Inconclusive | MutantConclusion::StepLimitReached { .. } => false,
        MutantConclusion::NotRun
        | MutantConclusion::Killed
        | MutantConclusion::Survived
        | MutantConclusion::Waited
        | MutantConclusion::Unobserved
        | MutantConclusion::Errored => true,
    }
}

/// The strongest fact all selected targets jointly establish.
///
/// `Survived` is deliberately the weakest non-empty state: every selected target must reach it before the aggregate may stay there.
/// A kill decides the mutation; every other non-affirmative result prevents a survival claim, with a stable precedence so target order cannot decide the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetAggregate {
    Empty,
    Survived,
    NotRun,
    Inconclusive,
    StepLimitReached,
    Waited,
    Errored,
    Killed,
}

impl TargetAggregate {
    const fn of(outcome: crate::outcome::Outcome) -> Self {
        match outcome {
            crate::outcome::Outcome::Survived => Self::Survived,
            crate::outcome::Outcome::NotRun => Self::NotRun,
            crate::outcome::Outcome::Inconclusive => Self::Inconclusive,
            crate::outcome::Outcome::StepLimitReached => Self::StepLimitReached,
            crate::outcome::Outcome::Waited => Self::Waited,
            crate::outcome::Outcome::Errored => Self::Errored,
            crate::outcome::Outcome::Killed => Self::Killed,
        }
    }

    const fn priority(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Survived => 1,
            Self::NotRun => 2,
            Self::Inconclusive => 3,
            Self::StepLimitReached => 4,
            Self::Waited => 5,
            Self::Errored => 6,
            Self::Killed => 7,
        }
    }

    const fn include(self, outcome: crate::outcome::Outcome) -> Self {
        self.join(Self::of(outcome))
    }

    const fn join(self, other: Self) -> Self {
        if other.priority() > self.priority() {
            other
        } else {
            self
        }
    }

    const fn outcome(self) -> Option<crate::outcome::Outcome> {
        match self {
            Self::Empty => None,
            Self::Survived => Some(crate::outcome::Outcome::Survived),
            Self::NotRun => Some(crate::outcome::Outcome::NotRun),
            Self::Inconclusive => Some(crate::outcome::Outcome::Inconclusive),
            Self::StepLimitReached => Some(crate::outcome::Outcome::StepLimitReached),
            Self::Waited => Some(crate::outcome::Outcome::Waited),
            Self::Errored => Some(crate::outcome::Outcome::Errored),
            Self::Killed => Some(crate::outcome::Outcome::Killed),
        }
    }
}

#[cfg(kani)]
mod kani_laws {
    use super::{AttemptLedger, TargetAggregate, WaitedAttempt, aggregate_outcomes, retry_outcome};
    use crate::execute::{MutantConclusion, MutantResult};
    use crate::outcome::Outcome;
    use std::time::Duration;

    fn result(conclusion: MutantConclusion, duration: Duration) -> MutantResult {
        MutantResult {
            conclusion,
            duration,
            output: Vec::new(),
            ..MutantResult::apparatus_error("", String::new())
        }
    }

    fn symbolic_duration() -> Duration {
        let seconds = kani::any::<u64>();
        let nanoseconds = kani::any::<u32>();
        kani::assume(nanoseconds < 1_000_000_000);
        Duration::new(seconds, nanoseconds)
    }

    fn symbolic_outcome() -> Outcome {
        let index = kani::any::<u8>();
        kani::assume(index < 7);
        match index {
            0 => Outcome::NotRun,
            1 => Outcome::Killed,
            2 => Outcome::Survived,
            3 => Outcome::StepLimitReached,
            4 => Outcome::Waited,
            5 => Outcome::Inconclusive,
            _ => Outcome::Errored,
        }
    }

    fn symbolic_aggregate() -> TargetAggregate {
        let index = kani::any::<u8>();
        kani::assume(index < 8);
        match index {
            0 => TargetAggregate::Empty,
            1 => TargetAggregate::Survived,
            2 => TargetAggregate::NotRun,
            3 => TargetAggregate::Inconclusive,
            4 => TargetAggregate::StepLimitReached,
            5 => TargetAggregate::Waited,
            6 => TargetAggregate::Errored,
            _ => TargetAggregate::Killed,
        }
    }

    #[kani::proof]
    fn target_join_is_idempotent() {
        let value = symbolic_aggregate();
        kani::assert(
            value.join(value) == value,
            "njutest-law-assertion:join-idempotent",
        );
        kani::cover!(value == TargetAggregate::Empty, "njutest-law-branch:empty");
        kani::cover!(
            value == TargetAggregate::Killed,
            "njutest-law-branch:killed"
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn target_join_is_commutative() {
        let left = symbolic_aggregate();
        let right = symbolic_aggregate();
        kani::assert(
            left.join(right) == right.join(left),
            "njutest-law-assertion:join-commutative",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn target_join_is_associative() {
        let first = symbolic_aggregate();
        let second = symbolic_aggregate();
        let third = symbolic_aggregate();
        kani::assert(
            first.join(second).join(third) == first.join(second.join(third)),
            "njutest-law-assertion:join-associative",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn killed_is_absorbing() {
        let value = symbolic_aggregate();
        kani::assert(
            value.join(TargetAggregate::Killed) == TargetAggregate::Killed,
            "njutest-law-assertion:killed-right-absorbing",
        );
        kani::assert(
            TargetAggregate::Killed.join(value) == TargetAggregate::Killed,
            "njutest-law-assertion:killed-left-absorbing",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn survived_means_every_nonempty_target_survived() {
        let first = symbolic_outcome();
        let second = symbolic_outcome();
        let third = symbolic_outcome();
        let aggregate = aggregate_outcomes([first, second, third]);
        kani::assert(
            (aggregate == TargetAggregate::Survived)
                == (first == Outcome::Survived
                    && second == Outcome::Survived
                    && third == Outcome::Survived),
            "njutest-law-assertion:survived-iff-all",
        );
        kani::cover!(
            aggregate == TargetAggregate::Survived,
            "njutest-law-branch:survived"
        );
        kani::cover!(
            aggregate != TargetAggregate::Survived,
            "njutest-law-branch:not-survived"
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn retry_reconciliation_is_closed() {
        let repeated = symbolic_outcome();
        let cancelled = kani::any::<bool>();
        let reconciled = retry_outcome(repeated, cancelled);
        if cancelled {
            kani::assert(
                reconciled == Outcome::NotRun,
                "njutest-law-assertion:retry-cancelled",
            );
            kani::cover!(true, "njutest-law-branch:cancelled");
        } else {
            match repeated {
                Outcome::Killed | Outcome::StepLimitReached | Outcome::Waited => {
                    kani::assert(
                        reconciled == repeated,
                        "njutest-law-assertion:retry-preserved",
                    );
                    kani::cover!(true, "njutest-law-branch:preserved");
                }
                Outcome::NotRun | Outcome::Survived | Outcome::Inconclusive | Outcome::Errored => {
                    kani::assert(
                        reconciled == Outcome::Inconclusive,
                        "njutest-law-assertion:retry-downgraded",
                    );
                    kani::cover!(true, "njutest-law-branch:downgraded");
                }
            }
        }
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    #[kani::unwind(3)]
    fn attempt_ledger_is_nonempty_and_its_result_is_derived() {
        let retried = kani::any::<bool>();
        let first = result(MutantConclusion::Waited, Duration::ZERO);
        let ledger = if retried {
            let constructed = AttemptLedger::with_retry(
                WaitedAttempt(first),
                result(MutantConclusion::Killed, Duration::ZERO),
                false,
            );
            kani::assert(
                constructed.is_ok(),
                "njutest-law-assertion:attempt-retry-constructs",
            );
            let Ok(ledger) = constructed else {
                return;
            };
            ledger
        } else {
            AttemptLedger::single(first)
        };
        kani::assert(
            ledger.attempt_count() == 1 || ledger.attempt_count() == 2,
            "njutest-law-assertion:attempt-count-closed",
        );
        kani::assert(
            ledger.retried() == retried,
            "njutest-law-assertion:attempt-retried-derived",
        );
        kani::assert(
            ledger.result().outcome()
                == if retried {
                    Outcome::Killed
                } else {
                    Outcome::Waited
                },
            "njutest-law-assertion:attempt-result-derived",
        );
        kani::cover!(retried, "njutest-law-branch:retried");
        kani::cover!(!retried, "njutest-law-branch:single");
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    #[kani::unwind(3)]
    fn attempt_duration_is_the_checked_sum_of_every_execution() {
        let first_duration = symbolic_duration();
        let retry_duration = symbolic_duration();
        let ledger = AttemptLedger::with_retry(
            WaitedAttempt(result(MutantConclusion::Waited, first_duration)),
            result(MutantConclusion::Killed, retry_duration),
            false,
        );
        match first_duration.checked_add(retry_duration) {
            Some(expected) => {
                kani::assert(
                    ledger.is_ok(),
                    "njutest-law-assertion:duration-sum-constructs",
                );
                let Ok(actual) = ledger else {
                    return;
                };
                kani::assert(
                    actual.duration() == expected,
                    "njutest-law-assertion:duration-sum-exact",
                );
                kani::cover!(true, "njutest-law-branch:sum");
            }
            None => {
                kani::assert(
                    matches!(
                        ledger,
                        Err(crate::workspace::SessionError::ExecutionDurationOverflow)
                    ),
                    "njutest-law-assertion:duration-overflow-refused",
                );
                kani::cover!(true, "njutest-law-branch:overflow");
            }
        }
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    #[kani::unwind(3)]
    fn cancelled_retry_is_structurally_not_run() {
        let constructed = AttemptLedger::with_retry(
            WaitedAttempt(result(MutantConclusion::Waited, Duration::ZERO)),
            result(MutantConclusion::Killed, Duration::ZERO),
            true,
        );
        kani::assert(
            constructed.is_ok(),
            "njutest-law-assertion:cancelled-retry-constructs",
        );
        let Ok(ledger) = constructed else {
            return;
        };
        kani::assert(
            ledger.result().outcome() == Outcome::NotRun,
            "njutest-law-assertion:cancelled-retry-not-run",
        );
        kani::assert(
            ledger.retried(),
            "njutest-law-assertion:cancelled-retry-retained",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    #[kani::unwind(3)]
    fn cancellation_before_retry_cannot_create_a_retry() {
        let first = result(MutantConclusion::Waited, Duration::ZERO);
        let classified = super::InitialAttempt::classify(first, true);
        kani::assert(
            matches!(&classified, super::InitialAttempt::Final(_)),
            "njutest-law-assertion:pre-retry-cancel-final",
        );
        let super::InitialAttempt::Final(observed) = classified else {
            return;
        };
        let ledger = AttemptLedger::single(observed);
        kani::assert(
            !ledger.retried(),
            "njutest-law-assertion:pre-retry-cancel-single",
        );
        kani::assert(
            ledger.attempt_count() == 1,
            "njutest-law-assertion:pre-retry-count-one",
        );
        kani::assert(
            ledger.result().outcome() == Outcome::Waited,
            "njutest-law-assertion:pre-retry-result-retained",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    #[kani::unwind(3)]
    fn equal_outcomes_choose_the_canonical_target_in_every_order() {
        let mut later = result(MutantConclusion::Survived, Duration::ZERO);
        later.target = String::from("z");
        let mut earlier = result(MutantConclusion::Survived, Duration::ZERO);
        earlier.target = String::from("a");

        let forward = [later.clone(), earlier.clone()];
        let reverse = [earlier, later];
        let forward_result = super::aggregate_result(&forward);
        kani::assert(
            forward_result.is_some(),
            "njutest-law-assertion:tie-forward-present",
        );
        let Some(forward_result) = forward_result else {
            return;
        };
        let reverse_result = super::aggregate_result(&reverse);
        kani::assert(
            reverse_result.is_some(),
            "njutest-law-assertion:tie-reverse-present",
        );
        let Some(reverse_result) = reverse_result else {
            return;
        };
        kani::assert(
            forward_result.target == "a",
            "njutest-law-assertion:tie-forward-canonical",
        );
        kani::assert(
            reverse_result.target == "a",
            "njutest-law-assertion:tie-reverse-canonical",
        );
        kani::cover!(true, "njutest-law-reached");
    }
}

fn aggregate_outcomes(
    outcomes: impl IntoIterator<Item = crate::outcome::Outcome>,
) -> TargetAggregate {
    outcomes
        .into_iter()
        .fold(TargetAggregate::Empty, TargetAggregate::include)
}

/// The verdict of one mutant over the targets it was put to: a target whose harness ran no tests is silent, and a silent target contributes no reaching test, so it cannot turn a survived verdict inconclusive.
/// When every target was silent the aggregate says so instead of inventing survival.
fn verdict_result(results: &[MutantResult]) -> Option<MutantResult> {
    let speaking: Vec<&MutantResult> = results.iter().filter(|result| spoke(result)).collect();
    if speaking.is_empty() {
        return aggregate_result(results).cloned();
    }
    let outcome = aggregate_outcomes(speaking.iter().map(|result| result.outcome())).outcome()?;
    speaking
        .into_iter()
        .find(|result| result.outcome() == outcome)
        .cloned()
}

fn aggregate_result(results: &[MutantResult]) -> Option<&MutantResult> {
    let outcome = aggregate_outcomes(results.iter().map(MutantResult::outcome)).outcome()?;
    results
        .iter()
        .filter(|result| result.outcome() == outcome)
        .min_by(|left, right| left.target.cmp(&right.target))
}

/// Reconciles the result of the isolated retry with the wait that caused it.
const fn retry_outcome(
    repeated: crate::outcome::Outcome,
    cancelled: bool,
) -> crate::outcome::Outcome {
    if cancelled {
        return crate::outcome::Outcome::NotRun;
    }
    match repeated {
        crate::outcome::Outcome::Killed
        | crate::outcome::Outcome::StepLimitReached
        | crate::outcome::Outcome::Waited => repeated,
        crate::outcome::Outcome::Survived
        | crate::outcome::Outcome::Inconclusive
        | crate::outcome::Outcome::Errored
        | crate::outcome::Outcome::NotRun => crate::outcome::Outcome::Inconclusive,
    }
}

const fn unreached() -> MutantResult {
    MutantResult {
        entered: None,
        conclusion: MutantConclusion::NotRun,
        target: String::new(),
        exit_code: crate::runner::EXIT_CODE_UNAVAILABLE,
        duration: Duration::ZERO,
        output: Vec::new(),
        protocol: Protocol::Unanswered,
        summary: None,
        signal: None,
        failed_tests: Vec::new(),
        passed_tests: Vec::new(),
        ignored_tests: Vec::new(),
        leader: None,
    }
}

/// The name a target id takes, re-exported so a caller can build one without knowing the shape.
#[must_use]
pub fn target_name(package: &str, kind: TargetKind, name: &str) -> String {
    target_id(package, kind, name)
}

#[cfg(test)]
mod tests {
    use njutest_devkit::result::{ResultState::Returned, result_state};

    use super::{AttemptLedger, InitialAttempt, WaitedAttempt, aggregate_outcomes, retry_outcome};
    use crate::execute::{MutantConclusion, MutantResult, StepLimitNotice};
    use crate::outcome::Outcome;
    use std::time::Duration;

    fn result(conclusion: MutantConclusion, duration: Duration) -> MutantResult {
        MutantResult {
            entered: None,
            conclusion,
            target: "target".to_owned(),
            exit_code: crate::runner::EXIT_CODE_UNAVAILABLE,
            duration,
            output: Vec::new(),
            protocol: crate::execute::Protocol::Unanswered,
            summary: None,
            signal: None,
            failed_tests: Vec::new(),
            passed_tests: Vec::new(),
            ignored_tests: Vec::new(),
            leader: None,
        }
    }

    #[test]
    fn no_nonaffirmative_target_can_be_hidden_by_a_survivor() {
        for outcome in [
            Outcome::StepLimitReached,
            Outcome::Waited,
            Outcome::Errored,
            Outcome::Inconclusive,
        ] {
            for ordered in [[Outcome::Survived, outcome], [outcome, Outcome::Survived]] {
                assert_eq!(
                    aggregate_outcomes(ordered).outcome(),
                    Some(outcome),
                    "{ordered:?}"
                );
            }
        }
    }

    #[test]
    fn target_order_cannot_change_the_aggregate() {
        for first in Outcome::ALL {
            for second in Outcome::ALL {
                let forward = aggregate_outcomes([first, second]);
                let reverse = aggregate_outcomes([second, first]);
                assert_eq!(forward, reverse, "{first:?}, {second:?}");
            }
        }
    }

    #[test]
    fn three_target_rotation_cannot_change_the_aggregate() {
        for first in Outcome::ALL {
            for second in Outcome::ALL {
                for third in Outcome::ALL {
                    let all = aggregate_outcomes([first, second, third]);
                    let rotated = aggregate_outcomes([second, third, first]);
                    assert_eq!(all, rotated, "{first:?}, {second:?}, {third:?}");
                }
            }
        }
    }

    #[test]
    fn retry_preserves_only_a_reproduced_wait_a_kill_or_a_verified_step_boundary() {
        for (repeated, expected) in [
            (Outcome::Killed, Outcome::Killed),
            (Outcome::StepLimitReached, Outcome::StepLimitReached),
            (Outcome::Waited, Outcome::Waited),
            (Outcome::Survived, Outcome::Inconclusive),
            (Outcome::Inconclusive, Outcome::Inconclusive),
            (Outcome::Errored, Outcome::Inconclusive),
            (Outcome::NotRun, Outcome::Inconclusive),
        ] {
            assert_eq!(retry_outcome(repeated, false), expected, "{repeated:?}");
            assert_eq!(
                retry_outcome(repeated, true),
                Outcome::NotRun,
                "cancellation dominates {repeated:?}"
            );
        }
    }

    #[test]
    fn attempt_ledger_is_nonempty_and_derives_its_result_and_retry_state() {
        let single =
            AttemptLedger::single(result(MutantConclusion::Killed, Duration::from_secs(1)));
        assert_eq!(single.attempt_count(), 1);
        assert!(!single.retried());
        assert_eq!(single.result().outcome(), Outcome::Killed);

        let notice = StepLimitNotice::specimen();
        let retried = AttemptLedger::with_retry(
            WaitedAttempt(result(MutantConclusion::Waited, Duration::from_secs(1))),
            result(
                MutantConclusion::StepLimitReached {
                    notice: notice.clone(),
                },
                Duration::from_secs(2),
            ),
            false,
        );
        assert_eq!(result_state(&retried), Returned, "ledger: {retried:?}");
        let Ok(retried) = retried else { return };
        assert_eq!(retried.attempt_count(), 2);
        assert!(retried.retried());
        assert_eq!(retried.result().outcome(), Outcome::StepLimitReached);
        assert_eq!(retried.result().step_notice(), Some(&notice));
        assert_eq!(retried.duration(), Duration::from_secs(3));
    }

    #[test]
    fn attempt_ledger_fails_closed_when_its_exact_duration_overflows() {
        let ledger = AttemptLedger::with_retry(
            WaitedAttempt(result(MutantConclusion::Waited, Duration::MAX)),
            result(MutantConclusion::Killed, Duration::from_nanos(1)),
            false,
        );
        assert!(matches!(
            ledger,
            Err(crate::workspace::SessionError::ExecutionDurationOverflow)
        ));
    }

    #[test]
    fn cancellation_reconciles_the_retry_without_detaching_step_evidence() {
        let ledger = AttemptLedger::with_retry(
            WaitedAttempt(result(MutantConclusion::Waited, Duration::ZERO)),
            result(
                MutantConclusion::StepLimitReached {
                    notice: StepLimitNotice::specimen(),
                },
                Duration::ZERO,
            ),
            true,
        );
        assert_eq!(result_state(&ledger), Returned, "ledger: {ledger:?}");
        let Ok(ledger) = ledger else { return };
        assert_eq!(ledger.result().outcome(), Outcome::NotRun);
        assert!(ledger.result().step_notice().is_none());
    }

    #[test]
    fn cancellation_before_a_retry_keeps_the_single_observed_attempt() {
        let first = result(MutantConclusion::Waited, Duration::from_secs(1));
        let classified = InitialAttempt::classify(first, true);
        assert!(
            matches!(classified, InitialAttempt::Final(_)),
            "cancellation must suppress the isolated retry"
        );
        let InitialAttempt::Final(observed) = classified else {
            return;
        };
        let ledger = AttemptLedger::single(observed);
        assert_eq!(ledger.attempt_count(), 1);
        assert!(!ledger.retried());
        assert_eq!(ledger.result().outcome(), Outcome::Waited);
        assert_eq!(ledger.duration(), Duration::from_secs(1));
    }
}
