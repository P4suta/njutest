// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::step::{StepAction, StepAdvance, StepPhase, step_transition};

fn valid_limit() -> usize {
    let allowed = kani::any::<usize>();
    kani::assume(allowed > 0 && allowed < usize::MAX);
    allowed
}

#[kani::proof]
fn activation_is_idempotent() {
    let allowed = valid_limit();
    let spent = kani::any::<usize>();
    kani::assume(spent > 0 && spent <= allowed);
    kani::assert(
        step_transition(StepPhase::Active(spent), StepAction::Activate, allowed)
            == Ok((StepPhase::Active(spent), StepAdvance::Continue)),
        "njutest-law-assertion:activation-idempotent",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_dormant_checkpoint_cannot_spend() {
    let allowed = valid_limit();
    kani::assert(
        step_transition(StepPhase::Dormant, StepAction::Checkpoint, allowed)
            == Ok((StepPhase::Dormant, StepAdvance::Continue)),
        "njutest-law-assertion:dormant-inert",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_counting_checkpoint_counts() {
    let allowed = valid_limit();
    let seen = kani::any::<usize>();
    kani::assume(seen < usize::MAX);
    kani::assert(
        step_transition(StepPhase::Counting(seen), StepAction::Checkpoint, allowed)
            == Ok((StepPhase::Counting(seen + 1), StepAdvance::Continue)),
        "njutest-law-assertion:counting-counts",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_counting_checkpoint_never_stops() {
    let allowed = valid_limit();
    let seen = kani::any::<usize>();
    kani::assume(seen < usize::MAX);
    kani::assert(
        !matches!(
            step_transition(StepPhase::Counting(seen), StepAction::Checkpoint, allowed),
            Ok((_, StepAdvance::Park)) | Ok((_, StepAdvance::Reached { .. }))
        ),
        "njutest-law-assertion:counting-never-stops",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn counting_is_not_reachable_from_dormant_or_active() {
    let allowed = valid_limit();
    let spent = kani::any::<usize>();
    let action = if kani::any::<bool>() {
        StepAction::Activate
    } else {
        StepAction::Checkpoint
    };
    for phase in [StepPhase::Dormant, StepPhase::Active(spent)] {
        kani::assert(
            !matches!(
                step_transition(phase, action, allowed),
                Ok((StepPhase::Counting(_), _))
            ),
            "njutest-law-assertion:counting-only-from-the-state-file",
        );
    }
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn an_active_checkpoint_advances_or_reaches_the_exact_boundary() {
    let allowed = valid_limit();
    let spent = kani::any::<usize>();
    kani::assume(spent > 0 && spent <= allowed);
    let result = step_transition(StepPhase::Active(spent), StepAction::Checkpoint, allowed);
    if spent < allowed {
        kani::assert(
            result == Ok((StepPhase::Active(spent + 1), StepAdvance::Continue)),
            "njutest-law-assertion:active-advance",
        );
        kani::cover!(true, "njutest-law-branch:advance");
    } else {
        kani::assert(
            result
                == Ok((
                    StepPhase::Stopping(allowed + 1),
                    StepAdvance::Reached {
                        allowed,
                        observed: allowed + 1,
                    },
                )),
            "njutest-law-assertion:active-boundary",
        );
        kani::cover!(true, "njutest-law-branch:boundary");
    }
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn stopping_is_absorbing() {
    let allowed = valid_limit();
    let observed = allowed + 1;
    let action = if kani::any::<bool>() {
        StepAction::Activate
    } else {
        StepAction::Checkpoint
    };
    kani::assert(
        step_transition(StepPhase::Stopping(observed), action, allowed)
            == Ok((StepPhase::Stopping(observed), StepAdvance::Park)),
        "njutest-law-assertion:stopping-absorbing",
    );
    kani::cover!(
        matches!(action, StepAction::Activate),
        "njutest-law-branch:activate"
    );
    kani::cover!(
        matches!(action, StepAction::Checkpoint),
        "njutest-law-branch:checkpoint"
    );
    kani::cover!(true, "njutest-law-reached");
}
