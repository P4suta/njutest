// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The mutants planted for each routing layer, and whether a prepared engine routes each of them the way its layer must.

use std::path::{Path, PathBuf};

use crate::EngineError;
use crate::cargo::BuildConfig;
use crate::runner::Cancel;
use crate::session::{Failing, LocateError, Locator, PrepareOptions, Proof, Route, Session};
use crate::workspace::{OpenOptions, Workspace};

/// The manifest of the planted crate.
const MANIFEST: &str = include_str!("sentinel/Cargo.toml.planted");

/// The lock file of the planted crate, which names nothing but the crate itself.
const LOCK: &str = include_str!("sentinel/Cargo.lock.planted");

/// Where the planted crate's library is written, relative to its root.
const LIBRARY: &str = "src/lib.rs";

/// Where the planted crate's tests are written, relative to its root.
const TESTS: &str = "tests/planted.rs";

/// A layer that removes executions from a run, which is what a mutant is planted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Planted {
    /// No measured test executes the mutation, so it is not run.
    Reach,
    /// A proof removed every target that could have noticed the mutation.
    Proof(Proof),
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
    /// Every layer a mutant is planted for, one per proof and the reach measurement.
    #[must_use]
    pub fn every() -> Vec<Self> {
        let every: Vec<Self> = std::iter::once(Self::Reach)
            .chain(Proof::ALL.into_iter().map(Self::Proof))
            .collect();
        for planted in &every {
            match planted {
                Self::Reach | Self::Proof(_) => {}
            }
        }
        every
    }

    /// The name a trace record and a sentence carry.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Reach => "reach",
            Self::Proof(proof) => proof.name(),
        }
    }

    /// The part of the planted crate this layer answers for.
    const fn fragment(self) -> Fragment {
        match self {
            Self::Reach => Fragment {
                library: include_str!("sentinel/reach.lib.planted"),
                tests: include_str!("sentinel/reach.test.planted"),
            },
            Self::Proof(Proof::BranchNeverTaken) => Fragment {
                library: include_str!("sentinel/branch-never-taken.lib.planted"),
                tests: include_str!("sentinel/branch-never-taken.test.planted"),
            },
            Self::Proof(Proof::NeverInfected) => Fragment {
                library: include_str!("sentinel/never-infected.lib.planted"),
                tests: include_str!("sentinel/never-infected.test.planted"),
            },
        }
    }

    /// The mutant this layer must remove, and one beside it the layer must leave to the tests.
    #[must_use]
    pub const fn expectations(self) -> [Expectation; 2] {
        let (removed, kept) = match self {
            Self::Reach => (
                Planting::new("one", "return-default"),
                Planting::new("two", "return-default"),
            ),
            Self::Proof(Proof::BranchNeverTaken) => (
                Planting::new("clamp", "le-to-lt"),
                Planting::new("clamp", "condition-to-true"),
            ),
            Self::Proof(Proof::NeverInfected) => (
                Planting::new("at_most", "return-true"),
                Planting::new("at_most", "le-to-lt"),
            ),
        };
        let expected = match self {
            Self::Reach => Expected::Unreached,
            Self::Proof(proof) => Expected::Discharged(proof),
        };
        [
            Expectation {
                planted: self,
                mutant: removed,
                expected,
            },
            Expectation {
                planted: self,
                mutant: kept,
                expected: Expected::Kept,
            },
        ]
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

/// How a planted mutant must be routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// No target reaches it.
    Unreached,
    /// Every target that could have noticed it was removed by this proof.
    Discharged(Proof),
    /// The measurement placed it and nothing removed the target that reaches it.
    Kept,
}

impl Expected {
    /// Every routing an expectation can demand: no target, each proof, and the tests.
    #[must_use]
    pub fn every() -> Vec<Self> {
        let every: Vec<Self> = std::iter::once(Self::Unreached)
            .chain(Proof::ALL.into_iter().map(Self::Discharged))
            .chain(std::iter::once(Self::Kept))
            .collect();
        for expected in &every {
            match expected {
                Self::Unreached | Self::Discharged(_) | Self::Kept => {}
            }
        }
        every
    }

    /// The name a trace record carries: `unreached`, `kept`, or the proof that must discharge it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unreached => "unreached",
            Self::Discharged(proof) => proof.name(),
            Self::Kept => "kept",
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
                Self::Kept,
                Route::Block {
                    reaching,
                    discharged,
                    fallback: None,
                },
            ) => !reaching.is_empty() && discharged.is_empty(),
            (
                Self::Unreached | Self::Discharged(_) | Self::Kept,
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
            Self::Kept => f.write_str("kept for the tests that reach it"),
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

/// What a prepared session did with one planted mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    /// What was expected of it.
    pub expectation: Expectation,
    /// The route the session decided, or why the catalog held no one mutant to route.
    pub route: Result<Route, LocateError>,
}

impl Sighting {
    /// Whether the session routed the planted mutant the way its layer must.
    #[must_use]
    pub fn sighted(&self) -> bool {
        self.route
            .as_ref()
            .is_ok_and(|route| self.expectation.expected.holds(route))
    }

    /// How the session routed it, as a reader is told.
    #[must_use]
    pub fn routed(&self) -> String {
        let route = match &self.route {
            Ok(route) => route,
            Err(error) => return format!("not routed: {error}"),
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
}

impl SentinelError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::ErrorCode {
        match self {
            Self::Unwritable { .. } => crate::error::SENTINEL_UNWRITABLE,
        }
    }
}

/// The source of the planted library: every layer's items, in the order [`Planted::every`] names the layers.
#[must_use]
pub fn library() -> String {
    Planted::every()
        .into_iter()
        .map(|planted| planted.fragment().library)
        .collect::<Vec<&str>>()
        .join("\n")
}

/// The source of the planted tests: every layer's tests, in the same order.
#[must_use]
pub fn tests() -> String {
    Planted::every()
        .into_iter()
        .map(|planted| planted.fragment().tests)
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Writes the planted crate under `root`, which is the directory a workspace is then opened at.
///
/// # Errors
/// Returns [`SentinelError::Unwritable`] when a directory or a file of it could not be written.
pub fn materialise(root: &Path) -> Result<(), SentinelError> {
    let files = [
        ("Cargo.toml", MANIFEST.to_owned()),
        ("Cargo.lock", LOCK.to_owned()),
        (LIBRARY, library()),
        (TESTS, tests()),
    ];
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

/// The options a session over the planted crate is prepared with: every switch that decides a route as the caller set it, and none that describes the caller's own tree.
#[must_use]
pub fn routing(options: &PrepareOptions) -> PrepareOptions {
    let PrepareOptions {
        tier: _,
        operators: _,
        scratch_working_directory,
        include: _,
        exclude: _,
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
        route: session
            .locate(&expectation.mutant.locator())
            .map(|mutant| session.route(mutant)),
    }
}

/// Plants a crate under `root`, prepares it the way `options` prepares the caller's tree, and asks the session how it routes every planted mutant, running none of them.
///
/// # Errors
/// Returns the failure to write the planted crate, or whatever stopped the engine opening or preparing it.
pub fn sighted(
    root: &Path,
    open: OpenOptions,
    options: &PrepareOptions,
    cancel: &Cancel,
) -> Result<Sighted, EngineError> {
    materialise(root)?;
    let workspace = Workspace::open(
        root,
        OpenOptions {
            report_directory: None,
            exclude: Vec::new(),
            allow_outside: Vec::new(),
            ..open
        },
        cancel,
    )?;
    let session = workspace.prepare(&routing(options), cancel)?;
    let sightings = Planted::every()
        .into_iter()
        .flat_map(Planted::expectations)
        .map(|expectation| sight(&session, expectation))
        .collect();
    let kept = session.close()?;
    Ok(Sighted { sightings, kept })
}
