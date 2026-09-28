// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one sealed execution of one test came to, from what the host observed and what the harness said (ADR 0046).

use crate::evidence::{Detection, Doubt, Sealed};

/// How the instance of one test ended, as the host observed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Ending {
    /// The instance's `_start` returned, which it does only when the harness's `main` returned zero.
    Returned,
    /// The instance called `proc_exit(0)`, which the harness does not do on its way out.
    ExitedZero,
    /// The instance exited with its harness's failure status: libtest's, or the one rustdoc's `main` reports a doctest's error with.
    ExitedFailure,
    /// The instance exited with a status that is neither zero nor the harness's failure status.
    ExitedOther,
    /// The instance trapped on `unreachable` after the standard library printed a panic's message.
    Panicked,
    /// The instance trapped on `unreachable` with no panic's message before it.
    Aborted,
    /// The instance trapped for another reason the host decides the same way every time.
    Trapped,
    /// The instance overflowed its stack, at a depth the host's native frames decide.
    StackOverflow,
    /// The instance spent the fuel its matched control's run bounds it to.
    FuelExhausted,
    /// The instance was refused memory its matched control never asked for.
    MemoryExhausted,
}

/// What libtest said about the one test the instance ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Account {
    /// It accounted for exactly that test, as passed.
    Passed,
    /// It accounted for exactly that test, as failed.
    Failed,
    /// It accounted for anything else, or did not account for the run.
    Other,
}

/// What ran the one test in the instance, which decides what a pass is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    /// libtest ran exactly that test, by name, and gave this account of it.
    Libtest(Account),
    /// rustdoc's `main` ran one doctest, which passes by returning.
    Doctest,
    /// rustdoc's `main` ran one doctest marked `should_panic`, which passes by failing.
    ShouldPanic,
}

/// What the host observed of one sealed execution, against its matched control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed {
    /// How the instance ended.
    pub ending: Ending,
    /// What ran the test, and what it said.
    pub harness: Harness,
    /// Whether the execution met a refusal of the sandbox, or the standard library's message for an unsupported operation or a failed allocation, that its control did not.
    pub beyond_control: bool,
}

/// What one sealed execution came to.
///
/// A failure that followed a refusal its control did not meet is how the run measured, never a verdict (ADR 0023); otherwise the ending decides, read as the harness that ran the test decides a pass, and libtest's account is asked only where the ending alone cannot say.
#[must_use]
pub const fn judged(observed: Observed) -> Sealed {
    if observed.beyond_control {
        return Sealed::Doubted(Doubt::Refused);
    }
    match observed.harness {
        Harness::Libtest(account) => accounted(observed.ending, account),
        Harness::Doctest => returned(observed.ending),
        Harness::ShouldPanic => panicking(observed.ending),
    }
}

/// What a test libtest gave `account` of came to, having ended as `ending`.
const fn accounted(ending: Ending, account: Account) -> Sealed {
    match ending {
        Ending::Returned => match account {
            Account::Passed => Sealed::Passed,
            Account::Failed | Account::Other => Sealed::Doubted(Doubt::Unaccounted),
        },
        Ending::ExitedFailure => match account {
            Account::Failed => Sealed::Detected(Detection::Failed),
            Account::Passed | Account::Other => Sealed::Doubted(Doubt::Unaccounted),
        },
        Ending::Panicked => Sealed::Detected(Detection::Panicked),
        Ending::Aborted | Ending::Trapped => Sealed::Detected(Detection::Trapped),
        Ending::FuelExhausted => Sealed::Detected(Detection::FuelExceeded),
        Ending::MemoryExhausted => Sealed::Detected(Detection::MemoryExceeded),
        Ending::ExitedZero => Sealed::Doubted(Doubt::ExitedEarly),
        Ending::StackOverflow => Sealed::Doubted(Doubt::StackOverflow),
        Ending::ExitedOther => Sealed::Doubted(Doubt::Unaccounted),
    }
}

/// What a doctest that passes by returning came to, having ended as `ending`.
const fn returned(ending: Ending) -> Sealed {
    match ending {
        Ending::Returned => Sealed::Passed,
        Ending::ExitedFailure => Sealed::Detected(Detection::Failed),
        Ending::Panicked => Sealed::Detected(Detection::Panicked),
        Ending::Aborted | Ending::Trapped => Sealed::Detected(Detection::Trapped),
        Ending::FuelExhausted => Sealed::Detected(Detection::FuelExceeded),
        Ending::MemoryExhausted => Sealed::Detected(Detection::MemoryExceeded),
        Ending::ExitedZero => Sealed::Doubted(Doubt::ExitedEarly),
        Ending::StackOverflow => Sealed::Doubted(Doubt::StackOverflow),
        Ending::ExitedOther => Sealed::Doubted(Doubt::Unaccounted),
    }
}

/// What a doctest that passes by failing came to, having ended as `ending`: a failure that rests on memory its control never asked for is the sandbox's, never the test's.
const fn panicking(ending: Ending) -> Sealed {
    match ending {
        Ending::ExitedFailure | Ending::Panicked | Ending::Aborted | Ending::Trapped => {
            Sealed::Passed
        }
        Ending::Returned => Sealed::Detected(Detection::Failed),
        Ending::FuelExhausted => Sealed::Detected(Detection::FuelExceeded),
        Ending::MemoryExhausted => Sealed::Doubted(Doubt::Refused),
        Ending::ExitedZero => Sealed::Doubted(Doubt::ExitedEarly),
        Ending::StackOverflow => Sealed::Doubted(Doubt::StackOverflow),
        Ending::ExitedOther => Sealed::Doubted(Doubt::Unaccounted),
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani_laws;
