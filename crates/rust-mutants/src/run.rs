// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Driving a session: what a run shares between the mutants it is measuring at once.

use std::sync::{PoisonError, RwLock};

/// The machine: shared while a run measures several mutations at once, and given to one of them when a budget expires.
///
/// A mutation's budget is a multiple of a duration the baseline measured, and
/// a duration measured while three other test processes were running is a
/// fact about the load rather than about the mutation. A run that has to
/// decide whether a budget really expired takes the machine to itself first,
/// so the measurement the decision rests on is the one the budget was
/// calibrated for. It is not a retry policy: one expired budget buys one
/// quiet measurement, and what that measurement observes is what stands.
#[derive(Debug, Default)]
pub struct Quiet(RwLock<()>);

impl Quiet {
    /// Runs `work` beside whatever else this run is measuring.
    pub fn shared<R>(&self, work: impl FnOnce() -> R) -> R {
        let held = self.0.read().unwrap_or_else(PoisonError::into_inner);
        let answer = work();
        drop(held);
        answer
    }

    /// Runs `work` with nothing else this run started running beside it.
    pub fn alone<R>(&self, work: impl FnOnce() -> R) -> R {
        let held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        let answer = work();
        drop(held);
        answer
    }
}
