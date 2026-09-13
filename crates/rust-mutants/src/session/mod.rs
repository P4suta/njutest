// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A prepared workspace: every accepted mutant instrumented into one build, and the test binaries that build produced.

pub(crate) mod prepare;
mod route;
mod verify;

pub use prepare::{prepare, rewrite_needed};
use prepare::{pristine, selection};
use verify::verify;
pub use verify::{Baseline, Verified};

pub use route::{
    Asked, BRANCH_NEVER_TAKEN, Discharge, Fallback, NEVER_INFECTED, Reaches, Route, Routing, Timing,
};

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use crate::EngineError;
use crate::catalog::{Catalog, Mutant};
use crate::discover::{self, DiscoverOptions, FileReport, SkipClaim};
use crate::execute::{self, Context, ExecRequest, MutantResult, TargetKind, TestTarget, target_id};
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
pub struct Locator {
    /// The workspace-relative path with forward slashes.
    pub path: String,
    /// The item, as a reader writes it. A suffix is enough: `clamp`, `Type::method`, `mod::path::Type::method`.
    pub item: String,
    /// The rule's name.
    pub rule: String,
    /// The bytes the edit replaces, as text.
    pub original: String,
    /// The line, as a hint that separates two mutations the rest would name together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// How many mutations this names, when it names a set of them on one reason.
    ///
    /// Without it a locator names one mutation and naming several is an error,
    /// because a reason written about one mutation cannot be checked against
    /// another. With it the claim is about every one of them: the count says
    /// how many the reason was written for, so a mutation added or removed at
    /// the same place stops the claim rather than joining it, and the outcome
    /// is required of each, so a claim covering three cannot go on standing
    /// once one of them is killed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// Why a locator named no one mutation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LocateError {
    /// The catalog holds no such mutation.
    #[error("no mutation of this catalog is the one described")]
    Nothing,
    /// The catalog holds more than one, and the line did not separate them.
    #[error("{} mutations of this catalog are the one described: {}", display_ids.len(), display_ids.join(", "))]
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

/// What preparing does about a target whose baseline does not pass with nothing active.
///
/// A suite that already fails cannot tell a mutation from what was failing
/// before it: every mutant put to that target comes back killed, and not one
/// of those kills is about a mutation. So neither answer here is "measure it
/// anyway".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failing {
    /// End the preparation and name the target. A person who asked for a measurement wants to hear that there was nothing to measure.
    #[default]
    Refuse,
    /// Report it and leave the target out of every route, so a caller with a verdict of its own can give it.
    ///
    /// [`Session::verified`] then names every target and what its baseline
    /// came to, and the record carries `baseline-not-passing` for each one
    /// left out. What only such a target could have noticed is reported as a
    /// mutation nothing reached, which is what the run can honestly say about
    /// it.
    Exclude,
}

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
    /// Exactly these rules, by name. Empty means the tier.
    pub operators: Vec<String>,
    /// Patterns a file must match to be mutable.
    pub include: Vec<Pattern>,
    /// Patterns that remove a file again.
    pub exclude: Vec<Pattern>,
    /// The member packages to mutate. Empty means every member.
    pub packages: Vec<String>,
    /// The places a reviewer configured the run to pass over, each with the reason they gave.
    pub skips: Vec<discover::SkipRule>,
    /// Where to remember successful measurements of this exact tree.
    ///
    /// Coverage is keyed by the compiled closure, manifests and toolchain.
    /// The passing instrumented baseline is keyed more strictly by the whole
    /// snapshot and its exact instrumentation and execution inputs, and is
    /// reused only when the built target artifacts are byte-identical. A
    /// mutation changes none of those inputs. `None` measures both again every
    /// time; this is what `--no-cache` supplies. A failing baseline is never
    /// remembered.
    pub measurements: Option<PathBuf>,
    /// Run every test target once with nothing active, and refuse to hand back a session whose instrumented baseline does not pass.
    pub verify: bool,
    /// Ask the guards, on that same run, which of each target's tests reached them, so a mutation is put to the tests that reached it rather than to every test of every target that did.
    ///
    /// It costs the run nothing it was not already spending: the baseline is
    /// one process per target either way. Turning it off is how a caller asks
    /// for the answer a run with nothing removed would give.
    pub touch: bool,
    /// Build and run the tree once with coverage instrumentation, so a mutant is only ever run against the targets that reached it.
    pub coverage: bool,
    /// Ask the compiler which mutations change nothing outside the branch they sit in, so a target that never ran that branch is not run against them.
    ///
    /// A proof without a measurement removes nothing: the lemma is the
    /// compiler's and the premise is the coverage layer's, and a caller with
    /// its own coverage discharges with [`crate::prove::discharges`].
    pub branch_proofs: bool,
    /// What to do about a target whose baseline does not pass.
    pub failing: Failing,
    /// How many validation rounds before falling back to bisection.
    pub max_rounds: u32,
    /// How long a build may take.
    pub build_timeout: Option<Duration>,
    /// How long one mutant execution may take, when the caller does not say.
    pub mutant_timeout: Timeout,
    /// Run a library's documented examples as a target of their own.
    ///
    /// A documented example is a test the project wrote, and a mutation only
    /// one of them can notice is a mutation nothing else in the suite covers.
    /// It costs a `cargo test --doc` for every mutation no other target
    /// noticed, which is why it is a switch.
    pub doctests: bool,
    /// What the project is compiled as: its features, target, profile, and how many jobs cargo may use.
    pub build: crate::cargo::BuildConfig,
    /// The arguments every test binary of this session is started with, the baseline included.
    ///
    /// A harness flag is how this project's suite runs — one thread, the
    /// ignored tests included, output shown — and the baseline is one run of
    /// that suite. A baseline taken one way and mutations measured another
    /// compares two suites: a mutation could be noticed by a test the
    /// baseline never ran, which is a kill nothing vouched for, and a
    /// mutation's budget is a multiple of a duration measured under other
    /// flags.
    ///
    /// An execution that names its own arguments runs with those instead, so
    /// a caller asking one question of one mutant is not fighting the
    /// session; an execution that names none runs with these.
    pub harness_args: Vec<String>,
    /// Targets never to start, by the id a report names them with.
    ///
    /// A target whose tests are about the text of what the compiler said —
    /// a `trybuild` or a snapshot suite — fails under instrumentation for a
    /// reason that is not the mutation, and would fail the verification of
    /// every run. Naming it here leaves it out of the build's answer and says
    /// so as a limitation, which is a decision somebody made rather than a
    /// result nobody can read.
    pub skip_targets: Vec<String>,
    /// Which mutants to place in the compiled tree when a later run is already
    /// known to ask about only part of the catalog.
    ///
    /// Discovery and the catalog remain whole: identities, prefixes, skips,
    /// and report positions are therefore resolved against exactly the same
    /// catalog as an unfiltered run. Only compiler validation and
    /// instrumentation are narrowed. A candidate outside this filter has not
    /// been accepted or refused by the compiler; a run using the same filter
    /// records it as `not_run/unselected`.
    pub validation_filter: Option<crate::run::Filter>,
}

impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            tier: Tier::Balanced,
            operators: Vec::new(),
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
            mutant_timeout: Timeout::default(),
            doctests: true,
            build: crate::cargo::BuildConfig::default(),
            skip_targets: Vec::new(),
            validation_filter: None,
        }
    }
}

/// One mutant execution to make.
///
/// Built rather than spelled as a literal, so that a field added later is a
/// method a caller may ignore instead of a compile error in every caller.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Request {
    /// The mutant, by full identity or by any prefix of at least four hex characters that names exactly one.
    pub mutant: String,
    /// The target to run it against. `None` runs every target until one kills it, which is what "does any test catch this?" means.
    pub target: Option<String>,
    /// One test to run, by its libtest path. `None` runs the whole target.
    pub test: Option<String>,
    /// Further arguments for the harness.
    pub args: Vec<String>,
    /// How long the process may take. `None` uses the session's default.
    pub timeout: Option<Duration>,
}

impl Request {
    /// A request for one mutant, named by its identity or by a prefix that names exactly one.
    #[must_use]
    pub fn new(mutant: impl Into<String>) -> Self {
        Self {
            mutant: mutant.into(),
            ..Self::default()
        }
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
    /// The catalog indices compiler validation was asked about. Every other
    /// index is an explicitly unvalidated candidate, never an accepted one.
    eligible: BTreeSet<u32>,
    targets: Vec<TestTarget>,
    scratch: PathBuf,
    /// How many executions this session has started, which is what names each one's own temporary directory.
    executions: std::sync::atomic::AtomicU64,
    mutant_timeout: Timeout,
    /// The arguments every test binary of this session is started with, unless one execution names its own.
    harness_args: Vec<String>,
    /// The files as they were before instrumentation, so a position can be counted in the file a person would open rather than in the rewrite.
    sources: BTreeMap<String, Vec<u8>>,
    /// Which package each mutant belongs to.
    packages: BTreeMap<u32, String>,
    /// The item each mutant sits in, by catalog index.
    items: BTreeMap<u32, String>,
    /// The branch proof of every mutant that has one, by catalog index.
    proofs: BTreeMap<u32, crate::syntax::branch::Proof>,
    reached: crate::reach::Reached,
    /// What the one run of every target with nothing active established, empty when nothing was verified.
    verified: Verified,
    /// What each set of tests a route named answers on its own, so the question is put once however many mutants that set covers.
    filtered: std::sync::Mutex<Established>,
    /// How many tests this session started to establish those answers, counted as they are started rather than as they are remembered.
    established: std::sync::atomic::AtomicU64,
    /// What the tree gained or lost while the proof layers ran, which is what a test wrote before anything was instrumented.
    written_by_a_test: Vec<Drift>,
    /// The digest of the pristine sources every unit of this build compiled.
    closure: String,
    /// The digest of the manifests, the lock file, and the cargo configuration the build read.
    manifests: String,
}

/// What narrowing a target's tests left: the ones that could still notice the mutation, or the proof that took the last of them away.
enum Narrowed {
    /// The target stays in the route, asked for these tests.
    Reaching(Reaches),
    /// Nothing of the target could have noticed, and this names what says so.
    Discharged(&'static str),
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
    ///
    /// A file with no candidate in it is here and a file that is not in the
    /// workspace is not, which is the difference a command narrowing to a path
    /// has to be able to tell: a run narrowed to a name nobody wrote measures
    /// nothing and reports that nothing was missed.
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

    /// The digest of the pristine sources every unit of this build compiled.
    ///
    /// What the tests say about a mutation can only change when something the
    /// test binaries were built from changes. The tree's own digest is a much
    /// larger set than that — a note beside the code, a workflow file, a crate
    /// this run never compiled — and keying a remembered outcome on it throws
    /// away every answer whenever any of them moves.
    #[must_use]
    pub fn closure(&self) -> &str {
        &self.closure
    }

    /// The digest of the manifests, the lock file, and the cargo configuration the build read.
    ///
    /// These are what decide which dependencies a compilation resolves and
    /// what flags it is given, and none of them is a source file, so the
    /// closure of compiled sources does not cover them.
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
        let text = std::str::from_utf8(source).ok()?;
        Some(LineIndex::new(text).position(text, mutant.candidate.span.start))
    }

    /// The package a mutant belongs to.
    #[must_use]
    pub fn package_of(&self, index: u32) -> Option<&str> {
        self.packages.get(&index).map(String::as_str)
    }

    /// The packages this session was told to measure, which is where a reader looks for a test to write.
    ///
    /// Each is named once and they are in one order, so what a recording says
    /// about a catalog is what the next recording of it says.
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
    /// A locator says where a mutation is and what it edits rather than what
    /// its identity is, so it survives an edit anywhere else in the file: an
    /// identity is minted from the whole file's digest, and any change to the
    /// file renames every mutant in it. The line is a hint that separates two
    /// mutations a locator would otherwise name together, never the thing that
    /// identifies one: code moves down a file and the claim about it does not
    /// stop being true.
    ///
    /// # Errors
    /// Returns [`LocateError::Nothing`] when the catalog holds no such
    /// mutation and [`LocateError::Several`] when it holds more than one and
    /// the line does not separate them.
    pub fn locate(&self, locator: &Locator) -> Result<&Mutant, LocateError> {
        match self.locate_all(locator)?.as_slice() {
            [one] => Ok(one),
            several => Err(LocateError::Several {
                display_ids: several
                    .iter()
                    .map(|mutant| mutant.display_id.clone())
                    .collect(),
            }),
        }
    }

    /// Every mutation of the catalog a locator names, which is one unless it states a count.
    ///
    /// A locator that states no count names one mutation, because a reason
    /// written about one mutation says nothing about another that happens to
    /// share a path, an item, a rule and the bytes it replaces. A locator that
    /// states a count names exactly that many, and the caller has to require
    /// its claim of every one of them: the count fixes which mutations are
    /// spoken about, and the outcome fixes what is said.
    ///
    /// # Errors
    /// Returns [`LocateError::Nothing`] when the catalog holds no such
    /// mutation, [`LocateError::Several`] when it holds more than one and
    /// neither the line nor a count separates them, and
    /// [`LocateError::Counted`] when a count is stated and another number of
    /// them is what the catalog holds.
    pub fn locate_all(&self, locator: &Locator) -> Result<Vec<&Mutant>, LocateError> {
        let matching: Vec<&Mutant> = self
            .catalog
            .mutants()
            .iter()
            .filter(|mutant| {
                mutant.candidate.path == locator.path
                    && mutant.candidate.rule.name == locator.rule
                    && mutant.candidate.original == locator.original.as_bytes()
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
                .map(|mutant| mutant.display_id.clone())
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
    ///
    /// A report names a file, a line, and the bytes an edit replaces; showing
    /// somebody the edit needs the file those bytes were cut from, and the
    /// snapshot holds the rewrite rather than the original.
    #[must_use]
    pub fn source(&self, path: &str) -> Option<&[u8]> {
        self.sources.get(path).map(Vec::as_slice)
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
    ///
    /// A target during which no statement of the named body ran cannot have
    /// observed this mutation.
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

    /// What the one run of every target with nothing active established, target by target.
    ///
    /// A caller that reports on the tree it was handed reads this rather than
    /// inferring it from what came back: a target that ran, what it came to,
    /// how long it took and how many tests it has are all answers of that one
    /// run, and a run that refused nothing still has targets in here that did
    /// not pass.
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
            .map_or(1, |baseline| baseline.tests)
            .max(1)
    }

    /// How many tests this session started to establish that a set of them answers on its own.
    ///
    /// A narrowed execution rests on the set having passed with nothing
    /// active, and putting that question is work no mutation asked for. It is
    /// counted as it is started rather than as it is remembered, so two
    /// workers that raced to establish the same set are two runs in the
    /// ledger, which is what they were.
    #[must_use]
    pub fn established_tests(&self) -> u64 {
        self.established.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether any measured target reached this mutant: `None` when the measurement says nothing about the place, so nothing is proved either way.
    #[must_use]
    pub fn reaches(&self, mutant: &Mutant) -> Option<bool> {
        self.covering(mutant).map(|targets| !targets.is_empty())
    }

    /// The targets an execution of this mutant runs, or nothing when nothing narrows it.
    ///
    /// It is [`Route::narrowing`] of [`Session::route`] and nothing else: a
    /// narrowing decided anywhere else would run fewer targets than the route
    /// a report and a recording show, and a survivor it reported would be one
    /// nobody measured.
    fn covering(&self, mutant: &Mutant) -> Option<Vec<String>> {
        self.route(mutant).narrowing()
    }

    /// The documentation targets a mutation is routed to, which is by the file it is in.
    ///
    /// A library's documented examples are compiled by rustdoc while cargo
    /// runs them, so a coverage build never instruments them and a measurement
    /// says nothing about what they reached. Routing them by file is wider
    /// than a region would be, which is the direction a fallback must go. A
    /// library with no examples answers nothing, so nothing is routed to it.
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
    /// A request that names a timeout is a caller who chose one. Otherwise
    /// the session's own policy decides, which by default is a multiple of
    /// what that target's baseline took.
    #[must_use]
    pub fn timeout_for(&self, request: &Request, target: &str) -> (Duration, TimeoutSource) {
        request.timeout.map_or_else(
            || self.mutant_timeout.of(self.baseline(target)),
            |chosen| (chosen, TimeoutSource::Configured),
        )
    }

    /// How long one target's own baseline took, when it was verified.
    ///
    /// A timeout a run derives is a multiple of this rather than a number a
    /// person guessed: a machine that runs the suite in a minute and one that
    /// takes ten are two machines, and a budget calibrated on one is a wrong
    /// answer on the other.
    #[must_use]
    pub fn baseline(&self, target: &str) -> Option<Duration> {
        self.verified
            .targets
            .get(target)
            .map(|baseline| baseline.duration)
    }

    /// What one target costs: how long its own baseline took, and how many tests that was the cost of.
    ///
    /// What an unverified target costs is a decision this engine makes, not
    /// one a caller should have to spell for itself: two places answering the
    /// same question is two places that can drift apart, and nothing would
    /// notice.
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
            .map(|baseline| baseline.duration)
            .max()
            .unwrap_or(Duration::from_secs(1))
    }

    /// Which targets could notice this mutation, and what the answer rests on.
    ///
    /// It is a question, not an instruction: [`Session::exec`] narrows to the
    /// covering targets exactly as it did before, and nothing here removes an
    /// execution until a proof layer says so.
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
    ///
    /// [`Session::judge`] decides the route once and narrows by that one; this
    /// is for the callers that did not, and it asks for the same route rather
    /// than for the targets alone. Deriving the targets in one place and the
    /// tests in another is how the two come to disagree, and a disagreement
    /// between them is a process that runs a test the route did not name or
    /// misses one it did.
    fn chosen(&self, request: &Request, mutant: &Mutant, asking: Asking) -> Chosen {
        Chosen::of(request, &self.route(mutant), asking)
    }

    /// The tests of `target` this execution runs, or nothing when it runs every test the target has.
    fn filtering(
        &self,
        target: &TestTarget,
        chosen: &Chosen,
        cancel: &Cancel,
    ) -> Option<Vec<String>> {
        let named = chosen.tests_of(&target.id)?;
        self.usable(target, named, cancel)?;
        Some(named.to_vec())
    }

    /// How long the named tests of `target` take on their own with nothing active, or nothing when running them on their own is not the same question as running the target.
    ///
    /// A route narrows a target to the tests that reached the mutation, and a
    /// filtered process only answers about the mutation if the same filter
    /// passes without it. Tests share process state — a `static`, a temporary
    /// directory, an ordering one of them relies on — and a set that only
    /// passes beside its neighbours would report a kill that is about the
    /// neighbours. So the set is put once, with nothing active, and remembered:
    /// the mutants of one function are covered by one set, so the cost is one
    /// process for all of them.
    ///
    /// A set that does not pass, or that runs a different number of tests than
    /// it names, takes its target off test routing for this mutation and runs
    /// every test of it.
    fn usable(&self, target: &TestTarget, tests: &[String], cancel: &Cancel) -> Option<Duration> {
        let key = (target.id.clone(), tests.to_vec());
        if let Ok(known) = self.filtered.lock()
            && let Some(answer) = known.get(&key)
        {
            return *answer;
        }
        let context = Context {
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: None,
            touch: None,
            profile: None,
        };
        let (timeout, _source) = self.mutant_timeout.of(self.baseline(&target.id));
        let request = ExecRequest::new(target)
            .with_tests(tests.to_vec())
            .with_timeout(Some(timeout))
            .with_scratch(self.exec_scratch());
        let result = execute::exec(&request, &context, cancel, &self.workspace.trace);
        let _asked = self.established.fetch_add(
            u64::try_from(tests.len()).unwrap_or(0),
            std::sync::atomic::Ordering::Relaxed,
        );
        if cancel.is_cancelled() {
            return None;
        }
        let ran = usize::try_from(result.tests_run.unwrap_or(0)).unwrap_or(0);
        let answer = (result.outcome == crate::outcome::Outcome::Survived && ran == tests.len())
            .then_some(result.duration);
        if answer.is_none() {
            let why = if ran == tests.len() {
                "did not pass on its own".to_owned()
            } else {
                format!("named {} tests and ran {ran} of them", tests.len())
            };
            self.workspace.trace.note(
                TEST_ROUTING_UNSOUND,
                &format!(
                    "{}: the set {} ({}), so every test of it runs instead of the ones a \
                     measurement named",
                    target.id,
                    why,
                    result.outcome.name()
                ),
            );
        }
        if let Ok(mut known) = self.filtered.lock() {
            let _remembered = known.insert(key, answer);
        }
        answer
    }

    /// The same route with every target a proof removes moved out of what could notice the mutation.
    ///
    /// Only a target the measurement actually read can be discharged: one
    /// whose profile could not be read is in the route because nothing is
    /// known about it, and a proof that rested on its silence would rest on
    /// the measurement's failure.
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
    ///
    /// Only a target the measurement actually read can be discharged by the
    /// branch proof: one whose profile could not be read is in the route
    /// because nothing is known about it, and a proof resting on its silence
    /// would rest on the measurement's failure. The guards answer about the
    /// mutants whose two branches the tree they were in compares, and a
    /// mutant never asked about is one they say nothing about.
    fn proof_against(&self, mutant: &Mutant, target: &str) -> Option<&'static str> {
        if self.never_took_the_branch(mutant, target) {
            return Some(BRANCH_NEVER_TAKEN);
        }
        if self.never_differed(mutant.index, target) {
            return Some(NEVER_INFECTED);
        }
        None
    }

    /// Whether every run of this target's guard answered the same on both of its branches.
    ///
    /// The guard holds the mutation and what it replaces, and where the
    /// compiler vouched that evaluating either runs none of the program's
    /// code, the baseline evaluated both and recorded every time they parted.
    /// A target whose record names this mutant nowhere ran a program that
    /// answered what the unmutated one answers, wherever it looked. Only a
    /// target the measurement read can say so: one absent from the record is
    /// one nothing is known about, and a target whose guard the tree does not
    /// compare says nothing about it either.
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
    ///
    /// Two records narrow, and a test has to survive both. One that never
    /// entered the body a branch proof names ran a condition the mutation
    /// leaves false as well. One that ran a compared guard and never saw its
    /// two branches part ran a program indistinguishable from the unmutated
    /// one. A target *nothing* of which survives is discharged, and the proof
    /// named is the record that emptied it.
    ///
    /// A target asked for as a whole is left alone. That is the shape a route
    /// takes when every test of the target reached the site — the same tests
    /// either way, and one fewer filtered set to establish — or when a touch
    /// could not be attributed at all, which is a fact about the measurement
    /// rather than about the tests. Narrowing the first would be sound and
    /// narrowing the second would not, and the route does not say which it is;
    /// what is not narrowed here is still discharged whole by
    /// [`Self::proof_against`] when no test of the target saw anything.
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
    ///
    /// A test that ran a compared guard and never saw the mutation answer
    /// anything but what it replaces ran a program indistinguishable from the
    /// unmutated one, so there was nothing there for it to notice.
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
    ///
    /// Two measurements can establish it and either will do. The marker the
    /// instrumenter wrote at the body's first statement is exact: it either
    /// ran or it did not. A coverage region beginning inside the body is the
    /// older premise, and it is what a body no marker could go into still
    /// rests on — the record only ever names markers the tree carries the
    /// call for, so a body inside a guard's own site falls to the region
    /// rather than to silence.
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
    #[must_use]
    pub fn root_name(&self) -> String {
        self.workspace
            .root()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The mutant a prefix names.
    ///
    /// # Errors
    /// [`SessionError::UnknownMutant`] when no mutant matches, when several
    /// do, or when the prefix is too short to be worth resolving.
    pub fn resolve(&self, prefix: &str) -> Result<&Mutant, EngineError> {
        self.catalog.resolve_prefix(prefix).map_err(|error| {
            EngineError::from(SessionError::UnknownMutant {
                message: error.to_string(),
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

    /// A temporary directory of this execution's own, so two executions at once
    /// cannot meet in one another's files.
    ///
    /// A consumer may run mutants in parallel across the targets one build
    /// produced, and two test processes sharing a temporary directory can fail
    /// one another. Falling back to the session's own directory keeps a run
    /// that cannot make the directory going, at the isolation it had before.
    fn exec_scratch(&self) -> PathBuf {
        let at = self
            .executions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let own = self.scratch.join(format!("exec-{at}"));
        if std::fs::create_dir_all(&own).is_ok() {
            own
        } else {
            self.scratch.clone()
        }
    }

    /// Runs one mutant and reports what the tests said.
    ///
    /// # Errors
    /// [`SessionError::UnknownMutant`], [`SessionError::UnknownTarget`], and
    /// [`SessionError::NoTargets`].
    pub fn exec(&self, request: &Request, cancel: &Cancel) -> Result<MutantResult, EngineError> {
        let mutant = self.executable(&request.mutant)?;
        let chosen = self.chosen(request, mutant, Asking::Anything);
        self.execute(
            request,
            Running {
                alone: false,
                chosen: &chosen,
                mutant,
            },
            cancel,
        )
    }

    /// What one mutant is, decided: executed, and when a budget expired, confirmed with the machine to itself.
    ///
    /// A budget is a multiple of what a target's baseline took, and a
    /// duration measured while three other test processes were running is a
    /// fact about the load rather than about the mutation. One expired budget
    /// buys one quiet measurement, and what that measurement observes is what
    /// stands: a second timeout is a timeout, and anything else is a run that
    /// could not decide.
    ///
    /// # Errors
    /// [`SessionError::UnknownMutant`], [`SessionError::UnknownTarget`], and
    /// [`SessionError::NoTargets`].
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
        let first = quiet.shared(|| self.execute(request, running(false), cancel))?;
        let (timeout, timeout_source) = self.timeout_for(request, &first.target);
        let judgement =
            if first.outcome != crate::outcome::Outcome::TimedOut || cancel.is_cancelled() {
                Judgement {
                    result: first.clone(),
                    attempts: vec![first],
                    retried: false,
                    timeout,
                    timeout_source,
                    route,
                }
            } else {
                let mut again = quiet.alone(|| self.execute(request, running(true), cancel))?;
                if again.outcome != crate::outcome::Outcome::TimedOut && !again.outcome.detected() {
                    again.outcome = crate::outcome::Outcome::Inconclusive;
                }
                Judgement {
                    result: again.clone(),
                    attempts: vec![first, again],
                    retried: true,
                    timeout,
                    timeout_source,
                    route,
                }
            };
        self.workspace.trace.route(judgement.route.record(
            mutant,
            judgement.route.executed(
                &judgement.result.target,
                judgement.result.outcome.detected(),
            ),
        ));
        Ok(judgement)
    }

    fn execute(
        &self,
        request: &Request,
        how: Running<'_>,
        cancel: &Cancel,
    ) -> Result<MutantResult, EngineError> {
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
                    return Ok(unreached());
                }
                routed
            }
        };
        let context = Context {
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: Some((mutant.id.as_str(), self.catalog.digest())),
            touch: None,
            profile: None,
        };
        let mut last = None;
        let mut silent = None;
        for target in targets {
            let (timeout, source) = self.timeout_for(request, &target.id);
            let mut exec = ExecRequest::new(target)
                .with_args(self.arguments(request))
                .with_timeout(Some(timeout))
                .with_scratch(self.exec_scratch());
            if let Some(test) = &request.test {
                exec = exec.with_test(test.clone());
            } else if let Some(named) = self.filtering(target, chosen, cancel) {
                exec = exec.with_tests(named);
            }
            let result = execute::exec(&exec, &context, cancel, &self.workspace.trace);
            self.workspace.trace.mutant_exec(MutantExecRecord {
                id: mutant.id.clone(),
                index: mutant.index,
                target: target.id.clone(),
                outcome: result.outcome.name().to_owned(),
                exit_code: result.exit_code,
                duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
                tests_run: result.tests_run,
                signal: result.signal,
                failed_tests: result.failed_tests.clone(),
                timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                timeout_source: source.name().to_owned(),
                alone,
            });
            if result.outcome.detected() || cancel.is_cancelled() {
                return Ok(result);
            }
            if spoke(&result) {
                last = Some(result);
            } else {
                silent = Some(result);
            }
        }
        last.or(silent).ok_or_else(|| {
            EngineError::from(SessionError::NoTargets {
                packages: self.packages(),
            })
        })
    }

    /// The arguments one execution's test binary is started with.
    ///
    /// A request that names its own runs with those, so a caller asking one
    /// question of one mutant is not fighting the session; one that names
    /// none runs with the session's, which is what the baseline ran with.
    fn arguments(&self, request: &Request) -> Vec<String> {
        if request.args.is_empty() {
            self.harness_args.clone()
        } else {
            request.args.clone()
        }
    }

    /// Runs one target with no mutant active: the original control.
    ///
    /// # Errors
    /// [`SessionError::UnknownTarget`] and [`SessionError::NoTargets`].
    pub fn control(&self, request: &Request, cancel: &Cancel) -> Result<MutantResult, EngineError> {
        let targets = self.selected(request.target.as_deref())?;
        let context = Context {
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: None,
            touch: None,
            profile: None,
        };
        let mut last = None;
        let mut silent = None;
        for target in targets {
            let (timeout, source) = self.timeout_for(request, &target.id);
            let mut exec = ExecRequest::new(target)
                .with_args(self.arguments(request))
                .with_timeout(Some(timeout))
                .with_scratch(self.exec_scratch());
            if let Some(test) = &request.test {
                exec = exec.with_test(test.clone());
            }
            let result = execute::exec(&exec, &context, cancel, &self.workspace.trace);
            self.workspace.trace.mutant_exec(MutantExecRecord {
                id: String::new(),
                index: u32::MAX,
                target: target.id.clone(),
                outcome: result.outcome.name().to_owned(),
                exit_code: result.exit_code,
                duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
                tests_run: result.tests_run,
                signal: result.signal,
                failed_tests: result.failed_tests.clone(),
                timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                timeout_source: source.name().to_owned(),
                alone: false,
            });
            if cancel.is_cancelled()
                || (result.outcome != crate::outcome::Outcome::Survived && spoke(&result))
            {
                return Ok(result);
            }
            if spoke(&result) {
                last = Some(result);
            } else {
                silent = Some(result);
            }
        }
        last.or(silent).ok_or_else(|| {
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
///
/// A session cannot be reopened — it owns a snapshot and the processes that
/// run in it — so what a later command reads is this and the stored outcomes,
/// never the session itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
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

/// Whose evidence an execution is narrowed by.
///
/// [`Session::judge`] is this run reaching its own verdict, so it removes the
/// targets its own proofs discharged. [`Session::exec`] is a caller asking
/// what the tests say, with evidence of its own this run knows nothing about,
/// so it keeps every target the measurement placed and removes only what the
/// measurement itself removed. The difference is deliberate, and naming it is
/// what keeps it from being one of them quietly becoming the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asking {
    /// What the tests say, which is the question a caller with its own evidence asks.
    Anything,
    /// What this run established, which is the question its own verdict answers.
    ThisRun,
}

/// What one execution was narrowed to, once the route it rests on has been decided.
///
/// A narrowing is one thing rather than two: naming tests of a target the
/// execution does not run, and running a target the narrowing forgot to say
/// anything about, are both states this cannot be in.
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
    ///
    /// A request that names a target is a person asking about that target, and
    /// what a measurement said about the others is not what they asked.
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

/// What a run notes when a set of tests does not answer on its own, so the whole target ran instead.
pub const TEST_ROUTING_UNSOUND: &str = "test-routing-unsound";

/// What each set of tests a route named answers on its own: how long it took with nothing active, or nothing when running it on its own is not the same question as running its target.
type Established = BTreeMap<(String, Vec<String>), Option<Duration>>;

/// What one mutant's judgement is made of: what stands, every attempt it took, and the budget each was given.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Judgement {
    /// What the run establishes about the mutant.
    pub result: MutantResult,
    /// Every execution, in order. One unless a budget expired.
    pub attempts: Vec<MutantResult>,
    /// Whether an expired budget was confirmed with the machine to itself.
    pub retried: bool,
    /// The budget the target that answered was given.
    pub timeout: Duration,
    /// Where that budget came from.
    pub timeout_source: TimeoutSource,
    /// Which targets could have noticed the mutation, and what removed the ones that could not.
    pub route: Route,
}

impl Judgement {
    /// How long every execution of this mutant took together.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.attempts.iter().fold(Duration::ZERO, |total, one| {
            total.saturating_add(one.duration)
        })
    }
}

/// What a run waits for one mutant execution.
///
/// A budget nobody chose has to come from somewhere, and the only thing a run
/// measured about a target is how long that target's own tests take with
/// nothing active. Five times that is long enough for a mutation that made
/// something slower and short enough that a mutation that made something
/// never return is a finding rather than a wait.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Timeout {
    /// A multiple of what the target's own baseline took, never below [`MINIMUM_DERIVED_TIMEOUT`].
    #[default]
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
#[must_use]
pub fn derived(baseline: Duration) -> Duration {
    baseline
        .saturating_mul(TIMEOUT_MULTIPLE)
        .max(MINIMUM_DERIVED_TIMEOUT)
}

impl Timeout {
    /// The budget and where it came from, for a target whose baseline took `baseline`.
    #[must_use]
    pub fn of(self, baseline: Option<Duration>) -> (Duration, TimeoutSource) {
        match self {
            Self::Fixed(chosen) => (chosen, TimeoutSource::Configured),
            Self::Auto => (
                baseline.map_or(DEFAULT_MUTANT_TIMEOUT, derived),
                TimeoutSource::Derived,
            ),
        }
    }
}

/// Whether a target said anything at all.
///
/// A request that names no target runs every one of them, and a package holds
/// targets with no tests in them: a library or a binary whose harness is empty
/// runs nothing and establishes nothing, in either direction. Reading that
/// silence as an answer makes a mutant look inconclusive because a sibling
/// target had no tests, and makes a control look failed for the same reason.
const fn spoke(result: &MutantResult) -> bool {
    execute::answered(result.outcome)
}

const fn unreached() -> MutantResult {
    MutantResult {
        outcome: crate::outcome::Outcome::NotRun,
        target: String::new(),
        exit_code: crate::runner::EXIT_CODE_UNAVAILABLE,
        duration: Duration::ZERO,
        output: Vec::new(),
        summary: None,
        tests_run: None,
        signal: None,
        failed_tests: Vec::new(),
        passed_tests: Vec::new(),
        ignored_tests: Vec::new(),
    }
}

/// The name a target id takes, re-exported so a caller can build one without knowing the shape.
#[must_use]
pub fn target_name(package: &str, kind: TargetKind, name: &str) -> String {
    target_id(package, kind, name)
}
