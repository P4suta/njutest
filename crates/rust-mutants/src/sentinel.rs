// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The mutants planted for each routing layer, and whether a prepared engine routes each of them the way its layer must.

use std::path::{Path, PathBuf};

use crate::EngineError;
use crate::cargo::{BuildConfig, Toolchain};
use crate::equivalence::{Identity, ProveOptions, Prover};
use crate::probe::Question;
use crate::runner::Cancel;
use crate::session::{Failing, LocateError, Locator, PrepareOptions, Proof, Route, Session};
use crate::workspace::{OpenOptions, Workspace};

/// The manifest of the planted crate.
const MANIFEST: &str = include_str!("sentinel/Cargo.toml.planted");

/// The manifest of the crate planted for the equivalence layer, built at the optimisation level at which the compiler renders its equivalent mutation identically.
const EQUIVALENCE_MANIFEST: &str = include_str!("sentinel/equivalence.Cargo.toml.planted");

/// The library of the crate planted for the equivalence layer, with the tests that call it.
const EQUIVALENCE_LIBRARY: &str = include_str!("sentinel/equivalence.lib.planted");

/// The reason a planted mutation the catalog does not hold establishes nothing.
pub const NOT_PLANTED: &str = "the planted crate holds no mutation of that rule in that item";

/// The lock file of the planted crate, which names nothing but the crate itself.
const LOCK: &str = include_str!("sentinel/Cargo.lock.planted");

/// Where the planted crate's library is written, relative to its root.
const LIBRARY: &str = "src/lib.rs";

/// Where the planted crate's tests are written, relative to its root.
const TESTS: &str = "tests/planted.rs";

/// A layer that removes executions from a run, one per form of evidence a proof reads, which is what a pair of mutants is planted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Planted {
    /// No measured test executes the mutation, so it is not run.
    Reach,
    /// No target entered the body a condition gates, so a mutation of the condition is removed by [`Proof::BranchNeverTaken`].
    Branch,
    /// Nothing a target ran ever saw a mutation differ from what it replaces, so it is removed by [`Proof::NeverInfected`].
    Infection(Infection),
    /// The coverage measurement alone places no target at a mutation, which is how a run routes when the guards recorded nothing.
    Coverage,
    /// The compiler renders a surviving mutation identically, so the equivalence layer removes it from what nothing noticed.
    Equivalence,
}

/// What [`Proof::NeverInfected`] reads to say a mutation never differed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Infection {
    /// A probe's answer to one question about the value a return replacement overwrites.
    Probe(Question),
    /// A guard's record of whether an inert comparison's two branches ever parted.
    Comparison,
}

/// What a layer's part of the planted crate holds: a library and the tests that exercise it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fragment {
    /// The items, as they are written into the library.
    library: &'static str,
    /// The tests, as they are written into the test target.
    tests: &'static str,
}

impl Planted {
    /// Every layer a pair is planted for: the ones every run's session is asked about, then the coverage route and the equivalence layer, which are asked of a run that could use them.
    #[must_use]
    pub fn every() -> Vec<Self> {
        let mut every = Self::routing();
        every.extend([Self::Coverage, Self::Equivalence]);
        every
    }

    /// The layers every run's session is asked about: the reach measurement, and every form of evidence of every proof.
    #[must_use]
    pub fn routing() -> Vec<Self> {
        let mut every = vec![Self::Reach];
        for proof in Proof::ALL {
            match proof {
                Proof::BranchNeverTaken => every.push(Self::Branch),
                Proof::NeverInfected => {
                    every.extend(
                        Question::ALL
                            .into_iter()
                            .map(|question| Self::Infection(Infection::Probe(question))),
                    );
                    every.push(Self::Infection(Infection::Comparison));
                }
            }
        }
        every
    }

    /// The proof that must remove this layer's planted mutant, or nothing for the reach measurement.
    #[must_use]
    pub const fn proof(self) -> Option<Proof> {
        match self {
            Self::Reach | Self::Coverage | Self::Equivalence => None,
            Self::Branch => Some(Proof::BranchNeverTaken),
            Self::Infection(_) => Some(Proof::NeverInfected),
        }
    }

    /// The name a trace record and a sentence carry.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Reach => "reach",
            Self::Branch => "branch-never-taken",
            Self::Infection(Infection::Probe(Question::Default)) => "never-infected:is-default",
            Self::Infection(Infection::Probe(Question::OkDefault)) => {
                "never-infected:is-ok-default"
            }
            Self::Infection(Infection::Probe(Question::SomeDefault)) => {
                "never-infected:is-some-default"
            }
            Self::Infection(Infection::Probe(Question::True)) => "never-infected:is-true",
            Self::Infection(Infection::Comparison) => "never-infected:inert-comparison",
            Self::Coverage => "coverage",
            Self::Equivalence => "equivalence",
        }
    }

    /// The part of the planted crate this layer answers for: the coverage route reads the reach layer's items by another measurement, and the equivalence layer has a crate of its own.
    const fn fragment(self) -> Fragment {
        match self {
            Self::Reach | Self::Coverage => Fragment {
                library: include_str!("sentinel/reach.lib.planted"),
                tests: include_str!("sentinel/reach.test.planted"),
            },
            Self::Branch => Fragment {
                library: include_str!("sentinel/branch-never-taken.lib.planted"),
                tests: include_str!("sentinel/branch-never-taken.test.planted"),
            },
            Self::Infection(Infection::Probe(Question::Default)) => Fragment {
                library: include_str!("sentinel/is-default.lib.planted"),
                tests: include_str!("sentinel/is-default.test.planted"),
            },
            Self::Infection(Infection::Probe(Question::OkDefault)) => Fragment {
                library: include_str!("sentinel/is-ok-default.lib.planted"),
                tests: include_str!("sentinel/is-ok-default.test.planted"),
            },
            Self::Infection(Infection::Probe(Question::SomeDefault)) => Fragment {
                library: include_str!("sentinel/is-some-default.lib.planted"),
                tests: include_str!("sentinel/is-some-default.test.planted"),
            },
            Self::Infection(Infection::Probe(Question::True)) => Fragment {
                library: include_str!("sentinel/is-true.lib.planted"),
                tests: include_str!("sentinel/is-true.test.planted"),
            },
            Self::Infection(Infection::Comparison) => Fragment {
                library: include_str!("sentinel/inert-comparison.lib.planted"),
                tests: include_str!("sentinel/inert-comparison.test.planted"),
            },
            Self::Equivalence => Fragment {
                library: EQUIVALENCE_LIBRARY,
                tests: "",
            },
        }
    }

    /// The pair planted for this layer: one rule, the item where the layer must remove its mutant, and the item where the same rule's mutant produces the evidence the layer reads.
    #[must_use]
    pub const fn pair(self) -> Pair {
        match self {
            Self::Reach | Self::Coverage => {
                Pair::new("return-default", "one", "two", KeptFor::Library)
            }
            Self::Equivalence => Pair::new("add-to-sub", "unchanged", "raised", KeptFor::Library),
            Self::Branch => Pair::new("le-to-lt", "clamp", "clamp_entered", KeptFor::Tests),
            Self::Infection(Infection::Probe(question)) => {
                let (removed, kept) = match question {
                    Question::Default => ("zero", "seven"),
                    Question::OkDefault => ("ok_zero", "ok_seven"),
                    Question::SomeDefault => ("some_zero", "some_seven"),
                    Question::True => ("small", "tiny"),
                };
                Pair::new(question.rule(), removed, kept, KeptFor::Tests)
            }
            Self::Infection(Infection::Comparison) => {
                Pair::new("le-to-lt", "pick", "pick_tied", KeptFor::Tests)
            }
        }
    }

    /// The mutant this layer must remove, and the one of the same rule it must leave to the target that reached it.
    ///
    /// Both are made by one rule because [`Pair`] holds one, so the kept mutant can only differ from the removed one in the evidence its test produces: a layer whose recorder went blind removes it as well, which is the direction a false survivor comes from.
    #[must_use]
    pub const fn expectations(self) -> [Expectation; 2] {
        let pair = self.pair();
        let (removed, kept) = match self {
            Self::Equivalence => (Expected::Identical, Expected::Rendered),
            Self::Reach | Self::Branch | Self::Infection(_) | Self::Coverage => (
                match self.proof() {
                    None => Expected::Unreached,
                    Some(proof) => Expected::Discharged(proof),
                },
                Expected::Kept(pair.kept_for),
            ),
        };
        [
            Expectation {
                planted: self,
                mutant: Planting::new(pair.removed, pair.rule),
                expected: removed,
            },
            Expectation {
                planted: self,
                mutant: Planting::new(pair.kept, pair.rule),
                expected: kept,
            },
        ]
    }
}

/// Two planted mutants of one rule: one a layer must remove, and one it must leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    /// The rule that makes both.
    pub rule: &'static str,
    /// The item whose mutant the layer must remove.
    pub removed: &'static str,
    /// The item whose mutant the layer must leave, whose test produces the evidence the layer reads.
    pub kept: &'static str,
    /// The target the kept mutant must be put to.
    pub kept_for: KeptFor,
}

impl Pair {
    /// The pair `rule` makes in `removed` and `kept`, the second put to `kept_for`.
    #[must_use]
    pub const fn new(
        rule: &'static str,
        removed: &'static str,
        kept: &'static str,
        kept_for: KeptFor,
    ) -> Self {
        Self {
            rule,
            removed,
            kept,
            kept_for,
        }
    }
}

impl std::fmt::Display for Planted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl serde::Serialize for Planted {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> serde::Deserialize<'de> for Planted {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::every()
            .into_iter()
            .find(|planted| planted.name() == name)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("{name:?} is not a layer a mutant is planted for"))
            })
    }
}

/// Which planted mutant an expectation is about, by the item it sits in and the rule that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Planting {
    /// The item, as a reader writes it.
    pub item: &'static str,
    /// The rule's name.
    pub rule: &'static str,
}

impl Planting {
    /// The mutant `rule` makes in `item` of the planted library.
    #[must_use]
    pub const fn new(item: &'static str, rule: &'static str) -> Self {
        Self { item, rule }
    }

    /// The locator that names it in a session over the planted crate.
    fn locator(self) -> Locator {
        Locator {
            path: LIBRARY.to_owned(),
            item: self.item.to_owned(),
            rule: self.rule.to_owned(),
            original: String::new(),
            line: None,
            count: None,
        }
    }
}

impl std::fmt::Display for Planting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{LIBRARY}:{}:{}", self.item, self.rule)
    }
}

/// Which target of the planted crate a kept mutant must be put to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum KeptFor {
    /// The unit tests compiled into the library.
    Library,
    /// The integration tests beside it.
    Tests,
}

impl KeptFor {
    /// The target the engine names it by.
    #[must_use]
    pub const fn target(self) -> &'static str {
        match self {
            Self::Library => "sentinel/lib/sentinel",
            Self::Tests => "sentinel/test/planted",
        }
    }
}

/// How a planted mutant must be routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// No target reaches it.
    Unreached,
    /// Every target that could have noticed it was removed by this proof.
    Discharged(Proof),
    /// The measurement put it to this target and nothing removed that target.
    Kept(KeptFor),
    /// The equivalence layer calls it identical, so it removes it from what nothing noticed.
    Identical,
    /// The equivalence layer finds the compiler renders it, so it leaves it where it was.
    Rendered,
}

impl Expected {
    /// Every answer an expectation can demand: no target, each proof, each target it can be kept for, and each thing the equivalence layer can say.
    #[must_use]
    pub fn every() -> Vec<Self> {
        let every: Vec<Self> = std::iter::once(Self::Unreached)
            .chain(Proof::ALL.into_iter().map(Self::Discharged))
            .chain(KeptFor::ALL.into_iter().map(Self::Kept))
            .chain([Self::Identical, Self::Rendered])
            .collect();
        for expected in &every {
            match expected {
                Self::Unreached
                | Self::Discharged(_)
                | Self::Kept(_)
                | Self::Identical
                | Self::Rendered => {}
            }
        }
        every
    }

    /// Whether what the equivalence layer said of a planted mutant is the answer this expects.
    ///
    /// A layer a control withdrew calls nothing identical, so it removes nothing, and neither answer counts against it; a layer still in service must call the equivalent mutation identical and must never call the rendered one so.
    #[must_use]
    pub fn compares(self, compared: &Compared) -> bool {
        match self {
            Self::Identical => compared.identity == Identity::Identical || compared.withdrawn,
            Self::Rendered => {
                compared.identity != Identity::Identical
                    && (compared.identity == Identity::Differs || compared.withdrawn)
            }
            Self::Unreached | Self::Discharged(_) | Self::Kept(_) => false,
        }
    }

    /// The name a trace record carries: `unreached`, the proof that must discharge it, or the target it must be kept for.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unreached => "unreached",
            Self::Discharged(proof) => proof.name(),
            Self::Kept(KeptFor::Library) => "kept-for-library",
            Self::Kept(KeptFor::Tests) => "kept-for-tests",
            Self::Identical => "identical",
            Self::Rendered => "rendered",
        }
    }

    /// Whether `route` is the routing this expects.
    #[must_use]
    pub fn holds(self, route: &Route) -> bool {
        match (self, route) {
            (Self::Unreached, Route::Unreached { .. }) => true,
            (Self::Discharged(proof), Route::Discharged { discharged }) => {
                !discharged.is_empty() && discharged.iter().all(|one| one.proof == proof)
            }
            (
                Self::Kept(kept_for),
                Route::Block {
                    reaching,
                    discharged,
                    fallback: None,
                },
            ) => {
                reaching.iter().any(|one| one.target == kept_for.target())
                    && !discharged.iter().any(|one| one.target == kept_for.target())
            }
            (
                Self::Unreached
                | Self::Discharged(_)
                | Self::Kept(_)
                | Self::Identical
                | Self::Rendered,
                Route::All { .. }
                | Route::Block { .. }
                | Route::Discharged { .. }
                | Route::Unreached { .. },
            ) => false,
        }
    }
}

impl std::fmt::Display for Expected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreached => f.write_str("unreached"),
            Self::Discharged(proof) => write!(f, "discharged by {proof}"),
            Self::Kept(kept_for) => write!(f, "kept for {}", kept_for.target()),
            Self::Identical => f.write_str("called identical by the equivalence layer"),
            Self::Rendered => f.write_str("rendered, so not called identical"),
        }
    }
}

impl serde::Serialize for Expected {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> serde::Deserialize<'de> for Expected {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::every()
            .into_iter()
            .find(|expected| expected.name() == name)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("{name:?} is not a routing a sentinel expects"))
            })
    }
}

/// One planted mutant and how the layer it was planted for must route it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expectation {
    /// The layer it was planted for.
    pub planted: Planted,
    /// The mutant.
    pub mutant: Planting,
    /// How it must be routed.
    pub expected: Expected,
}

/// What the equivalence layer said of one planted mutant, and whether a control had withdrawn it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compared {
    /// What comparing the two builds established.
    pub identity: Identity,
    /// Whether a control had withdrawn the layer, so that it calls nothing identical.
    pub withdrawn: bool,
}

/// What the engine did with one planted mutant: a route for a layer that routes, and a comparison for the equivalence layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The route the session decided, or why the catalog held no one mutant to route.
    Routed(Result<Route, LocateError>),
    /// What the equivalence layer said of it.
    Compared(Compared),
}

/// What a prepared session did with one planted mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    /// What was expected of it.
    pub expectation: Expectation,
    /// What the engine did with it.
    pub answer: Answer,
}

impl Sighting {
    /// Whether the engine answered the planted mutant the way its layer must.
    #[must_use]
    pub fn sighted(&self) -> bool {
        match &self.answer {
            Answer::Routed(route) => route
                .as_ref()
                .is_ok_and(|route| self.expectation.expected.holds(route)),
            Answer::Compared(compared) => self.expectation.expected.compares(compared),
        }
    }

    /// What the engine did with it, as a reader is told.
    #[must_use]
    pub fn routed(&self) -> String {
        let route = match &self.answer {
            Answer::Routed(Ok(route)) => route,
            Answer::Routed(Err(error)) => return format!("not routed: {error}"),
            Answer::Compared(compared) => {
                let withdrawn = if compared.withdrawn {
                    " (withdrawn by a control)"
                } else {
                    ""
                };
                return format!("{}{withdrawn}", compared.identity.name());
            }
        };
        let fallback = route
            .fallback()
            .map_or_else(String::new, |fallback| format!(" ({})", fallback.name()));
        let proofs: Vec<String> = route
            .discharged()
            .iter()
            .map(|one| format!("{} by {}", one.target, one.proof))
            .collect();
        let discharged = if proofs.is_empty() {
            String::new()
        } else {
            format!(": {}", proofs.join(", "))
        };
        format!("{}{fallback}{discharged}", route.granularity().name())
    }
}

/// Every planted mutant a session was asked about, and what the session preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighted {
    /// One per expectation of every planted layer, in the order [`Planted::every`] names them.
    pub sightings: Vec<Sighting>,
    /// The directories the session preserved because the workspace was opened to keep them.
    pub kept: Vec<PathBuf>,
}

impl Sighted {
    /// The first planted mutant its layer did not route as it must, or nothing when every layer did.
    #[must_use]
    pub fn blind(&self) -> Option<&Sighting> {
        self.sightings.iter().find(|one| !one.sighted())
    }
}

/// Why a planted crate could not be put where a session can open it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SentinelError {
    /// A file of the planted crate could not be written.
    #[error("the planted crate could not be written at {}: {source}", path.display())]
    Unwritable {
        /// The file or directory that could not be written.
        path: PathBuf,
        /// What the operating system said.
        #[source]
        source: std::io::Error,
    },
    /// The planted crate's session was built by another compiler than the one the tree under test resolves to.
    #[error(
        "the planted crate was built by `{planted}`, and the run by `{run}`, so what it showed about the layers is not about this run"
    )]
    OtherToolchain {
        /// What `rustc -vV` said for the tree under test.
        run: String,
        /// What it said for the planted crate.
        planted: String,
    },
}

impl SentinelError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::ErrorCode {
        match self {
            Self::Unwritable { .. } => crate::error::SENTINEL_UNWRITABLE,
            Self::OtherToolchain { .. } => crate::error::SENTINEL_OTHER_TOOLCHAIN,
        }
    }
}

/// The source of the planted library: every routing layer's items, in the order [`Planted::routing`] names the layers.
#[must_use]
pub fn library() -> String {
    Planted::routing()
        .into_iter()
        .map(|planted| planted.fragment().library)
        .collect::<Vec<&str>>()
        .join("\n")
}

/// The source of the planted tests: every routing layer's tests, in the same order.
#[must_use]
pub fn tests() -> String {
    Planted::routing()
        .into_iter()
        .map(|planted| planted.fragment().tests)
        .collect::<Vec<&str>>()
        .join("\n")
}

/// The source of the library planted for the equivalence layer, with the tests that call it.
#[must_use]
pub const fn equivalent_library() -> &'static str {
    Planted::Equivalence.fragment().library
}

/// Writes the planted crate under `root`, which is the directory a workspace is then opened at.
///
/// # Errors
/// Returns [`SentinelError::Unwritable`] when a directory or a file of it could not be written.
pub fn materialise(root: &Path) -> Result<(), SentinelError> {
    written(
        root,
        [
            ("Cargo.toml", MANIFEST.to_owned()),
            ("Cargo.lock", LOCK.to_owned()),
            (LIBRARY, library()),
            (TESTS, tests()),
        ],
    )
}

/// Writes the crate planted for the equivalence layer under `root`.
///
/// # Errors
/// Returns [`SentinelError::Unwritable`] when a directory or a file of it could not be written.
pub fn materialise_equivalent(root: &Path) -> Result<(), SentinelError> {
    written(
        root,
        [
            ("Cargo.toml", EQUIVALENCE_MANIFEST.to_owned()),
            ("Cargo.lock", LOCK.to_owned()),
            (LIBRARY, equivalent_library().to_owned()),
        ],
    )
}

/// Writes each of `files`, relative to `root`, with the directories it needs.
fn written<const N: usize>(root: &Path, files: [(&str, String); N]) -> Result<(), SentinelError> {
    for (relative, text) in files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SentinelError::Unwritable {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&path, text).map_err(|source| SentinelError::Unwritable {
            path: path.clone(),
            source,
        })?;
    }
    Ok(())
}

/// What of the caller's run the planted crate is prepared like: the toolchain it located, and the options it opens and prepares its own tree with.
#[derive(Debug)]
pub struct Run<'a> {
    /// The toolchain the tree under test resolved to.
    pub toolchain: &'a Toolchain,
    /// How the run opens a workspace.
    pub open: OpenOptions,
    /// How the run prepares one.
    pub options: &'a PrepareOptions,
    /// Whether the run asks the equivalence layer, which is the only run the layer is planted for, since planting it costs builds of a crate of its own.
    pub equivalence: bool,
}

/// The executables of the run's toolchain, named by path so that no toolchain file, rustup override or version-manager shim decides which compiler builds the planted crate.
struct Compiler {
    /// The cargo.
    cargo: PathBuf,
    /// The rustc cargo is told to drive.
    rustc: PathBuf,
    /// The rustdoc the doctests are run with.
    rustdoc: PathBuf,
}

impl Compiler {
    /// The binaries in `run`'s sysroot, or the ones it located where it names no sysroot.
    fn of(run: &Toolchain) -> Self {
        let binary = |name: &str| format!("{name}{}", std::env::consts::EXE_SUFFIX);
        match run.sysroot() {
            Some(sysroot) => {
                let bin = sysroot.join("bin");
                Self {
                    cargo: bin.join(binary("cargo")),
                    rustc: bin.join(binary("rustc")),
                    rustdoc: bin.join(binary("rustdoc")),
                }
            }
            None => Self {
                cargo: run.cargo().to_path_buf(),
                rustc: run.rustc().to_path_buf(),
                rustdoc: run.rustc().with_file_name(binary("rustdoc")),
            },
        }
    }
}

/// The options a session over the planted crate is prepared with: every switch that decides a route as the caller set it, and none that describes the caller's own tree.
#[must_use]
pub fn routing(options: &PrepareOptions) -> PrepareOptions {
    let PrepareOptions {
        tier: _,
        operators: _,
        scratch_working_directory,
        include: _,
        exclude: _,
        narrowing: _,
        packages: _,
        skips: _,
        measurements: _,
        verify: _,
        touch,
        coverage,
        branch_proofs,
        failing: _,
        max_rounds,
        build_timeout,
        mutant_timeout,
        mutant_steps,
        doctests,
        build,
        harness_args: _,
        skip_targets: _,
        validation_filter: _,
    } = options;
    let BuildConfig {
        features: _,
        all_features: _,
        no_default_features: _,
        target,
        profile,
        jobs,
        debug,
    } = build;
    PrepareOptions {
        tier: crate::rule::Tier::Balanced,
        operators: Vec::new(),
        scratch_working_directory: *scratch_working_directory,
        include: Vec::new(),
        exclude: Vec::new(),
        narrowing: Vec::new(),
        packages: Vec::new(),
        skips: Vec::new(),
        measurements: None,
        verify: true,
        touch: *touch,
        coverage: *coverage,
        branch_proofs: *branch_proofs,
        failing: Failing::Refuse,
        max_rounds: *max_rounds,
        build_timeout: *build_timeout,
        mutant_timeout: *mutant_timeout,
        mutant_steps: *mutant_steps,
        doctests: *doctests,
        build: BuildConfig {
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            target: target.clone(),
            profile: profile.clone(),
            jobs: *jobs,
            debug: *debug,
        },
        harness_args: Vec::new(),
        skip_targets: Vec::new(),
        validation_filter: None,
    }
}

/// How `session` routes the mutant `expectation` names, without running it.
#[must_use]
pub fn sight(session: &Session, expectation: Expectation) -> Sighting {
    Sighting {
        expectation,
        answer: Answer::Routed(
            session
                .locate(&expectation.mutant.locator())
                .map(|mutant| session.route(mutant)),
        ),
    }
}

/// How `session`'s coverage measurement alone routes the mutant `expectation` names, which is how the run routes where the guards recorded nothing.
#[must_use]
pub fn sight_by_coverage(session: &Session, expectation: Expectation) -> Sighting {
    Sighting {
        expectation,
        answer: Answer::Routed(
            session
                .locate(&expectation.mutant.locator())
                .map(|mutant| session.route_by_coverage(mutant)),
        ),
    }
}

/// What the equivalence layer, opened over the crate planted for it at `root`, says of each of its planted mutants.
///
/// # Errors
/// Returns the failure to write the crate, or whatever stopped the layer copying or building it.
pub fn compared(
    root: &Path,
    options: &ProveOptions,
    cancel: &Cancel,
) -> Result<Vec<Sighting>, EngineError> {
    static REGISTRY: crate::rule::Registry = crate::rule::Registry::canonical();
    materialise_equivalent(root)?;
    let selection = crate::syntax::Selection::tier(&REGISTRY, crate::rule::Tier::All);
    let candidates =
        match crate::syntax::discover_file(LIBRARY, equivalent_library().as_bytes(), &selection) {
            Ok(found) => found.candidates,
            Err(_not_planted) => Vec::new(),
        };
    let mut prover = Prover::open(root, options, cancel, &crate::trace::Recorder::disabled())?;
    let mut sightings = Vec::new();
    for expectation in Planted::Equivalence.expectations() {
        let planted = candidates.iter().find(|one| {
            one.item == expectation.mutant.item
                && one.candidate.rule.name == expectation.mutant.rule
        });
        let identity = match planted {
            Some(one) => prover.identical(&one.candidate, cancel)?,
            None => Identity::NotEstablished(NOT_PLANTED),
        };
        sightings.push(Sighting {
            expectation,
            answer: Answer::Compared(Compared {
                identity,
                withdrawn: prover.withdrawn(),
            }),
        });
    }
    prover.close()?;
    Ok(sightings)
}

/// Where the crate planted for the equivalence layer is written, beside the one planted for the routing layers at `root`.
fn equivalent_root(root: &Path) -> PathBuf {
    let mut named = root.as_os_str().to_owned();
    named.push("-equivalence");
    PathBuf::from(named)
}

/// Plants a crate under `root`, prepares it the way `options` prepares the caller's tree, and asks the session how it routes every planted mutant, running none of them.
///
/// # Errors
/// Returns the failure to write the planted crate, or whatever stopped the engine opening or preparing it.
pub fn sighted(run: Run<'_>, root: &Path, cancel: &Cancel) -> Result<Sighted, EngineError> {
    let Run {
        toolchain: run,
        open,
        options,
        equivalence,
    } = run;
    materialise(root)?;
    let compiler = Compiler::of(run);
    let mut env = open.env;
    env.set("RUSTC", compiler.rustc);
    env.set("RUSTDOC", compiler.rustdoc);
    let planting = OpenOptions {
        cargo: Some(compiler.cargo),
        env,
        report_directory: None,
        exclude: Vec::new(),
        allow_outside: Vec::new(),
        ..open
    };
    let proving = planting.clone();
    let workspace = Workspace::open(root, planting, cancel)?;
    let planted = workspace.toolchain().rustc_version();
    if planted != run.rustc_version() {
        return Err(SentinelError::OtherToolchain {
            run: run.rustc_version().summary.clone(),
            planted: planted.summary.clone(),
        }
        .into());
    }
    let routed = routing(options);
    let session = workspace.prepare(&routed, cancel)?;
    let mut sightings: Vec<Sighting> = Planted::routing()
        .into_iter()
        .flat_map(Planted::expectations)
        .map(|expectation| sight(&session, expectation))
        .collect();
    if session.measured_by_coverage() {
        sightings.extend(
            Planted::Coverage
                .expectations()
                .into_iter()
                .map(|expectation| sight_by_coverage(&session, expectation)),
        );
    }
    let kept = session.close()?;
    if equivalence {
        sightings.extend(compared(
            &equivalent_root(root),
            &ProveOptions {
                open: proving,
                timeout: routed.build_timeout,
                build: routed.build,
            },
            cancel,
        )?);
    }
    Ok(Sighted { sightings, kept })
}
