// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The two things every step of a run is watched by: the cancellation flag
//! and the trace.
//!
//! They travel together because they answer the same question from opposite
//! sides — "should this still be happening?" and "what happened?" — and
//! because passing them as one argument keeps every signature that starts a
//! process short enough to read.

use rust_mutants::runner::Cancel;
use rust_mutants::trace::Recorder;

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
