// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which of the two measurements a prepared session makes, as one value a test can name.
//!
//! There are two independent answers to "could this target have noticed this
//! mutation": the guards, which record on the run that verifies the baseline
//! which of a target's tests reached them, and the LLVM coverage build, which
//! is kept as a second opinion. A test about a proof layer is nearly always a
//! test about one of the four combinations, and naming the combination is
//! clearer than two `bool` arguments a reader has to count.

use crate::rule::Tier;
use crate::session::PrepareOptions;

/// Which measurements a session is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measuring {
    /// Build once with LLVM coverage instrumentation and route by the regions it exported.
    pub coverage: bool,
    /// Ask the guards, on the baseline run, which of each target's tests reached them.
    pub touch: bool,
}

impl Measuring {
    /// Neither, so every mutant is put to every test of every target: the answer a run with nothing removed gives.
    pub const NOTHING: Self = Self {
        coverage: false,
        touch: false,
    };
    /// The guards alone, which is the default a run makes and costs no build of its own.
    pub const GUARDS: Self = Self {
        coverage: false,
        touch: true,
    };
    /// The coverage build alone, which is the second opinion the guards are checked against.
    pub const COVERAGE: Self = Self {
        coverage: true,
        touch: false,
    };
    /// Both, so a mutation is put to the tests the guards named among the targets the regions placed.
    pub const BOTH: Self = Self {
        coverage: true,
        touch: true,
    };

    /// Every combination, so a test can hold a claim to all of them rather than to the one it thought of.
    pub const ALL: [Self; 4] = [Self::NOTHING, Self::GUARDS, Self::COVERAGE, Self::BOTH];

    /// The name a failing assertion carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match (self.coverage, self.touch) {
            (false, false) => "nothing",
            (false, true) => "guards",
            (true, false) => "coverage",
            (true, true) => "both",
        }
    }

    /// The command line a run measuring this way is asked for, so an engine test and a command-line test name one thing one way.
    #[must_use]
    pub const fn flags(self) -> &'static [&'static str] {
        match (self.coverage, self.touch) {
            (false, false) => &["--no-coverage", "--no-touch"],
            (false, true) => &["--no-coverage"],
            (true, false) => &["--coverage", "--no-touch"],
            (true, true) => &["--coverage"],
        }
    }

    /// Preparation options that measure this way, with every rule and the branch proofs whichever measurement can carry.
    #[must_use]
    pub fn options(self, tier: Tier) -> PrepareOptions {
        PrepareOptions {
            tier,
            coverage: self.coverage,
            branch_proofs: self.coverage || self.touch,
            touch: self.touch,
            ..PrepareOptions::default()
        }
    }
}
