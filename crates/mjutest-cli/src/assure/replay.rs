// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Putting one finding back to the tests, with nothing read back and nothing kept.
//!
//! A replay is an operation rather than an assurance about a project: it
//! answers whether the one finding it was given is still there, and advances no
//! index and stores no verdict. It reads no evidence and no cache, which is how
//! a timeout — the one outcome a run never reuses as a proof — is deliberately
//! run again.
//!
//! It also asks nothing of coverage. The finding says that nothing noticed a
//! mutation, and the way to put that to the tests again is to offer the
//! mutation to every test of its own package rather than to the ones a
//! measurement once said could reach it.

use std::path::Path;
use std::time::Duration;

use rust_mutants::session::{PrepareOptions, Request as ExecRequest, Timeout};
use rust_mutants::workspace::{OpenOptions, Workspace};

use crate::build::Cargo;
use crate::cli::{EXIT_ASSURED, EXIT_DEFECT, Environment};
use crate::error::RunnerError;
use crate::report::FindingKind;
use crate::watch::Watch;

/// What a replay established about the finding it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The finding is still there.
    Reproduced,
    /// The finding is not there any more.
    Resolved,
}

impl Outcome {
    /// The word a reader sees.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Reproduced => "REPRODUCED",
            Self::Resolved => "RESOLVED",
        }
    }

    /// The exit code this outcome earns, which is the one a finding earns.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Reproduced => EXIT_DEFECT,
            Self::Resolved => EXIT_ASSURED,
        }
    }
}

/// What one replay needs.
#[expect(
    missing_debug_implementations,
    reason = "an environment is a handle on the outside world; there is nothing to print about one"
)]
#[derive(Clone, Copy)]
pub struct Replaying<'a> {
    /// The tree the finding is about, which is only ever read.
    pub root: &'a Path,
    /// The environment the snapshot's commands run with.
    pub environment: &'a Environment,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// How long one execution may take.
    pub timeout: Option<Duration>,
}

/// Offers `mutant` to the tests again and says whether `kind` is still observable.
///
/// # Errors
/// Only what stopped the replay from happening: a snapshot that could not be
/// taken, a toolchain that could not be reached, a tree that does not build. A
/// mutation the tree no longer holds is one of those, because a finding about a
/// mutation that is not there is not a finding this can answer.
pub fn replay(
    replaying: &Replaying<'_>,
    mutant: &str,
    kind: FindingKind,
    watch: Watch<'_>,
) -> Result<Outcome, RunnerError> {
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
            report_directory: Some("reports".to_owned()),
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
            mutant_timeout: replaying.timeout.map_or(Timeout::Auto, Timeout::Fixed),
            ..PrepareOptions::default()
        },
        watch.cancel,
    )?;
    let outcome = session
        .resolve(mutant)
        .map_err(RunnerError::from)
        .and_then(|found| {
            session
                .exec(
                    &ExecRequest::new(found.id.clone()).with_timeout(replaying.timeout),
                    watch.cancel,
                )
                .map_err(RunnerError::from)
        })
        .map(|result| observed(kind, result.outcome));
    session.close()?;
    outcome
}

/// Whether the finding is still what the tests say.
fn observed(kind: FindingKind, outcome: rust_mutants::outcome::Outcome) -> Outcome {
    let still = match kind {
        FindingKind::Timeout => outcome == rust_mutants::outcome::Outcome::TimedOut,
        _ => !outcome.detected(),
    };
    if still {
        Outcome::Reproduced
    } else {
        Outcome::Resolved
    }
}
