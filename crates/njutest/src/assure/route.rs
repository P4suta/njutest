// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a route means to a report.

#[cfg(feature = "testkit")]
pub use rust_mutants::session::{Asked, BRANCH_NEVER_TAKEN, Discharge, NEVER_INFECTED, Reaches};
pub use rust_mutants::session::{Fallback, Route};

/// One sentence a reader can act on, for the fallback a route was widened by.
///
/// It takes the fallback rather than its name, so the match is exhaustive and a fallback the engine grows is a compile error here rather than a catch-all sentence somebody reads.
#[must_use]
pub const fn detail(fallback: Fallback) -> &'static str {
    match fallback {
        Fallback::NotMeasured => {
            "nothing was measured about which tests reach which code, so every test of \
             every target was run"
        }
        Fallback::PositionUnknown => {
            "the catalog could not say where the mutation is, so no measurement is about \
             it and every test of every target was run"
        }
        Fallback::OutsideBlocks => {
            "no instrumented region contains the position, which is a gap in the \
             measurement rather than proof that nothing runs it, so every test of every \
             target was run"
        }
        Fallback::CoverageIncomplete => {
            "a target that was measured carries no coverage, so its silence about the \
             position is not evidence and it was run"
        }
        Fallback::TouchIncomplete => {
            "a target's guards recorded nothing this run can route by, so its silence \
             about the position is not evidence and every test of it was run"
        }
    }
}

/// Whether this route says the tests executed the position, which is the premise every claim about identical code rests on.
#[must_use]
pub const fn ran_the_position(route: &Route) -> bool {
    matches!(
        route,
        Route::Block {
            reaching,
            fallback: None,
            ..
        } if !reaching.is_empty()
    )
}

/// Whether this route ran nothing at all, which is why nothing noticed the mutation.
#[must_use]
pub const fn nothing_ran(route: &Route) -> bool {
    match route {
        Route::Block { reaching, .. } => reaching.is_empty(),
        Route::Discharged { .. } | Route::Unreached { .. } => true,
        Route::All { .. } => false,
    }
}
