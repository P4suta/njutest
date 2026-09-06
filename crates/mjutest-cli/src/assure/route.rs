// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which tests could possibly notice one mutation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::assure::baseline::Measured;
use crate::coverage::{Block, Point};
use crate::report::TargetStatus;

/// One or more target identities, cheapest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaching(Vec<String>);

impl Reaching {
    /// The targets, or nothing when there are none.
    #[must_use]
    pub fn new(targets: Vec<String>) -> Option<Self> {
        if targets.is_empty() {
            None
        } else {
            Some(Self(targets))
        }
    }

    /// The identities, in the order they will run.
    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// Why a route gave up on the position and took the whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fallback {
    /// The catalog could not say where the mutant is.
    PositionUnknown,
    /// No instrumented region contains the position, which is a gap in the measurement rather than a fact about the code.
    OutsideBlocks,
}

impl Fallback {
    /// The wire name a trace and a report use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PositionUnknown => "position-unknown",
            Self::OutsideBlocks => "outside-blocks",
        }
    }

    /// One sentence a reader can act on.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::PositionUnknown => {
                "the catalog could not say where the mutation is, so every test that \
                 touched the file was run"
            }
            Self::OutsideBlocks => {
                "no instrumented region contains the position, which is a gap in the \
                 measurement rather than proof that nothing runs it, so every test that \
                 touched the file was run"
            }
        }
    }
}

/// Why a mutation no target reached is settled by running the package suite rather than reported as unreached.
///
/// Reporting that nothing reaches a position is a claim about the code, and it
/// rests on two premises: that instrumentation described the position, so a
/// target's silence about it is a fact rather than a gap, and that every
/// measured target carries coverage, so every target's silence is readable.
/// Where a premise fails the run has no proof and runs more, which is the
/// direction [ADR 0004](../../../../docs/adr/0004-proof-layers-not-budgets.md)
/// decision 2 requires of a fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unsettled {
    /// The catalog could not say where the mutation is, so no target's coverage is about it.
    PositionUnknown,
    /// No instrumented region contains the position, so nothing was measured about it.
    OutsideBlocks,
    /// A measured target carries no coverage at all, so its silence about the position is not evidence.
    CoverageIncomplete,
}

impl Unsettled {
    /// The wire name a trace and a report use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PositionUnknown => "position-unknown",
            Self::OutsideBlocks => "outside-blocks",
            Self::CoverageIncomplete => "coverage-incomplete",
        }
    }

    /// One sentence a reader can act on.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::PositionUnknown => {
                "the catalog could not say where the mutation is, so no test's coverage \
                 is about it and the package suite was run"
            }
            Self::OutsideBlocks => {
                "no instrumented region contains the position, so nothing was measured \
                 about it and the package suite was run"
            }
            Self::CoverageIncomplete => {
                "a measured test carries no coverage, so its silence about the position \
                 is not evidence and the package suite was run"
            }
        }
    }
}

/// The name a branch proof answers to in a route and in a recording.
pub const BRANCH_NEVER_TAKEN: &str = "branch-never-taken";

/// The name the probe pass answers to in a route and in a recording.
pub const NEVER_INFECTED: &str = "never-infected";

/// One target a proof removed from a reaching set, and the proof that removed it.
///
/// [ADR 0004](../../../../docs/adr/0004-proof-layers-not-budgets.md) decision 4
/// asks that every layer be visible, which means naming the proof beside the
/// target rather than counting removals: two layers answer for one route, and a
/// reader who cannot tell which of them removed a test cannot audit either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discharge {
    /// The target that was removed without being run.
    pub target: String,
    /// What removed it: [`BRANCH_NEVER_TAKEN`] or [`NEVER_INFECTED`].
    pub proof: &'static str,
}

/// How one mutant's tests were chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Route {
    /// The tests whose own coverage contains the mutant's position.
    Block {
        /// Them, cheapest first.
        reaching: Reaching,
        /// How many touched the file at all, which is what this narrowed down from.
        file_candidates: usize,
        /// The tests a proof removed without running them, in the order they would have run, each beside the proof that removed it.
        discharged: Vec<Discharge>,
    },
    /// Every test that could have noticed was discharged by a branch proof: that no test takes the branch the mutation narrows is the finding.
    Discharged {
        /// The tests that were removed without being run, each beside the proof that removed it.
        discharged: Vec<Discharge>,
        /// How many touched the file at all.
        file_candidates: usize,
    },
    /// Every test that touched the file, because the position could not narrow it with evidence.
    File {
        /// Them, cheapest first.
        reaching: Reaching,
        /// What the evidence could not support.
        fallback: Fallback,
    },
    /// The position is instrumented, every measured test carries coverage, and none reached it: the mutation lives in code the measured tests never execute.
    Unreached {
        /// How many tests touched the file without reaching the position.
        file_candidates: usize,
    },
    /// No test reached the position and the evidence does not carry that as a fact, so the package suite runs and settles it.
    Suite {
        /// Which premise of an unreached mutation failed.
        unsettled: Unsettled,
        /// How many tests touched the file.
        file_candidates: usize,
    },
}

impl Route {
    /// The wire name of how this was decided.
    #[must_use]
    pub const fn granularity(&self) -> &'static str {
        match self {
            Self::Block { .. } => "block",
            Self::Discharged { .. } => "discharged",
            Self::File { .. } => "file",
            Self::Unreached { .. } => "unreached",
            Self::Suite { .. } => "suite",
        }
    }

    /// The targets to run, which is nothing at all for an unreached mutant.
    #[must_use]
    pub fn reaching(&self) -> &[String] {
        match self {
            Self::Block { reaching, .. } | Self::File { reaching, .. } => reaching.as_slice(),
            Self::Discharged { .. } | Self::Unreached { .. } | Self::Suite { .. } => &[],
        }
    }

    /// Which premise of an unreached mutation failed, when the suite is what settles it.
    #[must_use]
    pub const fn unsettled(&self) -> Option<Unsettled> {
        match self {
            Self::Suite { unsettled, .. } => Some(*unsettled),
            Self::Block { .. }
            | Self::Discharged { .. }
            | Self::File { .. }
            | Self::Unreached { .. } => None,
        }
    }

    /// What widened this route beyond the position, by wire name: the fallback that took the whole file, or the premise that sent it to the package suite.
    #[must_use]
    pub fn widened(&self) -> Option<&'static str> {
        self.fallback()
            .map(Fallback::name)
            .or_else(|| self.unsettled().map(Unsettled::name))
    }

    /// Why the position did not decide it, when it did not.
    #[must_use]
    pub const fn fallback(&self) -> Option<Fallback> {
        match self {
            Self::File { fallback, .. } => Some(*fallback),
            Self::Block { .. }
            | Self::Discharged { .. }
            | Self::Unreached { .. }
            | Self::Suite { .. } => None,
        }
    }

    /// How many targets touched the file.
    #[must_use]
    pub fn file_candidates(&self) -> usize {
        match self {
            Self::Block {
                file_candidates, ..
            }
            | Self::Discharged {
                file_candidates, ..
            }
            | Self::Unreached { file_candidates }
            | Self::Suite {
                file_candidates, ..
            } => *file_candidates,
            Self::File { reaching, .. } => reaching.as_slice().len(),
        }
    }

    /// The tests a proof removed without running them, each beside the proof that removed it.
    #[must_use]
    pub fn discharged(&self) -> &[Discharge] {
        match self {
            Self::Block { discharged, .. } | Self::Discharged { discharged, .. } => discharged,
            Self::File { .. } | Self::Unreached { .. } | Self::Suite { .. } => &[],
        }
    }
}

/// What a branch proof narrows a route with.
#[derive(Debug, Clone, Copy)]
pub struct Proven<'a> {
    /// The file the mutation is in.
    pub path: &'a str,
    /// The body the narrowed condition gates.
    pub body: Body,
    /// What the baseline measured.
    pub baseline: &'a [Measured],
    /// Every region the build instrumented.
    pub instrumented: &'a BTreeSet<Block>,
}

/// The span of the body a narrowed condition gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body {
    /// Where the opening brace is.
    pub start: Point,
    /// Where the closing brace is.
    pub end: Point,
}

impl Body {
    /// Whether `point` lies inside the body.
    #[must_use]
    pub fn holds(&self, point: Point) -> bool {
        let after_start = (point.line, point.column) >= (self.start.line, self.start.column);
        let before_end = (point.line, point.column) <= (self.end.line, self.end.column);
        after_start && before_end
    }
}

/// Removes from `route` every target the probe recorded as never having infected this mutant.
///
/// A test that never reached the mutation with a state the mutation would have
/// changed cannot have killed it, however far it ran afterwards. The narrowing
/// applies only where the probe asked about this mutant at all, and only to
/// targets whose own log this run could read: a target the probe says nothing
/// about is a target that stays.
#[must_use]
pub fn uninfected(route: Route, mutant: u32, infected: &BTreeMap<String, BTreeSet<u32>>) -> Route {
    let Route::Block {
        reaching,
        file_candidates,
        discharged,
    } = route
    else {
        return route;
    };
    let mut kept = Vec::new();
    let mut removed = discharged;
    for id in reaching.as_slice() {
        match infected.get(id) {
            Some(seen) if !seen.contains(&mutant) => removed.push(Discharge {
                target: id.clone(),
                proof: NEVER_INFECTED,
            }),
            _ => kept.push(id.clone()),
        }
    }
    match Reaching::new(kept) {
        Some(reaching) => Route::Block {
            reaching,
            file_candidates,
            discharged: removed,
        },
        None => Route::Discharged {
            discharged: removed,
            file_candidates,
        },
    }
}

/// Removes from `route` every target that took the same branch on both programs.
///
/// C' implies C and the whole condition is inert, so a target during which no
/// statement of the gated body ran evaluated the condition to false every time
/// it was evaluated, evaluated the narrowed one to false there too, and ran
/// identically on the two programs. It cannot have observed the mutation.
///
/// The narrowing applies only where the evidence carries it: on a route decided
/// by region with no fallback, never on one decided by file, and only where the
/// body was instrumented at all — otherwise no target's silence about it means
/// anything. A target restored from a checkpoint carries no regions to argue
/// with and is never discharged.
#[must_use]
pub fn discharge(route: Route, proof: &Proven<'_>) -> Route {
    let Proven {
        path,
        body,
        baseline,
        instrumented,
    } = *proof;
    let Route::Block {
        reaching,
        file_candidates,
        discharged,
    } = route
    else {
        return route;
    };
    let file = Path::new(path);
    if !instrumented
        .iter()
        .any(|block| block.file == file && body.holds(block.start))
    {
        return Route::Block {
            reaching,
            file_candidates,
            discharged,
        };
    }
    let mut kept = Vec::new();
    let mut removed = discharged;
    for id in reaching.as_slice() {
        let Some(measured) = baseline.iter().find(|one| &one.target.id == id) else {
            kept.push(id.clone());
            continue;
        };
        let ran_the_body = measured
            .covered
            .iter()
            .any(|block| block.file == file && body.holds(block.start));
        if measured.restored || ran_the_body {
            kept.push(id.clone());
        } else {
            removed.push(Discharge {
                target: id.clone(),
                proof: BRANCH_NEVER_TAKEN,
            });
        }
    }
    match Reaching::new(kept) {
        Some(reaching) => Route::Block {
            reaching,
            file_candidates,
            discharged: removed,
        },
        None => Route::Discharged {
            discharged: removed,
            file_candidates,
        },
    }
}

/// The tests that could notice a mutation of `path` at `position`.
#[must_use]
pub fn route(
    path: &str,
    position: Option<Point>,
    baseline: &[Measured],
    instrumented: &BTreeSet<Block>,
) -> Route {
    let file = Path::new(path);
    let measured: Vec<&Measured> = baseline
        .iter()
        .filter(|measured| measured.status == TargetStatus::Passed)
        .collect();
    let candidates: Vec<&Measured> = measured
        .iter()
        .copied()
        .filter(|measured| measured.covered.iter().any(|block| block.file == file))
        .collect();

    let Some(position) = position else {
        return by_file(&candidates, Fallback::PositionUnknown);
    };
    if !instrumented
        .iter()
        .any(|block| block.contains(file, position))
    {
        return by_file(&candidates, Fallback::OutsideBlocks);
    }

    let reaching: Vec<&Measured> = candidates
        .iter()
        .copied()
        .filter(|measured| {
            measured
                .covered
                .iter()
                .any(|block| block.contains(file, position))
        })
        .collect();
    Reaching::new(cheapest_first(&reaching)).map_or_else(
        || nothing_reached(&measured, candidates.len()),
        |reaching| Route::Block {
            reaching,
            file_candidates: candidates.len(),
            discharged: Vec::new(),
        },
    )
}

/// What a run may say about a position no test reached: that nothing reaches it, where every measured test could have said so, and otherwise that the package suite has to answer.
fn nothing_reached(measured: &[&Measured], file_candidates: usize) -> Route {
    if measured.iter().any(|one| one.covered.is_empty()) {
        Route::Suite {
            unsettled: Unsettled::CoverageIncomplete,
            file_candidates,
        }
    } else {
        Route::Unreached { file_candidates }
    }
}

/// Every target that touched the file, and why the position did not narrow it. With no candidate at all there is nothing the fallback could have told us either, and an absence of evidence is not the proof an unreached mutation claims: the package suite settles it.
fn by_file(candidates: &[&Measured], fallback: Fallback) -> Route {
    Reaching::new(cheapest_first(candidates)).map_or(
        Route::Suite {
            unsettled: match fallback {
                Fallback::PositionUnknown => Unsettled::PositionUnknown,
                Fallback::OutsideBlocks => Unsettled::OutsideBlocks,
            },
            file_candidates: candidates.len(),
        },
        |reaching| Route::File { reaching, fallback },
    )
}

/// The identities of these targets, cheapest first, then by identity so two runs of the same work route the same way.
fn cheapest_first(targets: &[&Measured]) -> Vec<String> {
    let mut ordered: Vec<&&Measured> = targets.iter().collect();
    ordered.sort_by(|left, right| {
        left.duration_ms
            .cmp(&right.duration_ms)
            .then_with(|| left.target.id.cmp(&right.target.id))
    });
    ordered
        .into_iter()
        .map(|measured| measured.target.id.clone())
        .collect()
}
