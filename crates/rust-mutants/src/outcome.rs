// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What happened to one mutant in one run.

use std::fmt;

/// The outcome of one mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Outcome {
    /// Never executed: another shard owned it, a selection excluded it, the run was interrupted, or routing proved no test reaches it.
    #[default]
    NotRun,
    /// At least one test failed with the mutant active. Detected.
    Killed,
    /// Every selected test passed with the mutant active.
    Survived,
    /// A *confirmed* timeout: exceeded the budget, retried serially, exceeded it again. Detected — an infinite loop a mutant introduced is a behaviour change the tests noticed. A single timeout is inconclusive.
    TimedOut,
    /// The run could not decide: one timeout that did not reproduce, or a failure that also fails on the instrumented baseline.
    Inconclusive,
    /// The harness itself failed for this mutant: the test binary could not start, the runtime said it was built from another catalog, or a process said it could not record what it saw. A death by signal is not one of these: a test that aborts is a test that failed, which is a kill.
    Errored,
}

impl Outcome {
    /// Every outcome in declaration order.
    pub const ALL: [Self; 6] = [
        Self::NotRun,
        Self::Killed,
        Self::Survived,
        Self::TimedOut,
        Self::Inconclusive,
        Self::Errored,
    ];

    /// The canonical wire name: `snake_case`, stable, used in JSON and caches.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotRun => "not_run",
            Self::Killed => "killed",
            Self::Survived => "survived",
            Self::TimedOut => "timed_out",
            Self::Inconclusive => "inconclusive",
            Self::Errored => "errored",
        }
    }

    /// Whether the tests caught the mutant: a kill or a confirmed timeout.
    #[must_use]
    pub const fn detected(self) -> bool {
        matches!(self, Self::Killed | Self::TimedOut)
    }

    /// The outcome with the given wire name, if any.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|outcome| outcome.name() == name)
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
