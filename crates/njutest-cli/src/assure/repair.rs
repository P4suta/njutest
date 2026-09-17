// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Putting a candidate to the compiler and the tests before anybody is offered it.

use std::path::Path;
use std::time::Duration;

use rust_mutants::session::{PrepareOptions, Request as ExecRequest, Timeout};
use rust_mutants::workspace::{OpenOptions, Workspace};

use crate::build::Cargo;
use crate::cli::Environment;
use crate::error::RunnerError;
use crate::repair::Proposal;
use crate::watch::Watch;

/// How many times the patched tree must pass with nothing active.
pub const STABILITY_RUNS: u32 = 3;

/// How many times the patched tree must notice the mutant.
pub const KILL_RUNS: u32 = 2;

/// What a candidate is put to.
#[derive(Debug, Clone)]
pub struct Checking<'a> {
    /// The tree the candidate is about, which is only ever read.
    pub root: &'a Path,
    /// The environment the snapshot's commands run with.
    pub environment: &'a Environment,
    /// How cargo is bounded.
    pub cargo: Cargo,
    /// What the tree is compiled as, which must be what the run compiled: a candidate held to another build is held to another program.
    pub build: rust_mutants::cargo::BuildConfig,
    /// The arguments the test binaries are started with, which must be the ones the run used: a candidate held to a suite running another way is held to another suite.
    pub harness_args: Vec<String>,
    /// Test targets the run deliberately left out, which a repair check must leave out too.
    pub skip_targets: Vec<String>,
    /// How long one execution may take.
    pub timeout: Duration,
    /// How long the build may take, which is not how long a measurement may take. `None` is no bound.
    pub build_timeout: Option<Duration>,
    /// Where this project keeps what its runs leave behind, which the tree under test is copied without.
    pub reports: crate::app::reports::Store,
}

/// What putting a candidate to the tests established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// How many times the patched tree passed with nothing active.
    pub stable: u32,
    /// How many times it noticed the mutant.
    pub killed: u32,
    /// Whether it may be offered.
    pub accepted: bool,
    /// Why it may not, when it may not.
    pub why: Option<String>,
}

impl Verdict {
    /// A candidate refused for `why`, with what it did establish.
    #[must_use]
    pub fn refused(stable: u32, killed: u32, why: impl Into<String>) -> Self {
        Self {
            stable,
            killed,
            accepted: false,
            why: Some(why.into()),
        }
    }
}

/// Puts one candidate to the compiler and the tests.
///
/// # Errors
/// Only what stopped the check from happening at all: a snapshot that could
/// not be taken, a toolchain that could not be reached. A candidate that does
/// not work is a verdict, not an error.
pub fn check(
    checking: &Checking<'_>,
    proposal: &Proposal,
    mutant: &str,
    watch: Watch<'_>,
) -> Result<Verdict, RunnerError> {
    let workspace = Workspace::open(
        checking.root,
        OpenOptions {
            allow_outside: Vec::new(),
            cargo: None,
            search_path: checking
                .environment
                .var("PATH")
                .map(std::ffi::OsStr::to_owned),
            env: checking.environment.vars.clone(),
            temp_directory: checking.environment.temp_directory.clone(),
            report_directory: Some(checking.reports.relative()),
            exclude: Vec::new(),
            keep_temp: false,
            offline: checking.cargo.offline,
            locked: checking.cargo.locked,
            trace: rust_mutants::trace::Recorder::disabled(),
        },
        watch.cancel,
    )?;
    if let Err(refusal) = write(workspace.snapshot_root(), proposal) {
        let closed = workspace.close();
        drop(closed);
        return Ok(Verdict::refused(0, 0, refusal));
    }
    let session = workspace.prepare(
        &PrepareOptions {
            verify: false,
            build: checking.build.clone(),
            harness_args: checking.harness_args.clone(),
            skip_targets: checking.skip_targets.clone(),
            build_timeout: checking.build_timeout,
            mutant_timeout: Timeout::Fixed(checking.timeout),
            ..PrepareOptions::default()
        },
        watch.cancel,
    )?;
    let verdict = put(&session, mutant, checking.timeout, watch);
    session.close()?;
    verdict
}

/// Runs the candidate against the original code and then against the mutant.
fn put(
    session: &rust_mutants::session::Session,
    mutant: &str,
    timeout: Duration,
    watch: Watch<'_>,
) -> Result<Verdict, RunnerError> {
    let Ok(found) = session.resolve(mutant) else {
        return Ok(Verdict::refused(
            0,
            0,
            "the mutant it claims to close is not in the patched tree".to_owned(),
        ));
    };
    let request = ExecRequest::new(found.id.clone()).with_timeout(Some(timeout));
    let mut stable = 0;
    for _attempt in 0..STABILITY_RUNS {
        let ran = session.control(&request, watch.cancel)?;
        if ran.outcome != rust_mutants::outcome::Outcome::Survived {
            return Ok(Verdict::refused(
                stable,
                0,
                format!(
                    "the patched tree does not pass with nothing active: {}",
                    ran.outcome.name()
                ),
            ));
        }
        stable = stable.saturating_add(1);
    }
    let mut killed = 0;
    for _attempt in 0..KILL_RUNS {
        let ran = session.exec(&request, watch.cancel)?;
        if !ran.outcome.detected() {
            return Ok(Verdict::refused(
                stable,
                killed,
                format!(
                    "the patched tree does not notice the mutant: {}",
                    ran.outcome.name()
                ),
            ));
        }
        killed = killed.saturating_add(1);
    }
    Ok(Verdict {
        stable,
        killed,
        accepted: true,
        why: None,
    })
}

/// Writes one candidate into a snapshot, which is a copy nobody is working in.
fn write(root: &Path, proposal: &Proposal) -> Result<(), String> {
    let path = root.join(&proposal.path);
    rust_mutants::replace::file(&path, &proposal.content).map_err(|failure| {
        format!(
            "cannot write {}: {}",
            failure.path.display(),
            failure.source
        )
    })
}
