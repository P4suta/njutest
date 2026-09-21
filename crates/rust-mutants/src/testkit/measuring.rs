// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which of the two measurements a prepared session makes, as one value a test can name.

use crate::rule::Tier;
use crate::session::PrepareOptions;

/// Which measurements a session is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Measuring {
    /// Neither measurement.
    Nothing,
    /// Guard touches only.
    Guards,
    /// LLVM coverage only.
    Coverage,
    /// Both independent measurements.
    Both,
}

impl Measuring {
    /// Neither, so every mutant is put to every test of every target: the answer a run with nothing removed gives.
    pub const NOTHING: Self = Self::Nothing;
    /// The guards alone, which is the default a run makes and costs no build of its own.
    pub const GUARDS: Self = Self::Guards;
    /// The coverage build alone, which is the second opinion the guards are checked against.
    pub const COVERAGE: Self = Self::Coverage;
    /// Both, so a mutation is put to the tests the guards named among the targets the regions placed.
    pub const BOTH: Self = Self::Both;

    const fn coverage(self) -> bool {
        matches!(self, Self::Coverage | Self::Both)
    }

    const fn touch(self) -> bool {
        matches!(self, Self::Guards | Self::Both)
    }

    /// The name a failing assertion carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::Guards => "guards",
            Self::Coverage => "coverage",
            Self::Both => "both",
        }
    }

    /// The command line a run measuring this way is asked for, so an engine test and a command-line test name one thing one way.
    #[must_use]
    pub const fn flags(self) -> &'static [&'static str] {
        match self {
            Self::Nothing => &["--no-coverage", "--no-touch"],
            Self::Guards => &["--no-coverage"],
            Self::Coverage => &["--coverage", "--no-touch"],
            Self::Both => &["--coverage"],
        }
    }

    /// Preparation options that measure this way, with every rule and the branch proofs whichever measurement can carry.
    #[must_use]
    pub fn options(self, tier: Tier) -> PrepareOptions {
        PrepareOptions {
            tier,
            coverage: self.coverage(),
            branch_proofs: self.coverage() || self.touch(),
            touch: self.touch(),
            ..PrepareOptions::default()
        }
    }
}
