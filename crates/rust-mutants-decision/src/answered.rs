// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run asked to end at its first failing test comes to, which is the same whichever of its two endings arrives first.

/// How the wait on a process asked to end at its first failing test ended, as far as that stop reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Wait {
    /// The process exited on its own.
    Exited,
    /// The supervisor ended it because its harness said a test failed.
    Answered,
    /// It ended any other way: a bound, a stall, a monitor, a cancellation, or a failure to supervise it.
    Other,
}

/// What the stop at the first failing test makes of an ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Answer {
    /// The harness named a failing test, which is the whole answer, whether the process then exited or was ended.
    Answered,
    /// Nothing named a failure, so the process's own ending stands.
    Unanswered,
    /// The supervisor ended the process for a failure nothing named, which no reading can stand on.
    Contradicted,
}

/// What a run asked to end at its first failing test comes to, from how its wait ended and whether its harness named a failure.
#[must_use]
pub const fn answer(wait: Wait, named_a_failure: bool) -> Answer {
    match wait {
        Wait::Exited | Wait::Answered if named_a_failure => Answer::Answered,
        Wait::Answered => Answer::Contradicted,
        Wait::Exited | Wait::Other => Answer::Unanswered,
    }
}

/// What was observed of a run asked to end at its first failing test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed {
    /// How its wait ended.
    pub wait: Wait,
    /// Whether its harness named a failing test.
    pub named_a_failure: bool,
    /// Whether its output could not be read whole, which leaves nothing it said to stand on.
    pub capture_failed: bool,
}

/// Whether an ending reported as answered, or not, agrees with what was observed: the rule a run's ending is checked by where it runs, said apart from [`answer`].
///
/// It is answered exactly where the harness named a failure, the output was read whole, and the process exited or was ended for it; and nothing agrees with an end the supervisor made for a failure nothing named.
#[must_use]
pub const fn agrees(observed: Observed, reported_answered: bool) -> bool {
    let contradicted = matches!(observed.wait, Wait::Answered) && !observed.named_a_failure;
    let answered = observed.named_a_failure
        && !observed.capture_failed
        && !matches!(observed.wait, Wait::Other);
    !contradicted && answered == reported_answered
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani_laws;
