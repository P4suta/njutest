// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A prepared workspace: every accepted mutant instrumented into one build,
//! and the test binaries that build produced.
//!
//! [`prepare`] runs the phases in order — discover, instrument, validate,
//! build, verify — and each one's refusals are kept rather than smoothed
//! over: a candidate the compiler refused is a [`Rejection`] with the
//! compiler's own words, a place discovery passed over is a [`Skip`] with
//! its reason, and a tree that fails its own tests before any mutant is
//! live stops the run.
//!
//! A session is `Send + Sync` and every execution takes `&self`, so a
//! consumer may run mutants in parallel across the targets one build
//! produced.

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
use crate::syntax::{Found, Selection, Skip};
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
    /// Run every test target once with nothing active, and refuse to hand
    /// back a session whose instrumented baseline does not pass.
    pub verify: bool,
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
            max_rounds: crate::validate::DEFAULT_MAX_ROUNDS,
            build_timeout: None,
            mutant_timeout: None,
        }
    }
}

/// One mutant execution to make.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// The mutant, by full identity or by any prefix of at least four hex
    /// characters that names exactly one.
    pub mutant: String,
    /// The target to run it against. `None` runs every target until one
    /// kills it, which is what "does any test catch this?" means.
    pub target: Option<String>,
    /// One test to run, by its libtest path. `None` runs the whole target.
    pub test: Option<String>,
    /// Further arguments for the harness.
    pub args: Vec<String>,
    /// How long the process may take. `None` uses the session's default.
    pub timeout: Option<Duration>,
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
    mutant_timeout: Option<Duration>,
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

    /// The mutant a prefix names.
    ///
    /// # Errors
    ///
    /// [`SessionError::UnknownMutant`] when no mutant matches, when several
    /// do, or when the prefix is too short to be worth resolving.
    pub fn resolve(&self, prefix: &str) -> Result<&Mutant, EngineError> {
        self.catalog.resolve_prefix(prefix).map_err(|error| {
            EngineError::from(SessionError::UnknownMutant {
                message: error.to_string(),
            })
        })
    }

    /// Runs one mutant and reports what the tests said.
    ///
    /// With no target named, every target runs until one kills the mutant:
    /// a mutant is dead as soon as any test notices it, and running the
    /// rest afterwards would only cost time.
    ///
    /// # Errors
    ///
    /// [`SessionError::UnknownMutant`], [`SessionError::UnknownTarget`], and
    /// [`SessionError::NoTargets`].
    pub fn exec(&self, request: &Request, cancel: &Cancel) -> Result<MutantResult, EngineError> {
        let mutant = self.resolve(&request.mutant)?;
        let targets = self.selected(request.target.as_deref())?;
        let context = Context {
            base_env: &self.workspace.base_env,
            active: Some((mutant.id.as_str(), self.catalog.digest())),
        };
        let timeout = request.timeout.or(self.mutant_timeout);
        let mut last = None;
        for target in targets {
            let mut exec = ExecRequest::new(target)
                .with_args(request.args.clone())
                .with_timeout(timeout)
                .with_scratch(&self.scratch);
            if let Some(test) = &request.test {
                exec = exec.with_test(test.clone());
            }
            let result = execute::exec(&exec, &context, cancel, &self.workspace.trace);
            if result.outcome.detected() || cancel.is_cancelled() {
                return Ok(result);
            }
            last = Some(result);
        }
        last.ok_or_else(|| EngineError::from(SessionError::NoTargets))
    }

    /// Every way the snapshot stopped matching the tree that was
    /// instrumented: what a test wrote into the tree every later mutant is
    /// measured against.
    ///
    /// # Errors
    ///
    /// The snapshot's walk failures and refusals.
    pub fn changes(&self) -> Result<Vec<Drift>, EngineError> {
        Ok(self.workspace.snapshot.redigest()?)
    }

    /// Removes the snapshot, or preserves it, and reports what was kept.
    ///
    /// # Errors
    ///
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

/// Discovers, instruments, validates, builds, and verifies.
///
/// # Errors
///
/// Every failure of the phases it runs.
pub fn prepare(
    workspace: Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<Session, EngineError> {
    let trace = workspace.trace.clone();
    let phase = trace.phase("prepare");
    let registry = Registry::canonical();
    let selection = if options.operators.is_empty() {
        Selection::tier(&registry, options.tier)
    } else {
        let names: Vec<&str> = options.operators.iter().map(String::as_str).collect();
        Selection::rules(&registry, &names)?
    };

    let checked = compile(
        &workspace.driver(cancel),
        &CompileOptions {
            kind: CompileKind::Check,
            target_dir: Some(workspace.target_dir.clone()),
            locked: workspace.locked,
            offline: workspace.offline,
            timeout: Workspace::timeout(options.build_timeout),
        },
    )?;
    if !checked.success {
        return Err(EngineError::from(SessionError::PristineBroken {
            first: crate::validate::first_error_of(&checked.messages),
        }));
    }

    let discovery = discover::discover(
        &discover::Input {
            root: workspace.snapshot_root(),
            metadata: &workspace.metadata,
            units: &checked.units,
        },
        &DiscoverOptions {
            selection,
            include: options.include.clone(),
            exclude: options.exclude.clone(),
            packages: options.packages.clone(),
        },
        &trace,
    )?;

    let (sources, placements) = plan_tree(workspace.snapshot_root(), &discovery)?;

    let (validated, last_build) = establish(
        &workspace,
        &discovery,
        &sources,
        &placements,
        options,
        cancel,
        &trace,
    )?;

    // The rewrite was intended, so it stops being drift: from here, drift
    // means a test wrote into the tree every later mutant is measured
    // against.
    let mut workspace = workspace;
    workspace.snapshot.reseal()?;

    // The last attempt validation made is the build that compiled, so the
    // binaries it produced are the ones this session runs: no second build,
    // and no chance of the two disagreeing.
    let targets = execute::targets_of(
        &last_build,
        &workspace.metadata.packages,
        Some(&workspace.target_dir),
    );
    if targets.is_empty() {
        return Err(EngineError::from(SessionError::NoTargets));
    }

    let scratch = workspace.target_dir.join("scratch");
    std::fs::create_dir_all(&scratch).map_err(|source| SessionError::WriteFailed {
        path: scratch.display().to_string(),
        source,
    })?;

    if options.verify {
        verify(&workspace, &targets, &scratch, cancel)?;
    }
    phase.end();
    Ok(Session {
        catalog: discovery.catalog,
        skips: discovery.skips,
        validated,
        targets,
        scratch,
        mutant_timeout: options.mutant_timeout,
        workspace,
    })
}

/// Instruments the tree and lets the compiler say which mutants are real,
/// returning what it established and the build it ended with.
#[allow(
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

/// Reads every mutable file of the snapshot and pairs its candidates with
/// their catalog entries, which is everything instrumentation needs.
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

/// Runs every target once with nothing active. A tree whose instrumented
/// baseline fails is one whose every later result would be about the
/// instrumentation rather than about a mutant.
fn verify(
    workspace: &Workspace,
    targets: &[TestTarget],
    scratch: &std::path::Path,
    cancel: &Cancel,
) -> Result<(), EngineError> {
    let phase = workspace.trace.phase("verify");
    let context = Context {
        base_env: &workspace.base_env,
        active: None,
    };
    for target in targets {
        let request = ExecRequest::new(target).with_scratch(scratch);
        let result = execute::exec(&request, &context, cancel, &workspace.trace);
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
    Ok(())
}

/// Instruments the snapshot with a set of mutants left out and compiles it:
/// the [`Compile`] seam validation drives.
struct TreeCompiler<'a> {
    workspace: &'a Workspace,
    sources: &'a BTreeMap<String, Vec<u8>>,
    placements: &'a BTreeMap<String, Vec<Placement>>,
    catalog: &'a Catalog,
    cancel: &'a Cancel,
    timeout: Option<Duration>,
    /// The messages of the last attempt that compiled, which name the test
    /// binaries this session will run.
    last_build: Vec<crate::cargo::Message>,
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
            std::fs::write(self.workspace.snapshot_root().join(path), &file.text).map_err(
                |error| ValidateError::AttemptFailed {
                    message: format!("cannot write {path}: {error}"),
                },
            )?;
            files.push(file);
        }
        // The build validation ends with is the build the mutants execute:
        // some refusals only happen once code is generated, so checking
        // alone would accept a mutant the run cannot build.
        let compiled = compile(
            &self.workspace.driver(self.cancel),
            &CompileOptions {
                kind: CompileKind::Tests,
                target_dir: Some(self.workspace.target_dir.clone()),
                locked: self.workspace.locked,
                offline: self.workspace.offline,
                timeout: self.timeout,
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

/// The name a target id takes, re-exported so a caller can build one
/// without knowing the shape.
#[must_use]
pub fn target_name(package: &str, kind: TargetKind, name: &str) -> String {
    target_id(package, kind, name)
}
