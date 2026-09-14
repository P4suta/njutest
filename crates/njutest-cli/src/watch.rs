// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The two things every step of a run is watched by: the cancellation flag and the trace.

use crate::trace::Recorder;
use rust_mutants::runner::Cancel;

/// Cancellation and the trace, as one argument.
#[derive(Debug, Clone, Copy)]
pub struct Watch<'a> {
    /// Raised when the run should stop.
    pub cancel: &'a Cancel,
    /// Where the run records what it did.
    pub trace: &'a Recorder,
}

impl<'a> Watch<'a> {
    /// A watch over `cancel` that records into `trace`.
    #[must_use]
    pub const fn new(cancel: &'a Cancel, trace: &'a Recorder) -> Self {
        Self { cancel, trace }
    }

    /// Whether the run should stop.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl rust_mutants::runner::Watch for Watch<'_> {
    fn cancel(&self) -> &Cancel {
        self.cancel
    }

    fn exec(&self, spec: &rust_mutants::runner::Spec, result: &rust_mutants::runner::RunResult) {
        self.trace.exec(crate::trace::ExecRecord::of(spec, result));
    }
}
