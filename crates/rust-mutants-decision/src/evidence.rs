// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The standing of a mutant: a verdict only from what sealed executions observed (ADR 0046).

/// How a sealed execution detected the mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Detection {
    /// The test panicked, which traps an instance that aborts on a panic.
    Panicked,
    /// The test failed: its harness reported it failed and exited with its failure status, or a doctest that should panic returned.
    Failed,
    /// The instance trapped deterministically, for a reason other than a panic.
    Trapped,
    /// The execution spent more fuel than a bound its matched control stayed within.
    FuelExceeded,
    /// The execution asked for more memory than a bound its matched control stayed within.
    MemoryExceeded,
    /// The test declined to measure where its matched control measured, which is the mutant changing what the test did (ADR 0043).
    Declined,
}

/// Why a sealed execution established neither a pass nor a detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Doubt {
    /// The test ended its instance with status zero before its harness finished.
    ExitedEarly,
    /// The instance's stack overflowed, at a depth the host's native frames decide.
    StackOverflow,
    /// The mutant met a refusal of the sandbox that its matched control did not.
    Refused,
    /// The harness's account of its one test disagreed with what the host observed.
    Unaccounted,
    /// The test's control declined to measure, so no bound its control stayed within holds the execution, which ran past one (ADR 0043).
    Unmatched,
}

/// What a sealed execution of one test, with the mutant active, came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sealed {
    /// The test passed as its harness decides a pass: libtest accounted for exactly that test as passed, a doctest returned, or a doctest that should panic failed.
    Passed,
    /// The test detected the mutant.
    Detected(Detection),
    /// The execution established neither.
    Doubted(Doubt),
    /// The test declined to measure in the words its control declined in, so it measured nothing either way and is set aside (ADR 0043).
    SetAside,
}

/// What the one execution of one test that reaches the mutant established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    /// A sealed execution, whose outcome the host observed.
    Sealed(Sealed),
    /// A native process, whose account of itself is a lead and never a verdict.
    Native,
}

/// Whether the sealed build can answer for the mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Sealability {
    /// The sealed build holds the mutant's guard and every test that reaches it natively.
    Answerable,
    /// The sealed build does not hold the mutant's guard.
    GuardAbsent,
    /// A test that reaches the mutant natively is not in the sealed build.
    TestAbsent,
    /// A test reaches the mutant natively and its sealed control does not reach the mutant's guard.
    ReachDiffers,
}

/// A reason a mutant has no verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Reason {
    /// A test that reaches the mutant ran only natively.
    Native,
    /// The sealed build does not hold the mutant's guard.
    GuardAbsent,
    /// A test that reaches the mutant natively is not in the sealed build.
    TestAbsent,
    /// A test ended its instance with status zero before its harness finished.
    ExitedEarly,
    /// An instance's stack overflowed.
    StackOverflow,
    /// The mutant met a refusal of the sandbox that its matched control did not.
    Refused,
    /// A harness's account of its one test disagreed with what the host observed.
    Unaccounted,
    /// A test reaches the mutant natively and its sealed control does not reach the mutant's guard.
    ReachDiffers,
    /// Every test that reaches the mutant declined to measure, as its control did (ADR 0043).
    Declined,
    /// A test whose control declined ran past a bound no control of its own set.
    Unmatched,
}

impl Reason {
    /// The reason a doubt gives.
    #[must_use]
    pub const fn of(doubt: Doubt) -> Self {
        match doubt {
            Doubt::ExitedEarly => Self::ExitedEarly,
            Doubt::StackOverflow => Self::StackOverflow,
            Doubt::Refused => Self::Refused,
            Doubt::Unaccounted => Self::Unaccounted,
            Doubt::Unmatched => Self::Unmatched,
        }
    }

    const fn bit(self) -> u16 {
        match self {
            Self::Native => 0b00_0000_0001,
            Self::GuardAbsent => 0b00_0000_0010,
            Self::TestAbsent => 0b00_0000_0100,
            Self::ExitedEarly => 0b00_0000_1000,
            Self::StackOverflow => 0b00_0001_0000,
            Self::Refused => 0b00_0010_0000,
            Self::Unaccounted => 0b00_0100_0000,
            Self::ReachDiffers => 0b00_1000_0000,
            Self::Declined => 0b01_0000_0000,
            Self::Unmatched => 0b10_0000_0000,
        }
    }
}

/// Every reason a mutant has no verdict, as a set that is never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Doubts(u16);

impl Doubts {
    const fn of(reason: Reason) -> Self {
        Self(reason.bit())
    }

    const fn with(self, reason: Reason) -> Self {
        Self(self.0 | reason.bit())
    }

    /// Whether `reason` is one of them.
    #[must_use]
    pub const fn contains(self, reason: Reason) -> bool {
        self.0 & reason.bit() != 0
    }
}

/// What a verdict says about the mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// A test detected it: the one at `by` in the order the executions were given, and how.
    Killed {
        /// Where the detecting execution stands among those given.
        by: usize,
        /// How it detected the mutant.
        how: Detection,
    },
    /// Every test that reaches it passed, sealed, with it active, but for those that declined to measure as their controls did, and at least one measured.
    Survived,
    /// No test reaches it, and the sealed build holds its guard.
    Unreached,
}

/// A verdict, which only [`standing`] makes, and only from sealed executions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict(Found);

impl Verdict {
    /// What it says.
    #[must_use]
    pub const fn found(self) -> Found {
        self.0
    }
}

/// The standing of a mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// A verdict the sealed executions established.
    Established(Verdict),
    /// No verdict, and every reason there is none.
    Unproven(Doubts),
}

/// What a pass over the executions has established so far: the first detection, every doubt, and whether any execution measured rather than being set aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pass {
    killed: Option<(usize, Detection)>,
    doubts: Option<Doubts>,
    measured: bool,
}

impl Pass {
    const fn opening(sealability: Sealability) -> Self {
        let doubts = match sealability {
            Sealability::Answerable => None,
            Sealability::GuardAbsent => Some(Doubts::of(Reason::GuardAbsent)),
            Sealability::TestAbsent => Some(Doubts::of(Reason::TestAbsent)),
            Sealability::ReachDiffers => Some(Doubts::of(Reason::ReachDiffers)),
        };
        Self {
            killed: None,
            doubts,
            measured: false,
        }
    }

    const fn doubting(self, reason: Reason) -> Self {
        let doubts = match self.doubts {
            None => Doubts::of(reason),
            Some(doubts) => doubts.with(reason),
        };
        Self {
            killed: self.killed,
            doubts: Some(doubts),
            measured: self.measured,
        }
    }

    const fn measuring(self) -> Self {
        Self {
            measured: true,
            ..self
        }
    }

    const fn after(self, by: usize, execution: Execution) -> Self {
        match execution {
            Execution::Sealed(Sealed::SetAside) => self,
            Execution::Sealed(Sealed::Passed) => self.measuring(),
            Execution::Sealed(Sealed::Detected(how)) => match self.killed {
                Some(_) => self.measuring(),
                None => Self {
                    killed: Some((by, how)),
                    doubts: self.doubts,
                    measured: true,
                },
            },
            Execution::Sealed(Sealed::Doubted(doubt)) => {
                self.doubting(Reason::of(doubt)).measuring()
            }
            Execution::Native => self.doubting(Reason::Native).measuring(),
        }
    }
}

/// The standing of a mutant from whether the sealed build can answer for it and the one execution of each test that reaches it.
#[must_use]
pub fn standing(sealability: Sealability, executions: &[Execution]) -> Standing {
    if sealability == Sealability::GuardAbsent {
        return Standing::Unproven(Doubts::of(Reason::GuardAbsent));
    }
    let pass = executions
        .iter()
        .enumerate()
        .fold(Pass::opening(sealability), |pass, (by, execution)| {
            pass.after(by, *execution)
        });
    match pass {
        Pass {
            killed: Some((by, how)),
            ..
        } => Standing::Established(Verdict(Found::Killed { by, how })),
        Pass {
            killed: None,
            doubts: Some(doubts),
            ..
        } => Standing::Unproven(doubts),
        Pass {
            killed: None,
            doubts: None,
            ..
        } if executions.is_empty() => Standing::Established(Verdict(Found::Unreached)),
        Pass {
            killed: None,
            doubts: None,
            measured: false,
        } => Standing::Unproven(Doubts::of(Reason::Declined)),
        Pass {
            killed: None,
            doubts: None,
            measured: true,
        } => Standing::Established(Verdict(Found::Survived)),
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani_laws;
