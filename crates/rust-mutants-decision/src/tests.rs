// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec;
use std::vec::Vec;

use crate::step::{StepAction, StepAdvance, StepMachineError, StepPhase, step_transition};

const ALLOWANCES: [usize; 7] = [0, 1, 2, 3, 7, usize::MAX - 1, usize::MAX];

/// Every action the runtime asks of the machine, beside the match that says the set is whole.
fn every_action() -> [StepAction; 2] {
    match StepAction::Activate {
        StepAction::Activate | StepAction::Checkpoint => {}
    }
    [StepAction::Activate, StepAction::Checkpoint]
}

fn counts_near(allowed: usize) -> Vec<usize> {
    let mut counts = vec![0, 1, 2, 3, usize::MAX - 1, usize::MAX];
    if let Some(below) = allowed.checked_sub(1) {
        counts.push(below);
    }
    counts.push(allowed);
    for further in 1..=2 {
        if let Some(past) = allowed.checked_add(further) {
            counts.push(past);
        }
    }
    counts
}

fn every_phase(allowed: usize) -> Vec<StepPhase> {
    let mut phases = vec![StepPhase::Dormant];
    for count in counts_near(allowed) {
        phases.push(StepPhase::Counting(count));
        phases.push(StepPhase::Active(count));
        phases.push(StepPhase::Stopping(count));
    }
    phases
}

fn by_the_rules(
    phase: StepPhase,
    action: StepAction,
    allowed: usize,
) -> Result<(StepPhase, StepAdvance), StepMachineError> {
    if allowed == 0 || allowed == usize::MAX {
        return Err(StepMachineError::Limit);
    }
    let past = allowed.checked_add(1);
    match phase {
        StepPhase::Dormant => Ok((
            match action {
                StepAction::Activate => StepPhase::Active(1),
                StepAction::Checkpoint => StepPhase::Dormant,
            },
            StepAdvance::Continue,
        )),
        StepPhase::Counting(seen) => {
            if matches!(action, StepAction::Activate) {
                Ok((StepPhase::Counting(seen), StepAdvance::Continue))
            } else {
                seen.checked_add(1)
                    .map_or(Err(StepMachineError::Count), |next| {
                        Ok((StepPhase::Counting(next), StepAdvance::Continue))
                    })
            }
        }
        StepPhase::Active(spent) => {
            if spent == 0 || spent > allowed {
                Err(StepMachineError::Count)
            } else if matches!(action, StepAction::Activate) {
                Ok((StepPhase::Active(spent), StepAdvance::Continue))
            } else if spent == allowed {
                past.map_or(Err(StepMachineError::Count), |observed| {
                    Ok((
                        StepPhase::Stopping(observed),
                        StepAdvance::Reached { allowed, observed },
                    ))
                })
            } else {
                spent
                    .checked_add(1)
                    .map_or(Err(StepMachineError::Count), |next| {
                        Ok((StepPhase::Active(next), StepAdvance::Continue))
                    })
            }
        }
        StepPhase::Stopping(spent) => past.map_or(Err(StepMachineError::Count), |observed| {
            if spent == observed {
                Ok((StepPhase::Stopping(spent), StepAdvance::Park))
            } else {
                Err(StepMachineError::Count)
            }
        }),
    }
}

#[test]
fn every_transition_is_the_one_the_rules_give() {
    let mut asked = Vec::new();
    for allowed in ALLOWANCES {
        for phase in every_phase(allowed) {
            for action in every_action() {
                asked.push((phase, action, allowed));
                assert_eq!(
                    step_transition(phase, action, allowed),
                    by_the_rules(phase, action, allowed),
                    "{action:?} in {phase:?} under an allowance of {allowed}"
                );
            }
        }
    }
    assert!(
        asked.len() > 300,
        "the table asked {} transitions",
        asked.len()
    );
}

#[test]
fn an_execution_spends_exactly_its_allowance_and_then_parks() {
    for (allowed, past) in [(1usize, 2usize), (2, 3), (3, 4), (7, 8)] {
        assert_eq!(
            step_transition(StepPhase::Dormant, StepAction::Activate, allowed),
            Ok((StepPhase::Active(1), StepAdvance::Continue))
        );
        let mut phase = StepPhase::Active(1);
        for spent in 2..=allowed {
            assert_eq!(
                step_transition(phase, StepAction::Checkpoint, allowed),
                Ok((StepPhase::Active(spent), StepAdvance::Continue)),
                "the boundary {spent} under an allowance of {allowed}"
            );
            phase = StepPhase::Active(spent);
            assert_eq!(
                step_transition(phase, StepAction::Activate, allowed),
                Ok((phase, StepAdvance::Continue)),
                "a guard taken again spends nothing"
            );
        }
        assert_eq!(
            step_transition(phase, StepAction::Checkpoint, allowed),
            Ok((
                StepPhase::Stopping(past),
                StepAdvance::Reached {
                    allowed,
                    observed: past
                }
            )),
            "the boundary past an allowance of {allowed} is the one that stops it"
        );
        for action in every_action() {
            assert_eq!(
                step_transition(StepPhase::Stopping(past), action, allowed),
                Ok((StepPhase::Stopping(past), StepAdvance::Park)),
                "a stopping execution stays stopping"
            );
        }
    }
}

#[test]
fn an_allowance_with_no_boundary_past_it_or_none_at_all_is_refused() {
    for allowed in [0, usize::MAX] {
        for phase in every_phase(allowed) {
            for action in every_action() {
                assert_eq!(
                    step_transition(phase, action, allowed),
                    Err(StepMachineError::Limit),
                    "{action:?} in {phase:?} under an allowance of {allowed}"
                );
            }
        }
    }
}

#[test]
fn the_runtime_holds_this_machine_as_its_own_source() {
    let source = crate::STEP_SOURCE;
    assert!(
        source.contains("pub const fn step_transition(")
            && !source.contains("mod tests")
            && !source.contains("include_str!"),
        "the step module is written so a generated runtime can hold it whole"
    );
    assert!(
        source.starts_with("// SPDX-FileCopyrightText: 2026 njutest contributors\n")
            && !source.contains("__rm")
            && !source.contains("rust-mutants-runtime"),
        "the text a runtime holds is the module as written, never one a run rewrote, which would \
         hold a runtime of its own inside every runtime rendered from it"
    );
    let items = source
        .lines()
        .filter(|line| line.starts_with("pub enum ") || line.starts_with("pub const fn "))
        .count();
    assert_eq!(
        source.matches("pub ").count(),
        items,
        "every `pub ` in the step module is an item's own, which a runtime narrows to its crate"
    );
}
