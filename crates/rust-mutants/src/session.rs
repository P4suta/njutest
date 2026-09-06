// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A prepared workspace: every accepted mutant instrumented into one build, and the test binaries that build produced.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::EngineError;
use crate::cargo::{CompileKind, CompileOptions, compile};
use crate::catalog::{Catalog, Mutant};
use crate::discover::{self, DiscoverOptions};
use crate::execute::{self, Context, ExecRequest, MutantResult, TargetKind, TestTarget, target_id};
use crate::glob::Pattern;
use crate::instrument::{FileOutput, Placement, instrument_file, plan_file};
use crate::rule::{Registry, Tier};
use crate::runner::Cancel;
use crate::snapshot::Drift;
use crate::syntax::{Found, LineIndex, Position, Selection, Skip};
use crate::trace::{BuildRecord, InstrumentRecord, MutantExecRecord};
use crate::validate::{
    Attempt, Compile, Rejection, ValidateError, ValidateOptions, Validated, validate,
};
use crate::workspace::{SessionError, Workspace};

/// Configures [`Workspace::prepare`].
#[derive(Debug, Clone)]
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
    /// Run every test target once with nothing active, and refuse to hand back a session whose instrumented baseline does not pass.
    pub verify: bool,
    /// Build and run the probe tree, which says which tests could not have noticed a return replacement however far they ran.
    pub probe: bool,
    /// Build and run the tree once with coverage instrumentation, so a mutant is only ever run against the targets that reached it.
    pub coverage: bool,
    /// How many validation rounds before falling back to bisection.
    pub max_rounds: u32,
    /// How long a build may take.
    pub build_timeout: Option<Duration>,
    /// How long one mutant execution may take, when the caller does not say.
    pub mutant_timeout: Option<Duration>,
}

impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            tier: Tier::Balanced,
            operators: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
            verify: true,
            probe: false,
            coverage: false,
            max_rounds: crate::validate::DEFAULT_MAX_ROUNDS,
            build_timeout: None,
            mutant_timeout: None,
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

impl Route {
    /// Which targets a measurement puts at `position` of `path`, out of `targets`.
    ///
    /// A target that ran and whose profile could not be read is kept: what the
    /// measurement says nothing about is run rather than assumed.
    #[must_use]
    pub fn decide(
        reached: &crate::reach::Reached,
        path: &std::path::Path,
        position: crate::coverage::Point,
        targets: &[&str],
    ) -> Self {
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
        let unmeasured: Vec<&str> = targets
            .iter()
            .copied()
            .filter(|target| {
                reached.limitations.iter().any(|limitation| {
                    limitation == &format!("{}:{target}", crate::reach::UNMEASURED)
                })
            })
            .collect();
        let mut reaching: Vec<String> = targets
            .iter()
            .copied()
            .filter(|target| covering.contains(target) || unmeasured.contains(target))
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
    validated: Validated,
    targets: Vec<TestTarget>,
    scratch: PathBuf,
    /// How many executions this session has started, which is what names each one's own temporary directory.
    executions: std::sync::atomic::AtomicU64,
    mutant_timeout: Option<Duration>,
    /// The files as they were before instrumentation, so a position can be counted in the file a person would open rather than in the rewrite.
    sources: BTreeMap<String, Vec<u8>>,
    /// Which package each mutant belongs to.
    packages: BTreeMap<u32, String>,
    /// The branch proof of every mutant that has one, by catalog index.
    proofs: BTreeMap<u32, crate::syntax::branch::Proof>,
    reached: crate::reach::Reached,
    /// What the probe pass established, empty when it did not run.
    probed: crate::probe::tree::Probed,
    /// How long each target's own baseline took, which is what a derived timeout is a multiple of. Empty when nothing was verified.
    baseline: BTreeMap<String, Duration>,
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

    /// The test binaries this session built.
    #[must_use]
    pub fn targets(&self) -> &[TestTarget] {
        &self.targets
    }

    /// The digest of the tree as it was instrumented.
    #[must_use]
    pub fn workspace_digest(&self) -> &str {
        self.workspace.workspace_digest()
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

    /// The targets whose measured run covered this mutant, in identity order, or nothing when the measurement never instrumented the place.
    fn covering(&self, mutant: &Mutant) -> Option<Vec<&str>> {
        if !self.reached.measured() {
            return None;
        }
        let position = self.position(mutant)?;
        self.reached.covering(
            std::path::Path::new(&mutant.candidate.path),
            crate::coverage::Point {
                line: position.line,
                column: position.byte_column,
            },
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
        Route::decide(
            &self.reached,
            std::path::Path::new(&mutant.candidate.path),
            crate::coverage::Point {
                line: position.line,
                column: position.byte_column,
            },
            &targets,
        )
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
        let mutant = self.resolve(&request.mutant)?;
        let targets = self.selected(request.target.as_deref())?;
        let targets = match request
            .target
            .is_none()
            .then(|| self.covering(mutant))
            .flatten()
        {
            Some(covering) => {
                let routed: Vec<&TestTarget> = targets
                    .into_iter()
                    .filter(|target| covering.contains(&target.id.as_str()))
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
        let timeout = request.timeout.or(self.mutant_timeout);
        let mut last = None;
        let mut silent = None;
        for target in targets {
            let mut exec = ExecRequest::new(target)
                .with_args(request.args.clone())
                .with_timeout(timeout)
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
        last.or(silent)
            .ok_or_else(|| EngineError::from(SessionError::NoTargets))
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
        let timeout = request.timeout.or(self.mutant_timeout);
        let mut last = None;
        let mut silent = None;
        for target in targets {
            let mut exec = ExecRequest::new(target)
                .with_args(request.args.clone())
                .with_timeout(timeout)
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
        last.or(silent)
            .ok_or_else(|| EngineError::from(SessionError::NoTargets))
    }

    /// Every way the snapshot stopped matching the tree that was instrumented: what a test wrote into the tree every later mutant is measured against.
    ///
    /// # Errors
    /// The snapshot's walk failures and refusals.
    pub fn changes(&self) -> Result<Vec<Drift>, EngineError> {
        Ok(self.workspace.snapshot.redigest()?)
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
            return Err(EngineError::from(SessionError::NoTargets));
        }
        let Some(name) = name else {
            return Ok(self.targets.iter().collect());
        };
        let matching: Vec<&TestTarget> = self
            .targets
            .iter()
            .filter(|target| target.id == name || target.name == name)
            .collect();
        if matching.is_empty() {
            return Err(EngineError::from(SessionError::UnknownTarget {
                name: name.to_owned(),
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
        },
        &trace,
    )?;
    phase.end();
    Ok(discovery)
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
        return Err(EngineError::from(SessionError::NoTargets));
    }
    let members: Vec<&crate::cargo::Package> = workspace
        .metadata
        .members()
        .filter(|package| options.packages.is_empty() || options.packages.contains(&package.name))
        .collect();
    targets.extend(execute::documentation_targets(
        &members,
        workspace.toolchain.cargo(),
        &documentation_arguments(workspace),
    ));
    trace.build(BuildRecord {
        targets: targets.iter().map(|target| target.id.clone()).collect(),
    });
    let scratch = workspace.target_dir.join("scratch");
    std::fs::create_dir_all(&scratch).map_err(|source| SessionError::WriteFailed {
        path: scratch.display().to_string(),
        source,
    })?;
    let baseline = if options.verify {
        verify(workspace, &targets, &scratch, cancel)?
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
    let proofs = crate::prove::establish(asking, cancel, trace)?;
    let reached = crate::reach::establish(
        &crate::reach::Asking {
            workspace: asking.workspace,
            options: asking.options,
        },
        cancel,
        trace,
    )?;
    Ok((probed, proofs, reached))
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
    !matches!(
        (result.outcome, result.tests_run),
        (crate::outcome::Outcome::Inconclusive, Some(0))
    )
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
    }
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
    let pristine_phase = trace.phase("pristine");
    let checked = pristine(&workspace, options, cancel)?;
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
        },
        &trace,
    )?;

    discover_phase.end();

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
    workspace.snapshot.reseal()?;
    let build_phase = trace.phase("build");
    let (targets, scratch, baseline) = built(&workspace, &last_build, options, (cancel, &trace))?;
    build_phase.end();
    phase.end();
    let packages = discovery
        .candidates
        .iter()
        .filter_map(|located| {
            let id = located.found.candidate.id().ok()?;
            let mutant = discovery.catalog.by_id(&id)?;
            Some((mutant.index, located.package.clone()))
        })
        .collect();
    Ok(Session {
        catalog: discovery.catalog,
        skips: discovery.skips,
        sources,
        packages,
        proofs,
        reached,
        probed,
        validated,
        targets,
        scratch,
        baseline,
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
    };
    let validated = validate(
        &discovery.catalog,
        &mut writer,
        ValidateOptions {
            max_rounds: options.max_rounds,
        },
        trace,
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
    targets: &[TestTarget],
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
    for target in targets {
        let request = ExecRequest::new(target).with_scratch(scratch);
        let result = execute::exec(&request, &context, cancel, &workspace.trace);
        workspace.trace.verify(crate::trace::VerifyRecord {
            target: target.id.clone(),
            outcome: result.outcome.name().to_owned(),
            tests_run: result.tests_run,
            duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
        });
        let _kept = baseline.insert(target.id.clone(), result.duration);
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
}

impl Compile for TreeCompiler<'_> {
    fn attempt(
        &mut self,
        condemned: &std::collections::BTreeSet<u32>,
    ) -> Result<Attempt, ValidateError> {
        let mut files: Vec<FileOutput> = Vec::new();
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
            std::fs::write(self.workspace.snapshot_root().join(path), &file.text).map_err(
                |error| ValidateError::AttemptFailed {
                    message: format!("cannot write {path}: {error}"),
                },
            )?;
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
        })
    }
}

/// The name a target id takes, re-exported so a caller can build one without knowing the shape.
#[must_use]
pub fn target_name(package: &str, kind: TargetKind, name: &str) -> String {
    target_id(package, kind, name)
}
