// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a mutant's verdict rests on, as a report records it (ADR 0046).

use rust_mutants_decision::evidence::{
    Detection, Doubts, Execution, Found, Reason, Sealability, Sealed, Standing, standing,
};

use super::standing::{Answer, Put};
use crate::outcome::Outcome;
use crate::run::NotRunReason;

/// What a mutant's verdict rests on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Evidence {
    /// Sealed executions established it.
    Sealed {
        /// The sealed executions it rests on, in the order they ran.
        executions: Vec<SealedRun>,
    },
    /// Nothing established a verdict, so the outcome is what a native run said, which is a lead.
    Unproven {
        /// Every reason there is no verdict, in a fixed order.
        reasons: Vec<Doubt>,
    },
}

/// Which of the two a mutant's evidence is, which is all a finding is decided from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Sealed executions established the verdict.
    Sealed,
    /// There is no verdict.
    Unproven,
}

impl Evidence {
    /// Evidence where nothing was sealed: every reason is that the run sealed nothing it could put the mutant to.
    #[must_use]
    pub fn not_sealed() -> Self {
        Self::Unproven {
            reasons: vec![Doubt::NotSealed],
        }
    }

    /// Which of the two it is.
    #[must_use]
    pub const fn class(&self) -> Class {
        match self {
            Self::Sealed { .. } => Class::Sealed,
            Self::Unproven { .. } => Class::Unproven,
        }
    }

    /// What the sealed executions it records establish, decided again from them: nothing where it is unproven or where they establish no verdict.
    #[must_use]
    pub fn found(&self) -> Option<Found> {
        match self {
            Self::Sealed { executions } => {
                let executions: Vec<Execution> = executions
                    .iter()
                    .map(|run| Execution::Sealed(run.came_to.sealed()))
                    .collect();
                match standing(Sealability::Answerable, &executions) {
                    Standing::Established(verdict) => Some(verdict.found()),
                    Standing::Unproven(_) => None,
                }
            }
            Self::Unproven { .. } => None,
        }
    }

    /// What `answer` records.
    #[must_use]
    pub fn of(answer: &Answer) -> Self {
        let executions = answer.puts.iter().map(SealedRun::of).collect();
        match answer.standing {
            Standing::Established(_) => Self::Sealed { executions },
            Standing::Unproven(doubts) => Self::Unproven {
                reasons: Doubt::all_of(doubts),
            },
        }
    }
}

/// One sealed execution, as a report records it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedRun {
    /// The target whose module ran.
    pub target: String,
    /// The test it ran.
    pub test: String,
    /// What it came to.
    pub came_to: Came,
}

impl SealedRun {
    fn of(put: &Put) -> Self {
        Self {
            target: put.target.clone(),
            test: put.test.clone(),
            came_to: Came::of(put.came_to),
        }
    }
}

/// What one sealed execution came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Came {
    /// The test passed.
    Passed,
    /// The test panicked.
    Panicked,
    /// The harness reported the test failed.
    Failed,
    /// The instance trapped.
    Trapped,
    /// The execution spent more fuel than its control's bound.
    FuelExceeded,
    /// The execution asked for more memory than its control ever did.
    MemoryExceeded,
    /// The instance exited with status zero before its harness finished.
    ExitedEarly,
    /// The instance's stack overflowed.
    StackOverflow,
    /// The execution met a refusal of the sandbox its control did not.
    Refused,
    /// The harness's account disagreed with what the host saw.
    Unaccounted,
}

/// The row a verdict gives a mutant: its outcome, and why it was not run where it was not.
#[must_use]
pub const fn row_of(found: Found) -> (Outcome, Option<NotRunReason>) {
    match found {
        Found::Killed { .. } => (Outcome::Killed, None),
        Found::Survived => (Outcome::Survived, None),
        Found::Unreached => (Outcome::NotRun, Some(NotRunReason::Unreached)),
    }
}

impl Came {
    /// The name a report spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Panicked => "panicked",
            Self::Failed => "failed",
            Self::Trapped => "trapped",
            Self::FuelExceeded => "fuel-exceeded",
            Self::MemoryExceeded => "memory-exceeded",
            Self::ExitedEarly => "exited-early",
            Self::StackOverflow => "stack-overflow",
            Self::Refused => "refused",
            Self::Unaccounted => "unaccounted",
        }
    }

    const fn sealed(self) -> Sealed {
        match self {
            Self::Passed => Sealed::Passed,
            Self::Panicked => Sealed::Detected(Detection::Panicked),
            Self::Failed => Sealed::Detected(Detection::Failed),
            Self::Trapped => Sealed::Detected(Detection::Trapped),
            Self::FuelExceeded => Sealed::Detected(Detection::FuelExceeded),
            Self::MemoryExceeded => Sealed::Detected(Detection::MemoryExceeded),
            Self::ExitedEarly => {
                Sealed::Doubted(rust_mutants_decision::evidence::Doubt::ExitedEarly)
            }
            Self::StackOverflow => {
                Sealed::Doubted(rust_mutants_decision::evidence::Doubt::StackOverflow)
            }
            Self::Refused => Sealed::Doubted(rust_mutants_decision::evidence::Doubt::Refused),
            Self::Unaccounted => {
                Sealed::Doubted(rust_mutants_decision::evidence::Doubt::Unaccounted)
            }
        }
    }

    /// What a sealed execution that came to `sealed` is recorded as.
    #[must_use]
    pub const fn of(sealed: Sealed) -> Self {
        match sealed {
            Sealed::Passed => Self::Passed,
            Sealed::Detected(Detection::Panicked) => Self::Panicked,
            Sealed::Detected(Detection::Failed) => Self::Failed,
            Sealed::Detected(Detection::Trapped) => Self::Trapped,
            Sealed::Detected(Detection::FuelExceeded) => Self::FuelExceeded,
            Sealed::Detected(Detection::MemoryExceeded) => Self::MemoryExceeded,
            Sealed::Doubted(doubt) => match doubt {
                rust_mutants_decision::evidence::Doubt::ExitedEarly => Self::ExitedEarly,
                rust_mutants_decision::evidence::Doubt::StackOverflow => Self::StackOverflow,
                rust_mutants_decision::evidence::Doubt::Refused => Self::Refused,
                rust_mutants_decision::evidence::Doubt::Unaccounted => Self::Unaccounted,
            },
        }
    }
}

/// A reason a mutant has no verdict, as a report records it.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Doubt {
    /// The run sealed nothing it could put the mutant to: sealing was off, or the target was not installed.
    NotSealed,
    /// A test that reaches the mutant ran only natively.
    Native,
    /// The sealed build does not hold the mutant's guard.
    GuardAbsent,
    /// A test that reaches the mutant natively is not in the sealed build.
    TestAbsent,
    /// A test reaches the mutant natively and its sealed control does not reach its guard.
    ReachDiffers,
    /// A test ended its instance with status zero before its harness finished.
    ExitedEarly,
    /// An instance's stack overflowed.
    StackOverflow,
    /// The mutant met a refusal of the sandbox its control did not.
    Refused,
    /// A harness's account of its one test disagreed with what the host observed.
    Unaccounted,
}

impl Doubt {
    const fn of(reason: Reason) -> Self {
        match reason {
            Reason::Native => Self::Native,
            Reason::GuardAbsent => Self::GuardAbsent,
            Reason::TestAbsent => Self::TestAbsent,
            Reason::ReachDiffers => Self::ReachDiffers,
            Reason::ExitedEarly => Self::ExitedEarly,
            Reason::StackOverflow => Self::StackOverflow,
            Reason::Refused => Self::Refused,
            Reason::Unaccounted => Self::Unaccounted,
        }
    }

    fn all_of(doubts: Doubts) -> Vec<Self> {
        Reason::ALL
            .into_iter()
            .filter(|reason| doubts.contains(*reason))
            .map(Self::of)
            .collect()
    }

    /// The words a finding says it in.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::NotSealed => "nothing was sealed to put it to",
            Self::Native => "a test that reaches it ran only natively",
            Self::GuardAbsent => "the sealed build does not hold its guard",
            Self::TestAbsent => "a test that reaches it natively is not in the sealed build",
            Self::ReachDiffers => "a test that reaches it natively does not reach it sealed",
            Self::ExitedEarly => "a test exited before its harness finished",
            Self::StackOverflow => "a test's stack overflowed",
            Self::Refused => "a test met a refusal of the sandbox its control did not",
            Self::Unaccounted => "a harness's account disagreed with what the host saw",
        }
    }
}
