// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What happened to one mutant in one run.

use std::fmt;

/// The outcome of one mutant.
///
/// Closed, and deliberately: the type publishes `ALL` as every outcome there is, and a `#[non_exhaustive]` beside that promise says the opposite —
/// downstream must write a `_` arm for a case the list says cannot exist.
/// A tally that reached one counted every future outcome as a harness failure,
/// which is a screen telling somebody their machine is broken about a thing the run established perfectly well.
/// An outcome added here is a break for anything that renders one, and that is the honest shape of the change (ADR 0023).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Never executed: another shard owned it, a selection excluded it, the run was interrupted, or routing proved no test reaches it.
    NotRun,
    /// At least one test failed with the mutant active.
    /// Detected.
    Killed,
    /// Every selected test passed with the mutant active.
    Survived,
    /// One process reached the configured guard-take limit.
    /// This is an execution bound, not a decision about the mutation.
    StepLimitReached,
    /// A bound expired while this machine watched, confirmed by a serial retry.
    /// Not detected: the run established that it stopped waiting, which is a fact about the machine and not about the mutation.
    Waited,
    /// The run could not decide: one timeout that did not reproduce, or a failure that also fails on the instrumented baseline.
    Inconclusive,
    /// The harness itself failed for this mutant: the test binary could not start, the runtime said it was built from another catalog, or a process said it could not record what it saw.
    /// A death by signal is not one of these: a test that aborts is a test that failed, which is a kill.
    Errored,
}

impl Outcome {
    /// The canonical wire name: `snake_case`, stable, used in JSON and caches.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotRun => "not_run",
            Self::Killed => "killed",
            Self::Survived => "survived",
            Self::StepLimitReached => "step_limit_reached",
            Self::Waited => "waited",
            Self::Inconclusive => "inconclusive",
            Self::Errored => "errored",
        }
    }

    /// The canonical wire name, for interfaces that take a string slice.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.name()
    }

    /// What caught the mutant, when anything did.
    ///
    /// The one place that says which outcomes are detections.
    /// Two layers each answering that question is two answers, and this repository had them:
    /// the engine counted a timeout as caught while the report said an expired bound establishes nothing, about the same mutation in the same run (ADR 0023).
    /// Anything that needs the answer derives it from here.
    #[must_use]
    pub const fn noticed(self) -> Option<Noticed> {
        match self {
            Self::Killed => Some(Noticed::Tests),
            Self::NotRun
            | Self::Survived
            | Self::StepLimitReached
            | Self::Waited
            | Self::Inconclusive
            | Self::Errored => None,
        }
    }

    /// Whether anything caught the mutant.
    #[must_use]
    pub const fn detected(self) -> bool {
        self.noticed().is_some()
    }

    /// The outcome with the given wire name, if any.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|outcome| outcome.name() == name)
    }
}

/// What caught a mutant, which is not always a test.
///
/// A closed set, because a reader adding these up is told which column each belongs in and a new way of catching one is a column somebody has to place rather than a number that quietly joins another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum Noticed {
    /// A test failed with the mutant active.
    Tests,
}

impl Noticed {
    /// The canonical wire name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tests => "tests",
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}
