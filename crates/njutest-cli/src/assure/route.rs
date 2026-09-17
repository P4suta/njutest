// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a route means to a report.

pub use rust_mutants::session::{
    Asked, BRANCH_NEVER_TAKEN, Discharge, Fallback, NEVER_INFECTED, Reaches, Route,
};

/// One sentence a reader can act on, for the fallback a route was widened by.
#[must_use]
pub fn detail(fallback: &str) -> &'static str {
    match fallback {
        "not-measured" => {
            "nothing was measured about which tests reach which code, so every test of \
             every target was run"
        }
        "position-unknown" => {
            "the catalog could not say where the mutation is, so no measurement is about \
             it and every test of every target was run"
        }
        "outside-blocks" => {
            "no instrumented region contains the position, which is a gap in the \
             measurement rather than proof that nothing runs it, so every test of every \
             target was run"
        }
        "coverage-incomplete" => {
            "a target that was measured carries no coverage, so its silence about the \
             position is not evidence and it was run"
        }
        "touch-incomplete" => {
            "a target's guards recorded nothing this run can route by, so its silence \
             about the position is not evidence and every test of it was run"
        }
        _ => "the measurement did not decide it, so more was run rather than less",
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
        _ => false,
    }
}
