// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A prepared workspace: every accepted mutant instrumented into one build, and the test binaries that build produced.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::EngineError;
use crate::cargo::{CompileKind, CompileOptions, compile};
use crate::catalog::{Catalog, Mutant};
use crate::discover::{self, DiscoverOptions, SkipClaim};
use crate::execute::{self, Context, ExecRequest, MutantResult, TargetKind, TestTarget, target_id};
use crate::glob::Pattern;
use crate::instrument::{FileOutput, Placement, instrument_file, plan_file};
use crate::rule::{Registry, Tier};
use crate::runner::Cancel;
use crate::snapshot::Drift;
use crate::syntax::{Found, LineIndex, Position, Selection, Skip};
use crate::trace::{BuildRecord, InstrumentRecord, MutantExecRecord};
use crate::validate::{
    Attempt, Compile, Rejection, ValidateError, ValidateOptions, Validated, Validating, validate,
};
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
}

/// Whether an item path is the one a locator names, which a suffix says.
fn names(item: &str, wanted: &str) -> bool {
    item == wanted || item.ends_with(&format!("::{wanted}"))
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
    /// Where to remember what measuring this tree established, so the next run of it measures nothing.
    ///
    /// A coverage measurement is a function of the sources the build compiled,
    /// the manifests that chose its flags and dependencies, and the toolchain.
    /// A mutation changes none of them, and instrumenting for coverage
    /// rebuilds every crate in the graph, so a tree that has not changed is a
    /// whole build a run does not have to do. `None` measures it again every
    /// time.
    pub measurements: Option<PathBuf>,
    /// Run every test target once with nothing active, and refuse to hand back a session whose instrumented baseline does not pass.
    pub verify: bool,
    /// Build and run the probe tree, which says which tests could not have noticed a return replacement however far they ran.
    pub probe: bool,
    /// Build and run the tree once with coverage instrumentation, so a mutant is only ever run against the targets that reached it.
    pub coverage: bool,
    /// Ask the compiler which mutations change nothing outside the branch they sit in, so a target that never ran that branch is not run against them.
    ///
    /// A proof without a measurement removes nothing: the lemma is the
    /// compiler's and the premise is the coverage layer's, and a caller with
    /// its own coverage discharges with [`crate::prove::discharges`].
    pub branch_proofs: bool,
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
    /// Targets never to start, by the id a report names them with.
    ///
    /// A target whose tests are about the text of what the compiler said —
    /// a `trybuild` or a snapshot suite — fails under instrumentation for a
    /// reason that is not the mutation, and would fail the verification of
    /// every run. Naming it here leaves it out of the build's answer and says
    /// so as a limitation, which is a decision somebody made rather than a
    /// result nobody can read.
    pub skip_targets: Vec<String>,
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
            verify: true,
            probe: false,
            coverage: true,
            branch_proofs: true,
            max_rounds: crate::validate::DEFAULT_MAX_ROUNDS,
            build_timeout: None,
            mutant_timeout: Timeout::default(),
            doctests: true,
            build: crate::cargo::BuildConfig::default(),
            skip_targets: Vec::new(),
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

/// Which targets could notice a mutation, and what the route rests on.
///
/// A route is the reach layer of [ADR 0004](../../docs/adr/0004-proof-layers-not-budgets.md):
/// a rule that removes an execution because evidence the run already holds
/// says the execution could not observe the mutant. Every fallback is toward
/// running more, and every one of them is named, so a reader who sees a run go
/// faster can say which layer did it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Route {
    /// Every target, because the measurement says nothing about this place.
    All {
        /// Which target could notice it: all of them.
        reaching: Vec<String>,
        /// Why the route is everything.
        fallback: Fallback,
    },
    /// The targets a measurement places at the mutation, and the ones a proof removed.
    Block {
        /// The targets whose measured run covered the position, plus every target the measurement could not read.
        reaching: Vec<String>,
        /// The targets a proof removed from what could have noticed the mutation.
        discharged: Vec<Discharge>,
        /// Why targets the measurement did not place are in `reaching` anyway.
        fallback: Option<Fallback>,
    },
    /// A mutation every target was proved unable to notice.
    Discharged {
        /// Each target, with the proof that removed it.
        discharged: Vec<Discharge>,
    },
    /// A mutation no measured target executes, which nothing needs to run to find out again.
    Unreached,
}

/// The proof that a target which never ran the body of the branch a mutation sits in cannot have noticed it.
pub const BRANCH_NEVER_TAKEN: &str = "branch-never-taken";

/// The proof that a target which ran the mutation without its value ever differing cannot have noticed it.
pub const NEVER_INFECTED: &str = "never-infected";

/// Why a route is wider than a measurement alone would make it. Every one of these runs more, never less.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fallback {
    /// Nothing was measured at all.
    NotMeasured,
    /// The mutation's position could not be counted in the file a person would open.
    PositionUnknown,
    /// The coverage build instrumented no block holding the position, so the measurement says nothing about it.
    OutsideBlocks,
    /// A target ran and its profile could not be read, so what it reached is unknown.
    CoverageIncomplete,
}

impl Fallback {
    /// The name a route record carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotMeasured => "not-measured",
            Self::PositionUnknown => "position-unknown",
            Self::OutsideBlocks => "outside-blocks",
            Self::CoverageIncomplete => "coverage-incomplete",
        }
    }
}

/// One target a proof removed from what could have noticed a mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discharge {
    /// The target.
    pub target: String,
    /// The proof that removed it.
    pub proof: &'static str,
}

/// What a route is decided among.
#[derive(Debug, Clone, Copy)]
pub struct Routing<'a> {
    /// Every target the run built.
    pub targets: &'a [&'a str],
    /// The targets a coverage build can measure, which is every one it compiles.
    ///
    /// A library's documented examples are compiled by rustdoc while cargo
    /// runs them, so no coverage build instruments them and no measurement can
    /// name them. They are not unmeasured targets, they are targets this
    /// measurement is not about, and they reach by the rule in `also_reaching`.
    pub measurable: &'a [&'a str],
    /// The targets routed by a rule other than the measurement.
    pub also_reaching: &'a [&'a str],
}

impl Route {
    /// Which targets a measurement puts at `position` of `path`, out of `targets`.
    ///
    /// A target that ran and whose profile could not be read is kept: what the
    /// measurement says nothing about is run rather than assumed.
    /// A target a coverage build could have measured is measured when the
    /// measurement **names** it, and not otherwise. One that is absent — its
    /// profile unreadable, its run never made, the measurement cut short
    /// before it — is one nothing was established about, and it stays in the
    /// route. Being absent from a measurement is not the same as being
    /// measured and covering nothing, and reading the first as the second
    /// turns a kill into a survivor: the one thing this layer must never do.
    #[must_use]
    pub fn decide(
        reached: &crate::reach::Reached,
        path: &std::path::Path,
        position: crate::coverage::Point,
        among: &Routing<'_>,
    ) -> Self {
        let Routing {
            targets,
            measurable,
            also_reaching,
        } = *among;
        let everything = |fallback: Fallback| Self::All {
            reaching: targets.iter().map(|target| (*target).to_owned()).collect(),
            fallback,
        };
        if !reached.measured() {
            return everything(Fallback::NotMeasured);
        }
        let Some(covering) = reached.covering(path, position) else {
            return everything(Fallback::OutsideBlocks);
        };
        let unmeasured: Vec<&str> = measurable
            .iter()
            .copied()
            .filter(|target| {
                !reached.targets.contains_key(*target)
                    || reached.limitations.iter().any(|limitation| {
                        limitation == &format!("{}:{target}", crate::reach::UNMEASURED)
                    })
            })
            .collect();
        let mut reaching: Vec<String> = targets
            .iter()
            .copied()
            .filter(|target| {
                covering.contains(target)
                    || unmeasured.contains(target)
                    || also_reaching.contains(target)
            })
            .map(str::to_owned)
            .collect();
        reaching.dedup();
        if reaching.is_empty() {
            return Self::Unreached;
        }
        Self::Block {
            reaching,
            discharged: Vec::new(),
            fallback: (!unmeasured.is_empty()).then_some(Fallback::CoverageIncomplete),
        }
    }

    /// The granularity a route record carries: `all`, `block`, `discharged`, or `unreached`.
    #[must_use]
    pub const fn granularity(&self) -> &'static str {
        match self {
            Self::All { .. } => "all",
            Self::Block { .. } => "block",
            Self::Discharged { .. } => "discharged",
            Self::Unreached => "unreached",
        }
    }

    /// Why the route is wider than the measurement alone would make it, when it is.
    #[must_use]
    pub fn fallback(&self) -> Option<&'static str> {
        match self {
            Self::All { fallback, .. } => Some(fallback.name()),
            Self::Block { fallback, .. } => fallback.map(Fallback::name),
            Self::Discharged { .. } | Self::Unreached => None,
        }
    }

    /// The targets that could notice the mutation.
    #[must_use]
    pub fn reaching(&self) -> Vec<&str> {
        match self {
            Self::All { reaching, .. } | Self::Block { reaching, .. } => {
                reaching.iter().map(String::as_str).collect()
            }
            Self::Discharged { .. } | Self::Unreached => Vec::new(),
        }
    }

    /// The targets the coverage measurement alone places at the mutation, or nothing when it places none.
    ///
    /// This is what [`Session::exec`] runs, and it is deliberately wider than
    /// [`Route::reaching`]: a discharge is a proof a caller may not share, and
    /// `exec` is the question "what do the tests say", asked by a caller with
    /// its own evidence. [`Session::judge`] is the one that removes work.
    ///
    /// It is the one place a coverage narrowing is decided: an execution that
    /// narrowed by anything else would run fewer targets than the route says,
    /// and a survivor it reported would be one nobody measured.
    #[must_use]
    pub fn narrowing(&self) -> Option<Vec<String>> {
        let with_discharged = |reaching: &[String], discharged: &[Discharge]| {
            let mut every: Vec<String> = reaching.to_vec();
            every.extend(discharged.iter().map(|one| one.target.clone()));
            every.sort();
            every.dedup();
            every
        };
        match self {
            Self::All { .. } => None,
            Self::Block {
                reaching,
                discharged,
                ..
            } => Some(with_discharged(reaching, discharged)),
            Self::Discharged { discharged } => Some(with_discharged(&[], discharged)),
            Self::Unreached => Some(Vec::new()),
        }
    }

    /// The targets a proof removed, each with the proof's name.
    #[must_use]
    pub fn discharged(&self) -> &[Discharge] {
        match self {
            Self::Block { discharged, .. } | Self::Discharged { discharged } => discharged,
            Self::All { .. } | Self::Unreached => &[],
        }
    }

    /// The targets an execution of this route ran, given the target that answered and whether it detected the mutation.
    ///
    /// [`Session::exec`] walks the routed targets in order and stops at the
    /// first one that detects, so what ran is the whole route when nothing
    /// detected and the prefix ending at the answer when one did. An answer
    /// this route does not hold is one target on its own, which is what
    /// `--target` asks for.
    #[must_use]
    pub fn executed(&self, answered: &str, detected: bool) -> Vec<String> {
        if answered.is_empty() {
            return Vec::new();
        }
        let reaching: Vec<String> = self.reaching().into_iter().map(str::to_owned).collect();
        let Some(at) = reaching.iter().position(|target| target == answered) else {
            return vec![answered.to_owned()];
        };
        if detected {
            reaching.into_iter().take(at.saturating_add(1)).collect()
        } else {
            reaching
        }
    }

    /// The record of this decision, with the targets that actually ran.
    #[must_use]
    pub fn record(&self, mutant: &Mutant, executed: Vec<String>) -> crate::trace::RouteRecord {
        crate::trace::RouteRecord {
            mutant: mutant.display_id.clone(),
            index: mutant.index,
            granularity: self.granularity().to_owned(),
            fallback: self.fallback().map(str::to_owned),
            reaching: self.reaching().into_iter().map(str::to_owned).collect(),
            discharged: self
                .discharged()
                .iter()
                .map(|discharge| crate::trace::DischargeRecord {
                    target: discharge.target.clone(),
                    proof: discharge.proof.to_owned(),
                })
                .collect(),
            executed,
            reused: None,
        }
    }
}

/// A prepared workspace.
#[derive(Debug)]
pub struct Session {
    workspace: Workspace,
    catalog: Catalog,
    skips: Vec<Skip>,
    claims: Vec<SkipClaim>,
    validated: Validated,
    targets: Vec<TestTarget>,
    scratch: PathBuf,
    /// How many executions this session has started, which is what names each one's own temporary directory.
    executions: std::sync::atomic::AtomicU64,
    mutant_timeout: Timeout,
    /// The files as they were before instrumentation, so a position can be counted in the file a person would open rather than in the rewrite.
    sources: BTreeMap<String, Vec<u8>>,
    /// Which package each mutant belongs to.
    packages: BTreeMap<u32, String>,
    /// The item each mutant sits in, by catalog index.
    items: BTreeMap<u32, String>,
    /// The branch proof of every mutant that has one, by catalog index.
    proofs: BTreeMap<u32, crate::syntax::branch::Proof>,
    reached: crate::reach::Reached,
    /// What the probe pass established, empty when it did not run.
    probed: crate::probe::tree::Probed,
    /// How long each target's own baseline took, which is what a derived timeout is a multiple of. Empty when nothing was verified.
    baseline: BTreeMap<String, Duration>,
    /// What the tree gained or lost while the proof layers ran, which is what a test wrote before anything was instrumented.
    written_by_a_test: Vec<Drift>,
    /// The digest of the pristine sources every unit of this build compiled.
    closure: String,
    /// The digest of the manifests, the lock file, and the cargo configuration the build read.
    manifests: String,
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

    /// Every place discovery passed over, with its reason.
    #[must_use]
    pub fn skips(&self) -> &[Skip] {
        &self.skips
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
    fn packages(&self) -> Vec<String> {
        let mut named: Vec<String> = self.packages.values().cloned().collect();
        named.sort_unstable();
        named.dedup();
        named
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
        match narrowed.as_slice() {
            [] => Err(LocateError::Nothing),
            [one] => Ok(one),
            several => Err(LocateError::Several {
                display_ids: several
                    .iter()
                    .map(|mutant| mutant.display_id.clone())
                    .collect(),
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
        self.baseline.get(target).copied()
    }

    /// The longest a target's own baseline took, which is what an estimate of a run's cost rests on.
    #[must_use]
    pub fn slowest_baseline(&self) -> Duration {
        self.baseline
            .values()
            .copied()
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
        let Some(position) = self.position(mutant) else {
            return Route::All {
                reaching: targets.iter().map(|target| (*target).to_owned()).collect(),
                fallback: Fallback::PositionUnknown,
            };
        };
        let measurable: Vec<&str> = self
            .targets
            .iter()
            .filter(|target| target.kind != TargetKind::Doc)
            .map(|target| target.id.as_str())
            .collect();
        let decided = Route::decide(
            &self.reached,
            std::path::Path::new(&mutant.candidate.path),
            crate::coverage::Point {
                line: position.line,
                column: position.byte_column,
            },
            &Routing {
                targets: &targets,
                measurable: &measurable,
                also_reaching: &self.documenting(mutant),
            },
        );
        self.discharging(mutant, decided)
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
        for target in reaching {
            match self.proof_against(mutant, &target) {
                Some(proof) => discharged.push(Discharge { target, proof }),
                None => kept.push(target),
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
    /// would rest on the measurement's failure. The probe answers about the
    /// targets it ran, and a mutant it never asked about is one it says
    /// nothing about.
    fn proof_against(&self, mutant: &Mutant, target: &str) -> Option<&'static str> {
        if let Some(proof) = self.branch(mutant.index)
            && let Some(covered) = self.reached.targets.get(target)
            && crate::prove::discharges(
                proof,
                std::path::Path::new(&mutant.candidate.path),
                &covered.iter().cloned().collect::<Vec<_>>(),
            )
        {
            return Some(BRANCH_NEVER_TAKEN);
        }
        if self.probed.asked.contains(&mutant.index)
            && let Some(infected) = self.probed.infected.get(target)
            && !infected.contains(&mutant.index)
        {
            return Some(NEVER_INFECTED);
        }
        None
    }

    /// The recording this session writes to, which is the one the workspace was opened with.
    #[must_use]
    pub const fn trace(&self) -> &crate::trace::Recorder {
        &self.workspace.trace
    }

    /// What the probe pass established: which mutants it could ask about, and what each target infected.
    #[must_use]
    pub const fn probed(&self) -> &crate::probe::tree::Probed {
        &self.probed
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
        self.execute(request, Running::everywhere(), cancel)
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
        let mutant = self.resolve(&request.mutant)?;
        let route = self.route(mutant);
        let reaching: Option<Vec<String>> = request
            .target
            .is_none()
            .then(|| route.reaching().into_iter().map(str::to_owned).collect());
        let only = reaching.as_deref();
        let first = quiet.shared(|| self.execute(request, Running::shared(only), cancel))?;
        let (timeout, timeout_source) = self.timeout_for(request, &first.target);
        let judgement = if first.outcome != crate::outcome::Outcome::TimedOut
            || cancel.is_cancelled()
        {
            Judgement {
                result: first.clone(),
                attempts: vec![first],
                retried: false,
                timeout,
                timeout_source,
                route,
            }
        } else {
            let mut again = quiet.alone(|| self.execute(request, Running::alone(only), cancel))?;
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
        let Running { alone, only } = how;
        let mutant = self.resolve(&request.mutant)?;
        let targets = self.selected(request.target.as_deref())?;
        let narrowing = only.map_or_else(
            || {
                request
                    .target
                    .is_none()
                    .then(|| self.covering(mutant))
                    .flatten()
            },
            |named| Some(named.to_vec()),
        );
        let targets = match narrowing {
            Some(covering) => {
                let routed: Vec<&TestTarget> = targets
                    .into_iter()
                    .filter(|target| covering.iter().any(|one| one == &target.id))
                    .collect();
                if routed.is_empty() {
                    return Ok(unreached());
                }
                routed
            }
            None => targets,
        };
        let context = Context {
            base_env: &self.workspace.base_env,
            cargo: Some(self.workspace.toolchain.cargo()),
            sysroot: self.workspace.toolchain.sysroot(),
            active: Some((mutant.id.as_str(), self.catalog.digest())),
            probe: None,
            profile: None,
        };
        let mut last = None;
        let mut silent = None;
        for target in targets {
            let (timeout, source) = self.timeout_for(request, &target.id);
            let mut exec = ExecRequest::new(target)
                .with_args(request.args.clone())
                .with_timeout(Some(timeout))
                .with_scratch(self.exec_scratch());
            if let Some(test) = &request.test {
                exec = exec.with_test(test.clone());
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
            probe: None,
            profile: None,
        };
        let mut last = None;
        let mut silent = None;
        for target in targets {
            let (timeout, source) = self.timeout_for(request, &target.id);
            let mut exec = ExecRequest::new(target)
                .with_args(request.args.clone())
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
    /// The targets to run, or nothing to let the coverage narrowing decide.
    only: Option<&'a [String]>,
}

impl<'a> Running<'a> {
    /// Every target the measurement placed, beside whatever else is running.
    const fn everywhere() -> Self {
        Self {
            alone: false,
            only: None,
        }
    }

    /// These targets, beside whatever else is running.
    const fn shared(only: Option<&'a [String]>) -> Self {
        Self { alone: false, only }
    }

    /// These targets, with the machine to itself.
    const fn alone(only: Option<&'a [String]>) -> Self {
        Self { alone: true, only }
    }
}

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

/// The rules a set of options selects.
fn selection(options: &PrepareOptions) -> Result<Selection<'static>, EngineError> {
    static REGISTRY: Registry = Registry::canonical();
    if options.operators.is_empty() {
        return Ok(Selection::tier(&REGISTRY, options.tier));
    }
    let names: Vec<&str> = options.operators.iter().map(String::as_str).collect();
    Ok(Selection::rules(&REGISTRY, &names)?)
}

/// Refuses a tree that does not compile before anything is instrumented, and hands back the units the check compiled.
///
/// The check is also what says which files each target compiles **outside** a
/// test build, and a file no non-test unit compiled is one only the tests see:
/// without that, a library whose only compilation is its own test harness has
/// every one of its files read as test-only and nothing in it is worth
/// mutating. So the check is not only a gate and cannot be skipped.
///
/// Whether the tree *links* is a second question, and it used to be a second
/// compilation of the whole workspace on every run. It is not one any more.
/// The first validation round links the instrumented tree, and an instrumented
/// tree that links is one whose pristine form links too — the guards only add
/// code. A round that fails with nothing attributable to a mutation is a round
/// that compiles with nothing live, which is `RM4001`: the tree, in the
/// compiler's own words. The answer is the same and the successful run does
/// one build fewer.
fn gate(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<crate::cargo::Compiled, EngineError> {
    pristine(workspace, options, cancel)
}

/// Compiles the tree as it was copied, which is both the gate a run stands on and the source of every unit's file set.
fn pristine(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<crate::cargo::Compiled, EngineError> {
    let checked = compile(
        &workspace.driver(cancel),
        &CompileOptions {
            kind: CompileKind::Check,
            packages: Vec::new(),
            target_dir: Some(workspace.target_dir.clone()),
            locked: workspace.locked,
            offline: workspace.offline,
            timeout: Workspace::timeout(options.build_timeout),
            env: Vec::new(),
            build: options.build.clone(),
        },
    )?;
    if checked.success {
        Ok(checked)
    } else {
        Err(EngineError::from(SessionError::PristineBroken {
            first: crate::validate::first_error_of(&checked.messages),
        }))
    }
}

/// The digest of the pristine sources every unit of the build compiled.
///
/// The bytes hashed are the ones the file held before anything was
/// instrumented: for a file with guards in it that is what the plan kept, and
/// for every other file it is the snapshot's own copy, which nothing wrote to.
/// Hashing the rewrite instead would tie the digest to the catalog, and a
/// catalog changes whenever any mutant anywhere does.
///
/// A build whose dep-info cannot be read yields nothing at all rather than a
/// partial answer, and a caller with nothing to key on remembers nothing.
fn closure_of(workspace: &Workspace, checked: &crate::cargo::Compiled) -> String {
    let root = workspace.snapshot_root();
    let units = &checked.units;
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for unit in units {
        for path in &unit.sources {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let Ok(name) = crate::id::normalize_path(&relative.to_string_lossy()) else {
                continue;
            };
            if files.contains_key(&name) {
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            drop(files.insert(name, crate::id::digest(&bytes)));
        }
    }
    if files.is_empty() {
        return String::new();
    }
    folded(
        files
            .iter()
            .map(|(name, digest)| (name.as_str(), digest.as_str())),
    )
}

/// The digest of every manifest, the lock file, and the cargo configuration the build read.
fn manifests_of(workspace: &Workspace) -> String {
    let root = workspace.snapshot_root();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let named: Vec<PathBuf> = workspace
        .metadata
        .packages
        .iter()
        .map(|package| package.manifest_path.clone())
        .chain([
            root.join("Cargo.toml"),
            root.join("Cargo.lock"),
            root.join(".cargo").join("config.toml"),
            root.join(".cargo").join("config"),
            root.join("rust-toolchain.toml"),
            root.join("rust-toolchain"),
        ])
        .collect();
    for path in named {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Ok(name) = crate::id::normalize_path(&relative.to_string_lossy()) else {
            continue;
        };
        if files.contains_key(&name) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        drop(files.insert(name, crate::id::digest(&bytes)));
    }
    folded(
        files
            .iter()
            .map(|(name, digest)| (name.as_str(), digest.as_str())),
    )
}

/// One digest over a sorted list of names and their own digests.
fn folded<'a>(entries: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut text = String::new();
    for (name, digest) in entries {
        text.push_str(name);
        text.push('\0');
        text.push_str(digest);
        text.push('\n');
    }
    crate::id::digest(text.as_bytes())
}

/// The test binaries the instrumented build produced, and the directory their processes work in.
type Built = (Vec<TestTarget>, PathBuf, BTreeMap<String, Duration>);

fn built(
    workspace: &Workspace,
    last_build: &[crate::cargo::Message],
    options: &PrepareOptions,
    watching: (&Cancel, &crate::trace::Recorder),
) -> Result<Built, EngineError> {
    let (cancel, trace) = watching;
    let mut targets = execute::targets_of(
        last_build,
        &workspace.metadata.packages,
        Some(&workspace.target_dir),
    );
    if targets.is_empty() {
        return Err(EngineError::from(SessionError::NoTargets {
            packages: options.packages.clone(),
        }));
    }
    let members: Vec<&crate::cargo::Package> = workspace
        .metadata
        .members()
        .filter(|package| options.packages.is_empty() || options.packages.contains(&package.name))
        .collect();
    if options.doctests {
        targets.extend(execute::documentation_targets(
            &members,
            workspace.toolchain.cargo(),
            &documentation_arguments(workspace),
        ));
    }
    let built: Vec<crate::trace::TargetRecord> = targets
        .iter()
        .map(|target| crate::trace::TargetRecord {
            id: target.id.clone(),
            kind: target.kind.name().to_owned(),
            harness: target.harness,
            limitations: target.limitations.clone(),
        })
        .collect();
    let skipped: Vec<String> = targets
        .iter()
        .filter(|target| options.skip_targets.iter().any(|one| one == &target.id))
        .map(|target| target.id.clone())
        .collect();
    targets.retain(|target| !skipped.contains(&target.id));
    let mut details = built;
    for detail in &mut details {
        if skipped.contains(&detail.id) {
            detail
                .limitations
                .push(crate::limitation::TARGET_SKIPPED_BY_CONFIGURATION.to_owned());
        }
    }
    trace.build(BuildRecord {
        targets: details.iter().map(|target| target.id.clone()).collect(),
        details,
    });
    if targets.is_empty() {
        return Err(EngineError::from(SessionError::NoTargets {
            packages: options.packages.clone(),
        }));
    }
    let scratch = workspace.target_dir.join("scratch");
    std::fs::create_dir_all(&scratch).map_err(|source| SessionError::WriteFailed {
        path: scratch.display().to_string(),
        source,
    })?;
    let baseline = if options.verify {
        verify(workspace, &mut targets, &scratch, cancel)?
    } else {
        BTreeMap::new()
    };
    Ok((targets, scratch, baseline))
}

/// What cargo is told before the documentation examples' own arguments, so that running them reuses the build this session already made.
fn documentation_arguments(workspace: &Workspace) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        std::ffi::OsString::from("--target-dir"),
        workspace.target_dir.clone().into_os_string(),
    ];
    if workspace.locked {
        args.push(std::ffi::OsString::from("--locked"));
    }
    if workspace.offline {
        args.push(std::ffi::OsString::from("--offline"));
    }
    args
}

/// What the proof layers establish before anything is instrumented: which tests could not have noticed a return replacement, which branch proofs the compiler vouches for, and which targets reached what.
type Layers = (
    crate::probe::tree::Probed,
    BTreeMap<u32, crate::syntax::branch::Proof>,
    crate::reach::Reached,
);

fn layers(
    asking: &crate::prove::Asking<'_>,
    remembering: Option<&crate::reach::remembered::Remembering>,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<Layers, EngineError> {
    let probed = if asking.options.probe {
        crate::probe::tree::establish(
            &crate::probe::tree::Asking {
                workspace: asking.workspace,
                discovery: asking.discovery,
                sources: asking.sources,
                options: asking.options,
            },
            cancel,
            trace,
        )?
    } else {
        crate::probe::tree::Probed::default()
    };
    let proofs = if asking.options.branch_proofs {
        crate::prove::establish(asking, cancel, trace)?
    } else {
        BTreeMap::new()
    };
    let reached = measured(asking, remembering, cancel, trace)?;
    Ok((probed, proofs, reached))
}

/// What measuring this tree established, made now or remembered from the last run that made it.
///
/// The measurement is the most expensive thing a run does: instrumenting for
/// coverage changes the fingerprint of every crate and rebuilds the whole
/// graph. It is also a function of the tree alone, which a mutation does not
/// change, so a tree nothing has touched since the last run has already been
/// measured. Reading that back is a whole build removed on the claim the
/// outcome store already rests on: nothing that could change the answer
/// changed.
///
/// A measurement that did not reach every target is not remembered. It is a
/// measurement of some of them — sound to route by, because what it could not
/// read stays in every route — and remembering it would hand every later run
/// of the tree a partial answer with nothing to tell it from a whole one.
fn measured(
    asking: &crate::prove::Asking<'_>,
    remembering: Option<&crate::reach::remembered::Remembering>,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<crate::reach::Reached, EngineError> {
    if let Some(remembering) = remembering
        && let Some(reached) = remembering.read()
    {
        let phase = trace.phase("coverage");
        trace.note(
            "coverage-remembered",
            &format!(
                "the measurement of this tree is the one an earlier run made, filed under {}",
                remembering.key
            ),
        );
        phase.end();
        return Ok(reached);
    }
    let reached = crate::reach::establish(
        &crate::reach::Asking {
            workspace: asking.workspace,
            options: asking.options,
        },
        cancel,
        trace,
    )?;
    if let Some(remembering) = remembering
        && reached.measured()
    {
        remembering.write(&reached);
    }
    Ok(reached)
}

/// Where this run may remember what it measured, when it was given somewhere and asked to measure.
fn remembering(
    options: &PrepareOptions,
    closure: &str,
    manifests: &str,
    workspace: &Workspace,
) -> Option<crate::reach::remembered::Remembering> {
    if !options.coverage || closure.is_empty() {
        return None;
    }
    let directory = options.measurements.as_ref()?;
    let toolchain = format!(
        "{} {} {}",
        workspace.toolchain.cargo_version().summary,
        workspace.toolchain.rustc_version().summary,
        workspace.toolchain.host()
    );
    Some(crate::reach::remembered::Remembering::of(
        directory,
        &crate::reach::remembered::Of {
            closure,
            manifests,
            toolchain: &toolchain,
            build: &options.build.arguments(),
        },
    ))
}

/// What a mutant no measured target reached amounts to: nothing ran, because nothing that ran could have noticed.
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

/// The gate a run stands on, and what discovery found on the tree it passed.
///
/// # Errors
/// The pristine gate and the failures of discovery.
fn gated(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<Gated, EngineError> {
    let pristine_phase = trace.phase("pristine");
    let checked = gate(workspace, options, cancel)?;
    let closure = closure_of(workspace, &checked);
    pristine_phase.end();
    let discover_phase = trace.phase("discover");
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
        trace,
    )?;
    discover_phase.end();
    Ok(Gated { discovery, closure })
}

/// What the gate established: what there is to mutate, and the digest of everything the build read.
struct Gated {
    discovery: discover::Discovery,
    closure: String,
}

/// Discovers, instruments, validates, builds, and verifies.
///
/// # Errors
/// Every failure of the phases it runs.
pub fn prepare(
    workspace: Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<Session, EngineError> {
    let trace = workspace.trace.clone();
    let phase = trace.phase("prepare");
    let Gated { discovery, closure } = gated(&workspace, options, cancel, &trace)?;
    let manifests = manifests_of(&workspace);

    let plan_phase = trace.phase("plan");
    let (sources, placements) = plan_tree(workspace.snapshot_root(), &discovery)?;
    plan_phase.end();

    let (probed, proofs, reached) = layers(
        &crate::prove::Asking {
            workspace: &workspace,
            discovery: &discovery,
            sources: &sources,
            options,
        },
        remembering(options, &closure, &manifests, &workspace).as_ref(),
        cancel,
        &trace,
    )?;

    let validate_phase = trace.phase("validate");
    let (validated, last_build) = establish(
        &workspace,
        &discovery,
        &sources,
        &placements,
        options,
        cancel,
        &trace,
    )?;

    validate_phase.end();

    let mut workspace = workspace;
    let absorbed = workspace.snapshot.reseal()?;
    let written_by_a_test: Vec<Drift> = absorbed
        .into_iter()
        .filter(|drift| !sources.contains_key(drift.rel_path()))
        .collect();
    let build_phase = trace.phase("build");
    let (targets, scratch, baseline) = built(&workspace, &last_build, options, (cancel, &trace))?;
    build_phase.end();
    phase.end();
    let indexed: Vec<(u32, &discover::Located)> = discovery
        .candidates
        .iter()
        .filter_map(|located| {
            let id = located.found.candidate.id().ok()?;
            let mutant = discovery.catalog.by_id(&id)?;
            Some((mutant.index, located))
        })
        .collect();
    let packages = indexed
        .iter()
        .map(|(index, located)| (*index, located.package.clone()))
        .collect();
    let items = indexed
        .iter()
        .map(|(index, located)| (*index, located.found.item.clone()))
        .collect();
    Ok(Session {
        catalog: discovery.catalog,
        skips: discovery.skips,
        claims: discovery.claims,
        sources,
        packages,
        items,
        proofs,
        reached,
        probed,
        validated,
        targets,
        scratch,
        baseline,
        written_by_a_test,
        closure,
        manifests,
        executions: std::sync::atomic::AtomicU64::new(0),
        mutant_timeout: options.mutant_timeout,
        workspace,
    })
}

/// Instruments the tree and lets the compiler say which mutants are real, returning what it established and the build it ended with.
#[expect(
    clippy::too_many_arguments,
    reason = "every argument is a distinct fact of the run, and bundling them would only move the list"
)]
fn establish(
    workspace: &Workspace,
    discovery: &discover::Discovery,
    sources: &BTreeMap<String, Vec<u8>>,
    placements: &BTreeMap<String, Vec<Placement>>,
    options: &PrepareOptions,
    cancel: &Cancel,
    trace: &crate::trace::Recorder,
) -> Result<(Validated, Vec<crate::cargo::Message>), EngineError> {
    let mut writer = TreeCompiler {
        workspace,
        sources,
        placements,
        catalog: &discovery.catalog,
        cancel,
        timeout: Workspace::timeout(options.build_timeout),
        last_build: Vec::new(),
        packages: options.packages.clone(),
        build: options.build.clone(),
        written: BTreeMap::new(),
    };
    let validated = validate(
        &discovery.catalog,
        &mut writer,
        &Validating {
            options: ValidateOptions {
                max_rounds: options.max_rounds,
            },
            cancel,
            trace,
        },
    )?;
    Ok((validated, writer.last_build))
}

/// Reads every mutable file of the snapshot and pairs its candidates with their catalog entries, which is everything instrumentation needs.
type Planned = (BTreeMap<String, Vec<u8>>, BTreeMap<String, Vec<Placement>>);

fn plan_tree(
    root: &std::path::Path,
    discovery: &discover::Discovery,
) -> Result<Planned, EngineError> {
    let found: Vec<Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let mut sources: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut placements: BTreeMap<String, Vec<Placement>> = BTreeMap::new();
    for file in &discovery.files {
        if file.candidates == 0 {
            continue;
        }
        let source =
            std::fs::read(root.join(&file.path)).map_err(|source| SessionError::WriteFailed {
                path: file.path.clone(),
                source,
            })?;
        sources.insert(file.path.clone(), source);
        placements.insert(
            file.path.clone(),
            plan_file(&discovery.catalog, &file.path, &found)?,
        );
    }
    Ok((sources, placements))
}

/// How many lines a byte string holds.
fn lines(bytes: &[u8]) -> u64 {
    u64::try_from(crate::splice::count_lines(bytes)).unwrap_or(u64::MAX)
}

/// How many lines the rewritten body holds, the appended runtime excluded.
fn body_lines(file: &FileOutput) -> u64 {
    let text = file.text.as_bytes();
    file.text
        .rfind("\n#[doc(hidden)]")
        .map_or_else(|| lines(text), |at| lines(text.get(..=at).unwrap_or(text)))
}

/// Runs every target once with nothing active. A tree whose instrumented baseline fails is one whose every later result would be about the instrumentation rather than about a mutant.
fn verify(
    workspace: &Workspace,
    targets: &mut [TestTarget],
    scratch: &std::path::Path,
    cancel: &Cancel,
) -> Result<BTreeMap<String, Duration>, EngineError> {
    let phase = workspace.trace.phase("verify");
    let context = Context {
        base_env: &workspace.base_env,
        cargo: Some(workspace.toolchain.cargo()),
        sysroot: workspace.toolchain.sysroot(),
        active: None,
        probe: None,
        profile: None,
    };
    let mut baseline = BTreeMap::new();
    for target in targets.iter_mut() {
        let request = ExecRequest::new(target).with_scratch(scratch);
        let result = execute::exec(&request, &context, cancel, &workspace.trace);
        workspace.trace.verify(crate::trace::VerifyRecord {
            target: target.id.clone(),
            outcome: result.outcome.name().to_owned(),
            tests_run: result.tests_run,
            duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
        });
        let _kept = baseline.insert(target.id.clone(), result.duration);
        if target.kind == TargetKind::Doc && result.tests_run == Some(0) {
            target
                .limitations
                .push(crate::limitation::DOCTESTS_NONE.to_owned());
        }
        if !matches!(
            result.outcome,
            crate::outcome::Outcome::Survived | crate::outcome::Outcome::Inconclusive
        ) {
            return Err(EngineError::from(SessionError::VerifyFailed {
                target: target.id.clone(),
                output: String::from_utf8_lossy(&result.output).into_owned(),
            }));
        }
    }
    phase.end();
    Ok(baseline)
}

/// Instruments the snapshot with a set of mutants left out and compiles it: the [`Compile`] seam validation drives.
struct TreeCompiler<'a> {
    workspace: &'a Workspace,
    sources: &'a BTreeMap<String, Vec<u8>>,
    placements: &'a BTreeMap<String, Vec<Placement>>,
    catalog: &'a Catalog,
    cancel: &'a Cancel,
    timeout: Option<Duration>,
    /// The messages of the last attempt that compiled, which name the test binaries this session will run.
    last_build: Vec<crate::cargo::Message>,
    /// The member packages the run is about, which are the ones whose test binaries it will start.
    packages: Vec<String>,
    /// What the project is compiled as, which every attempt compiles the same way.
    build: crate::cargo::BuildConfig,
    /// What each file held when this last wrote it, so a round writes only what its condemnations changed.
    written: BTreeMap<String, String>,
}

impl Compile for TreeCompiler<'_> {
    fn attempt(
        &mut self,
        condemned: &std::collections::BTreeSet<u32>,
    ) -> Result<Attempt, ValidateError> {
        let mut files: Vec<FileOutput> = Vec::new();
        let mut written: u32 = 0;
        for (path, placements) in self.placements {
            let kept: Vec<Placement> = placements
                .iter()
                .filter(|placement| !condemned.contains(&placement.index))
                .cloned()
                .collect();
            let source = self
                .sources
                .get(path)
                .ok_or_else(|| ValidateError::AttemptFailed {
                    message: format!("{path} was never read"),
                })?;
            let file = instrument_file(path, source, &kept, self.catalog.digest())?;
            self.workspace.trace.instrument(InstrumentRecord {
                path: path.clone(),
                guards: u32::try_from(file.guards.len()).unwrap_or(u32::MAX),
                module: file.module.clone(),
                lines_before: lines(source),
                lines_after: body_lines(&file),
            });
            if rewrite_needed(self.written.get(path), &file.text) {
                std::fs::write(self.workspace.snapshot_root().join(path), &file.text).map_err(
                    |error| ValidateError::AttemptFailed {
                        message: format!("cannot write {path}: {error}"),
                    },
                )?;
                let _replaced = self.written.insert(path.clone(), file.text.clone());
                written = written.saturating_add(1);
            }
            files.push(file);
        }
        let compiled = compile(
            &self.workspace.driver(self.cancel),
            &CompileOptions {
                kind: CompileKind::Tests,
                packages: self.packages.clone(),
                target_dir: Some(self.workspace.target_dir.clone()),
                locked: self.workspace.locked,
                offline: self.workspace.offline,
                timeout: self.timeout,
                env: Vec::new(),
                build: self.build.clone(),
            },
        )?;
        let success = compiled.success;
        if success {
            self.last_build.clone_from(&compiled.messages);
        }
        Ok(Attempt {
            files,
            messages: compiled.messages,
            success,
            written,
        })
    }
}

/// Whether a round has to write this file again: only what its condemnations changed.
///
/// Every round instruments every mutable file, because attribution needs each
/// file's branch spans whatever it condemns. Writing them all back costs the
/// whole tree in bytes for every round, and a file whose live set did not
/// change holds what it already held.
#[must_use]
pub fn rewrite_needed(written: Option<&String>, next: &str) -> bool {
    written.is_none_or(|last| last != next)
}

/// The name a target id takes, re-exported so a caller can build one without knowing the shape.
#[must_use]
pub fn target_name(package: &str, kind: TargetKind, name: &str) -> String {
    target_id(package, kind, name)
}
