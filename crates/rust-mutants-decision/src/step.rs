// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How an execution spends its step allowance, one boundary at a time; this crate compiles and tests it, and every generated runtime holds this file as its module `step`.

/// What the runtime asks of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAction {
    /// The selected mutant's guard was taken.
    Activate,
    /// A function or loop boundary was raised.
    Checkpoint,
}

/// Where an execution's allowance stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPhase {
    /// No mutant has activated, and a boundary costs nothing.
    Dormant,
    /// Every boundary is counted, and none stops the execution.
    Counting(usize),
    /// The mutant is active and this many boundaries are spent.
    Active(usize),
    /// The allowance was passed at this boundary, and the execution is stopping.
    Stopping(usize),
}

/// What the runtime does after a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAdvance {
    /// It carries on.
    Continue,
    /// It waits for the stop already under way.
    Park,
    /// The allowance is spent: `allowed` boundaries were, and this one is `observed`.
    Reached {
        /// The allowance.
        allowed: usize,
        /// The boundary past it.
        observed: usize,
    },
}

/// Why a transition cannot be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMachineError {
    /// The allowance is none the machine can spend: zero, or one with no boundary past it.
    Limit,
    /// The phase is not one the allowance can reach, or a count has no room left.
    Count,
}

/// The phase `action` moves `phase` to under an allowance of `allowed`, and what the runtime does next.
///
/// # Errors
/// An allowance of zero or of `usize::MAX`, and a phase or a count the allowance cannot reach.
pub const fn step_transition(
    phase: StepPhase,
    action: StepAction,
    allowed: usize,
) -> Result<(StepPhase, StepAdvance), StepMachineError> {
    if allowed == 0 || allowed == usize::MAX {
        return Err(StepMachineError::Limit);
    }
    match (phase, action) {
        (StepPhase::Dormant, StepAction::Activate) => {
            Ok((StepPhase::Active(1), StepAdvance::Continue))
        }
        (StepPhase::Dormant, StepAction::Checkpoint) => {
            Ok((StepPhase::Dormant, StepAdvance::Continue))
        }
        (StepPhase::Counting(seen), StepAction::Checkpoint) => match seen.checked_add(1) {
            Some(next) => Ok((StepPhase::Counting(next), StepAdvance::Continue)),
            None => Err(StepMachineError::Count),
        },
        (StepPhase::Counting(seen), StepAction::Activate) => {
            Ok((StepPhase::Counting(seen), StepAdvance::Continue))
        }
        (StepPhase::Active(spent), StepAction::Activate) if spent > 0 && spent <= allowed => {
            Ok((StepPhase::Active(spent), StepAdvance::Continue))
        }
        (StepPhase::Active(spent), StepAction::Checkpoint) if spent > 0 && spent < allowed => {
            match spent.checked_add(1) {
                Some(next) => Ok((StepPhase::Active(next), StepAdvance::Continue)),
                None => Err(StepMachineError::Count),
            }
        }
        (StepPhase::Active(spent), StepAction::Checkpoint) if spent == allowed => {
            match allowed.checked_add(1) {
                Some(observed) => Ok((
                    StepPhase::Stopping(observed),
                    StepAdvance::Reached { allowed, observed },
                )),
                None => Err(StepMachineError::Count),
            }
        }
        (StepPhase::Stopping(spent), _) => match allowed.checked_add(1) {
            Some(observed) if spent == observed => {
                Ok((StepPhase::Stopping(spent), StepAdvance::Park))
            }
            Some(_) | None => Err(StepMachineError::Count),
        },
        (StepPhase::Active(_), _) => Err(StepMachineError::Count),
    }
}
