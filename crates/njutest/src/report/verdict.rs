// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a mutation row is judged by, and the only place an outcome's own decision, answer and owed finding are read, so nothing else in the crate can decide a row from its outcome alone (ADR 0046).

use super::{Answered, Decision, FindingKind, Outcome, matrix};

impl Outcome {
    /// Who decided a mutation this outcome is recorded for, where what the row says rests on what it says; [`RowVerdict::decision`] is what a reader asks.
    ///
    /// A step boundary and a clock boundary are both explicit non-answers.
    /// Neither may become detection credit without a separately represented comparison proving that the mutant diverged from its control.
    #[must_use]
    const fn decision(self) -> Decision {
        match self {
            Self::CompileRejected => Decision::Types,
            Self::Killed => Decision::Tests,
            Self::ModelNoticed => Decision::ModelNoticed,
            Self::ModelProved => Decision::ModelProved,
            Self::StepLimitReached => Decision::StepLimitReached,
            Self::Waited => Decision::Waited,
            Self::Survived => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
            Self::Equivalent => Decision::Proved,
            Self::Unconfirmed | Self::Errored | Self::Declined => Decision::Errored,
        }
    }

    /// Whether a review acceptance can answer this outcome, where what the row says rests on what it says; [`RowVerdict::acceptable`] is what a reader asks.
    ///
    /// In particular, an expired clock or a crossed step allowance is an observation that still needs an answer; it cannot be converted into one by accepting it.
    #[must_use]
    const fn review_answerable(self) -> bool {
        matches!(self, Self::Survived | Self::Unreached | Self::Equivalent)
    }

    /// Whether a row of this outcome is a verdict only where sealed executions established it, which a compiler's refusal, a model checker's own proof and a hole are not (ADR 0046).
    #[must_use]
    const fn needs_sealing(self) -> bool {
        match self {
            Self::Killed | Self::Survived | Self::Unreached | Self::Equivalent => true,
            Self::CompileRejected
            | Self::ModelNoticed
            | Self::ModelProved
            | Self::StepLimitReached
            | Self::Waited
            | Self::Unconfirmed
            | Self::Errored
            | Self::Declined => false,
        }
    }

    /// Whether a row of this outcome supports a completed answer, where what it says rests on what it says; [`RowVerdict::answered`] is what a reader asks.
    ///
    /// `accepted` belongs to the row rather than to an aggregate count.
    /// That prevents an acceptance on one mutation from hiding an unanswered survivor or unreached mutation elsewhere in the report.
    #[must_use]
    const fn answered(self, accepted: bool) -> bool {
        match self {
            Self::CompileRejected
            | Self::Killed
            | Self::ModelNoticed
            | Self::ModelProved
            | Self::Equivalent => true,
            Self::Survived | Self::Unreached => accepted,
            Self::StepLimitReached
            | Self::Waited
            | Self::Unconfirmed
            | Self::Errored
            | Self::Declined => false,
        }
    }

    /// The actionable finding a row of this outcome must carry, after any row-local acceptance, where what it says rests on what it says; [`RowVerdict::required_finding`] is what a reader asks.
    #[must_use]
    const fn required_finding(self, accepted: bool) -> Option<FindingKind> {
        match self {
            Self::Survived | Self::Unreached if !accepted => Some(FindingKind::SurvivingMutant),
            Self::StepLimitReached => Some(FindingKind::StepLimitReachedMutant),
            Self::Waited => Some(FindingKind::WaitedMutant),
            Self::Unconfirmed => Some(FindingKind::FailingTest),
            Self::Errored => Some(FindingKind::TargetMissing),
            Self::Declined => Some(FindingKind::NotMeasured),
            Self::CompileRejected
            | Self::Killed
            | Self::ModelNoticed
            | Self::ModelProved
            | Self::Equivalent
            | Self::Survived
            | Self::Unreached => None,
        }
    }
}

/// What one mutation row's answer rests on (ADR 0046).
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Resting {
    /// Sealed executions established what it says.
    Sealed,
    /// No sealed execution established anything, so what it says is what a native run said: a lead.
    Unproven,
    /// No execution was asked about it, because the compiler refused the mutation.
    Unexecuted,
}

impl Resting {
    /// What a row whose evidence is `evidence` rests on: nothing where it carries none.
    #[must_use]
    pub const fn of(evidence: Option<&rust_mutants::sealed::record::Evidence>) -> Self {
        match evidence {
            None => Self::Unexecuted,
            Some(evidence) => match evidence.class() {
                rust_mutants::sealed::record::Class::Sealed => Self::Sealed,
                rust_mutants::sealed::record::Class::Unproven => Self::Unproven,
            },
        }
    }
}

/// What a mutation row is judged by: what the run observed, whether a reviewer accepted it, and what that rests on, which is the one place its decision, its answer and the finding it owes are read (ADR 0046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowVerdict {
    /// What the run observed of it.
    pub outcome: Outcome,
    /// Whether a reviewer accepted it as it stands.
    pub accepted: bool,
    /// What that rests on.
    pub resting: Resting,
}

impl RowVerdict {
    /// Whether what the row says is a native lead: an outcome only a sealed execution can establish, which none did.
    #[must_use]
    pub const fn lead(self) -> bool {
        matches!(self.resting, Resting::Unproven) && self.outcome.needs_sealing()
    }

    /// Who decided it, which is nobody for a lead.
    #[must_use]
    pub const fn decision(self) -> Decision {
        if self.lead() {
            Decision::Unproven
        } else {
            self.outcome.decision()
        }
    }

    /// Whether the row supports a completed answer, which a lead never does.
    #[must_use]
    pub const fn answered(self) -> bool {
        !self.lead() && self.outcome.answered(self.accepted)
    }

    /// Whether a reviewer's acceptance can answer it, which it cannot for a lead: an acceptance answers what the tests were asked, and a lead says nothing they were.
    #[must_use]
    pub const fn acceptable(self) -> bool {
        !self.lead() && self.outcome.review_answerable()
    }

    /// The actionable finding the row must carry: `unproven-mutant` for a lead, and its outcome's otherwise.
    #[must_use]
    pub const fn required_finding(self) -> Option<FindingKind> {
        if self.lead() {
            Some(FindingKind::UnprovenMutant)
        } else {
            self.outcome.required_finding(self.accepted)
        }
    }

    /// Whether the run put the mutation and could not decide it, which the matrix counts as a hole of the mutation dimension.
    #[must_use]
    pub const fn unsettled(self) -> bool {
        self.lead() || matrix::unsettled(self.outcome)
    }
}

/// What each target the sealed executions of `evidence` ran answered, in the order each first ran: a kill where one of its executions detected the mutant, an error where one established nothing, and a survival where every one passed.
#[must_use]
pub fn sealed_answers(evidence: &rust_mutants::sealed::record::Evidence) -> Vec<Answered> {
    let rust_mutants::sealed::record::Evidence::Sealed { executions } = evidence else {
        return Vec::new();
    };
    let mut answered: Vec<Answered> = Vec::new();
    for run in executions {
        let came_to = sealed_outcome(run.came_to);
        match answered.iter_mut().find(|one| one.target == run.target) {
            Some(one) => one.outcome = stronger_answer(one.outcome, came_to),
            None => answered.push(Answered {
                target: run.target.clone(),
                outcome: came_to,
            }),
        }
    }
    answered
}

/// What the sealed executions of `evidence` establish, decided again from them: a kill, a survival, or nothing reaching the mutation, and nothing where they establish no verdict.
#[must_use]
pub fn sealed_standing(evidence: &rust_mutants::sealed::record::Evidence) -> Option<Outcome> {
    let (outcome, reason) = rust_mutants::sealed::record::row_of(evidence.found()?);
    match outcome {
        rust_mutants::outcome::Outcome::Killed => Some(Outcome::Killed),
        rust_mutants::outcome::Outcome::Survived => Some(Outcome::Survived),
        rust_mutants::outcome::Outcome::NotRun => match reason {
            Some(rust_mutants::run::NotRunReason::Unreached) => Some(Outcome::Unreached),
            Some(
                rust_mutants::run::NotRunReason::Discharged
                | rust_mutants::run::NotRunReason::Interrupted
                | rust_mutants::run::NotRunReason::Unselected
                | rust_mutants::run::NotRunReason::StoppedEarly
                | rust_mutants::run::NotRunReason::Declined,
            )
            | None => None,
        },
        rust_mutants::outcome::Outcome::StepLimitReached
        | rust_mutants::outcome::Outcome::Waited
        | rust_mutants::outcome::Outcome::Inconclusive
        | rust_mutants::outcome::Outcome::Errored => None,
    }
}

/// The target whose sealed execution killed the mutation first, where the executions of `evidence` establish a kill.
#[must_use]
pub fn sealed_killer(evidence: &rust_mutants::sealed::record::Evidence) -> Option<&str> {
    let rust_mutants::sealed::record::Evidence::Sealed { executions } = evidence else {
        return None;
    };
    if sealed_standing(evidence) != Some(Outcome::Killed) {
        return None;
    }
    executions
        .iter()
        .find(|run| sealed_outcome(run.came_to) == Outcome::Killed)
        .map(|run| run.target.as_str())
}

/// What one sealed execution that came to `came_to` answered about the mutation.
const fn sealed_outcome(came_to: rust_mutants::sealed::record::Came) -> Outcome {
    match came_to {
        rust_mutants::sealed::record::Came::Passed => Outcome::Survived,
        rust_mutants::sealed::record::Came::Panicked
        | rust_mutants::sealed::record::Came::Failed
        | rust_mutants::sealed::record::Came::Trapped
        | rust_mutants::sealed::record::Came::FuelExceeded
        | rust_mutants::sealed::record::Came::MemoryExceeded => Outcome::Killed,
        rust_mutants::sealed::record::Came::ExitedEarly
        | rust_mutants::sealed::record::Came::StackOverflow
        | rust_mutants::sealed::record::Came::Refused
        | rust_mutants::sealed::record::Came::Unaccounted => Outcome::Errored,
    }
}

/// What one target answered, of two of its sealed executions: a detection outweighs a doubt, which outweighs a pass.
const fn stronger_answer(held: Outcome, came_to: Outcome) -> Outcome {
    if said_of(came_to) > said_of(held) {
        came_to
    } else {
        held
    }
}

/// How much one sealed answer says about the mutation: a detection, then a doubt, then a pass.
const fn said_of(answer: Outcome) -> u8 {
    match answer {
        Outcome::Killed => 2,
        Outcome::Errored => 1,
        Outcome::CompileRejected
        | Outcome::ModelNoticed
        | Outcome::ModelProved
        | Outcome::StepLimitReached
        | Outcome::Waited
        | Outcome::Survived
        | Outcome::Unreached
        | Outcome::Equivalent
        | Outcome::Unconfirmed
        | Outcome::Declined => 0,
    }
}
