// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which tests could possibly notice one mutation.

use std::collections::BTreeSet;
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
    },
    /// Every test that touched the file, because the position could not narrow it with evidence.
    File {
        /// Them, cheapest first.
        reaching: Reaching,
        /// What the evidence could not support.
        fallback: Fallback,
    },
    /// The position is instrumented and no test reached it: the mutation lives in code the measured tests never execute.
    Unreached {
        /// How many tests touched the file without reaching the position.
        file_candidates: usize,
    },
}

impl Route {
    /// The wire name of how this was decided.
    #[must_use]
    pub const fn granularity(&self) -> &'static str {
        match self {
            Self::Block { .. } => "block",
            Self::File { .. } => "file",
            Self::Unreached { .. } => "unreached",
        }
    }

    /// The targets to run, which is nothing at all for an unreached mutant.
    #[must_use]
    pub fn reaching(&self) -> &[String] {
        match self {
            Self::Block { reaching, .. } | Self::File { reaching, .. } => reaching.as_slice(),
            Self::Unreached { .. } => &[],
        }
    }

    /// Why the position did not decide it, when it did not.
    #[must_use]
    pub const fn fallback(&self) -> Option<Fallback> {
        match self {
            Self::File { fallback, .. } => Some(*fallback),
            Self::Block { .. } | Self::Unreached { .. } => None,
        }
    }

    /// How many targets touched the file.
    #[must_use]
    pub fn file_candidates(&self) -> usize {
        match self {
            Self::Block {
                file_candidates, ..
            }
            | Self::Unreached { file_candidates } => *file_candidates,
            Self::File { reaching, .. } => reaching.as_slice().len(),
        }
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
    let candidates: Vec<&Measured> = baseline
        .iter()
        .filter(|measured| measured.status == TargetStatus::Passed)
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
    Reaching::new(cheapest_first(&reaching)).map_or(
        Route::Unreached {
            file_candidates: candidates.len(),
        },
        |reaching| Route::Block {
            reaching,
            file_candidates: candidates.len(),
        },
    )
}

/// Every target that touched the file, and why the position did not narrow it. With no candidate at all there is nothing to run and nothing the fallback could have told us, so it is the same answer as unreached.
fn by_file(candidates: &[&Measured], fallback: Fallback) -> Route {
    Reaching::new(cheapest_first(candidates)).map_or(
        Route::Unreached {
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
