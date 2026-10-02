// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The evidence a row a test builds by hand holds beside its outcome.

use crate::outcome::Outcome;
use crate::run::NotRunReason;
use crate::sealed::record::{Came, Evidence, SealedRun};

/// The sealed evidence that establishes `outcome`, not run for `reason`, from one test of `target`, or nothing sealed where no sealed execution establishes such a row.
#[must_use]
pub fn sealed_as(outcome: Outcome, reason: Option<NotRunReason>, target: &str) -> Evidence {
    let came_to: &[Came] = match (outcome, reason) {
        (Outcome::Killed, _) => &[Came::Panicked],
        (Outcome::Survived, _) => &[Came::Passed],
        (Outcome::NotRun, Some(NotRunReason::Unreached)) => &[],
        (
            Outcome::NotRun
            | Outcome::StepLimitReached
            | Outcome::Waited
            | Outcome::Inconclusive
            | Outcome::Errored,
            _,
        ) => return Evidence::not_sealed(),
    };
    Evidence::Sealed {
        executions: came_to
            .iter()
            .map(|came_to| SealedRun {
                target: target.to_owned(),
                test: "tests::one".to_owned(),
                came_to: *came_to,
            })
            .collect(),
    }
}
