// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Putting one finding back to the tests, with nothing read back and nothing kept.

use std::path::Path;
use std::time::Duration;

use rust_mutants::session::{PrepareOptions, Request as ExecRequest, Timeout};
use rust_mutants::workspace::{OpenOptions, Workspace};

use crate::build::Cargo;
use crate::cli::{EXIT_ASSURED, EXIT_DEFECT, EXIT_INSUFFICIENT, Environment};
use crate::error::RunnerError;
use crate::report::FindingKind;
use crate::watch::Watch;

/// What a replay established about the finding it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Outcome {
    /// The finding is still there.
    Reproduced,
    /// The finding is not there any more.
    Resolved,
    /// The replay established neither, so the finding stands as the run left it.
    Inconclusive,
}

impl Outcome {
    /// The word a reader sees.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Reproduced => "REPRODUCED",
            Self::Resolved => "RESOLVED",
            Self::Inconclusive => "INCONCLUSIVE",
        }
    }

    /// The exit code this outcome earns, which is the one a finding earns.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Reproduced => EXIT_DEFECT,
            Self::Resolved => EXIT_ASSURED,
            Self::Inconclusive => EXIT_INSUFFICIENT,
        }
    }
}

/// What one replay needs.
#[derive(Debug, Clone)]
pub struct Replaying<'a> {
    /// The tree the finding is about, which is only ever read.
    pub root: &'a Path,
    /// The environment the snapshot's commands run with.
    pub environment: &'a Environment,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// What the tree is compiled as, which must be what the run that found the finding compiled: a replay of another build is a replay of another program.
    pub build: rust_mutants::cargo::BuildConfig,
    /// The arguments the test binaries are started with, which must be the ones the run used: a finding put back to a suite running another way is put to another suite.
    pub harness_args: Vec<String>,
    /// Test targets the run deliberately left out, which a replay must leave out too.
    pub skip_targets: Vec<String>,
    /// How long one execution may take.
    pub timeout: Option<Duration>,
    /// How many guard takes one execution may spend before it is stopped by a count rather than by the bound above.
    pub steps: u64,
    /// Where this project keeps what its runs leave behind, which the tree under test is copied without.
    pub reports: crate::config::ReportDirectory,
}

/// Offers `mutant` to the tests again and says whether `kind` is still observable.
///
/// # Errors
/// Only what stopped the replay from happening: a snapshot that could not be taken, a toolchain that could not be reached, a tree that does not build.
/// A mutation the tree no longer holds is one of those, because a finding about a mutation that is not there is not a finding this can answer.
pub fn replay(
    replaying: &Replaying<'_>,
    mutant: &str,
    kind: FindingKind,
    watch: Watch<'_>,
) -> Result<Outcome, RunnerError> {
    let operators = match replayed(kind) {
        Replayable::Site(operators) => operators,
        Replayable::Phase => return Ok(Outcome::Inconclusive),
    };
    let workspace = Workspace::open(
        replaying.root,
        OpenOptions {
            allow_outside: Vec::new(),
            cargo: None,
            search_path: replaying
                .environment
                .var("PATH")
                .map(std::ffi::OsStr::to_owned),
            env: replaying.environment.vars.clone(),
            temp_directory: replaying.environment.temp_directory.clone(),
            report_directory: Some(replaying.reports.as_str().to_owned()),
            exclude: Vec::new(),
            keep_temp: false,
            offline: replaying.cargo.offline,
            locked: replaying.cargo.locked,
            trace: rust_mutants::trace::Recorder::disabled(),
        },
        watch.cancel,
    )?;
    let session = workspace.prepare(
        &PrepareOptions {
            verify: false,
            build: replaying.build.clone(),
            harness_args: replaying.harness_args.clone(),
            skip_targets: replaying.skip_targets.clone(),
            mutant_timeout: replaying.timeout.map_or(Timeout::Auto, Timeout::Fixed),
            mutant_steps: (replaying.steps > 0).then_some(replaying.steps),
            operators,
            ..crate::assure::engine::switches()
        },
        watch.cancel,
    )?;
    let outcome = session
        .resolve(mutant)
        .map_err(RunnerError::from)
        .and_then(|found| {
            session
                .exec(
                    &ExecRequest::new(found.id.to_string()).with_timeout(replaying.timeout),
                    watch.cancel,
                )
                .map_err(RunnerError::from)
        })
        .map(|result| observed(kind, result.outcome()));
    session.close()?;
    outcome
}

/// Where a finding's subject can be put back to the tests from.
enum Replayable {
    /// One site of a catalog discovered by these rules, or by the tier's where there are none.
    Site(Vec<String>),
    /// A phase as a whole, which no single execution reproduces.
    Phase,
}

/// Where a finding of `kind` is put back to the tests from.
fn replayed(kind: FindingKind) -> Replayable {
    match kind {
        FindingKind::UnnoticedFault => {
            Replayable::Site(vec![crate::assure::faults::RULE.to_owned()])
        }
        FindingKind::BrokenUnderFault
        | FindingKind::DimensionNotMeasured
        | FindingKind::CorruptAfterCrash => Replayable::Phase,
        FindingKind::BuildFailure
        | FindingKind::FailingTest
        | FindingKind::TargetMissing
        | FindingKind::SurvivingMutant
        | FindingKind::Timeout
        | FindingKind::WaitedMutant
        | FindingKind::StepLimitReachedMutant
        | FindingKind::NotMeasured
        | FindingKind::UnmatchedAcceptance
        | FindingKind::UndefinedBehaviour
        | FindingKind::HollowTarget
        | FindingKind::WireUnnoticed
        | FindingKind::UnstableBaseline
        | FindingKind::EnvironmentDependent
        | FindingKind::EnvironmentDependentReach => Replayable::Site(Vec::new()),
    }
}

/// Whether the finding is still what the tests say.
///
/// A clock expiry reproduces a clock-expiry finding, and a verified step boundary reproduces a step-boundary finding.
/// Neither is upgraded into a mutation verdict merely because the same non-answer happened twice.
/// An execution that established nothing is neither: a replay saying the finding is still there is a claim that the measurement was made again.
const fn observed(kind: FindingKind, outcome: rust_mutants::outcome::Outcome) -> Outcome {
    use rust_mutants::outcome::Outcome as Measured;
    match kind {
        FindingKind::Timeout | FindingKind::WaitedMutant => match outcome {
            Measured::Waited => Outcome::Reproduced,
            Measured::Killed | Measured::Survived => Outcome::Resolved,
            Measured::NotRun
            | Measured::StepLimitReached
            | Measured::Inconclusive
            | Measured::Errored => Outcome::Inconclusive,
        },
        FindingKind::BrokenUnderFault
        | FindingKind::DimensionNotMeasured
        | FindingKind::CorruptAfterCrash => Outcome::Inconclusive,
        FindingKind::StepLimitReachedMutant => match outcome {
            Measured::StepLimitReached => Outcome::Reproduced,
            Measured::Killed | Measured::Survived => Outcome::Resolved,
            Measured::NotRun | Measured::Waited | Measured::Inconclusive | Measured::Errored => {
                Outcome::Inconclusive
            }
        },
        FindingKind::BuildFailure
        | FindingKind::FailingTest
        | FindingKind::TargetMissing
        | FindingKind::SurvivingMutant
        | FindingKind::NotMeasured
        | FindingKind::UnmatchedAcceptance
        | FindingKind::UndefinedBehaviour
        | FindingKind::HollowTarget
        | FindingKind::WireUnnoticed
        | FindingKind::UnstableBaseline
        | FindingKind::EnvironmentDependent
        | FindingKind::EnvironmentDependentReach
        | FindingKind::UnnoticedFault => match outcome {
            Measured::Survived => Outcome::Reproduced,
            Measured::Killed => Outcome::Resolved,
            Measured::NotRun
            | Measured::StepLimitReached
            | Measured::Waited
            | Measured::Inconclusive
            | Measured::Errored => Outcome::Inconclusive,
        },
    }
}

#[cfg(test)]
mod tests {
    use rust_mutants::outcome::Outcome as Measured;

    use super::{FindingKind, Outcome, observed};

    /// The outcomes that answer no question a finding can ask: nothing ran, the run could not decide, or the harness failed.
    const ESTABLISH_NOTHING: [Measured; 3] =
        [Measured::NotRun, Measured::Inconclusive, Measured::Errored];

    #[test]
    fn a_replay_that_established_nothing_says_so_rather_than_picking_a_side() {
        for kind in FindingKind::ALL {
            for measured in ESTABLISH_NOTHING {
                assert_eq!(
                    observed(kind, measured),
                    Outcome::Inconclusive,
                    "a replay of a {} finding came back {}, which is the run saying it \
                     measured nothing; saying the finding is still there, or that it is \
                     gone, is a claim about a measurement that was never made",
                    kind.name(),
                    measured.name()
                );
            }
        }
    }
}
