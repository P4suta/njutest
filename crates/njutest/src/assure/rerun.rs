// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A stored report's sealed executions, run again before the report is reissued (ADR 0046, decision 7; ADR 0003: re-run, never replay).

use rust_mutants::sealed::record::{Evidence, SealedRun};
use rust_mutants::sealed::rerun::{Now, Recorded, Reproduction, Reran};

use crate::assure::run::{self, Request};
use crate::cli::Environment;
use crate::error::{CACHE_UNREPRODUCED, ErrorCode, RunnerError};
use crate::report::{BuildEvidence, MutantRecord, Report};
use crate::watch::Watch;

/// Why a stored answer is not reissued: the sealed executions it rests on did not come out the same when they ran again.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UnreproducedError {
    /// One execution came to something other than the answer recorded, or could not be made again.
    #[error(
        "{}: mutant {mutant} on {target}, test {test}, is stored as {stored}, and running it \
         again came to {now}; the stored answer is not reissued, and this run establishes \
         everything again",
        CACHE_UNREPRODUCED.code
    )]
    Differed {
        /// The mutant, as a person types it.
        mutant: String,
        /// The target whose sealed module ran it.
        target: String,
        /// The test.
        test: String,
        /// What the answer recorded it came to.
        stored: &'static str,
        /// What it came to now.
        now: Now,
    },
    /// The answer names a configured build this configuration does not make as the answer says it was made.
    #[error(
        "{}: the stored answer names configured build {build:?}, which this configuration does \
         not make as it was made; the stored answer is not reissued, and this run establishes \
         everything again",
        CACHE_UNREPRODUCED.code
    )]
    Unconfigured {
        /// The build's name.
        build: String,
    },
}

impl UnreproducedError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Differed { .. } | Self::Unconfigured { .. } => CACHE_UNREPRODUCED,
        }
    }
}

/// What running a stored report's sealed executions again found.
#[derive(Debug)]
pub enum Rerun {
    /// It rests on no sealed execution, so nothing it affirms rests on one, and nothing ran.
    NothingSealed,
    /// Every sealed execution it rests on came to what it recorded.
    Reproduced,
    /// It is not believed, and why.
    Unreproduced(UnreproducedError),
}

/// One sealed execution a row rests on, with the row.
type Executed<'report> = (&'report MutantRecord, &'report SealedRun);

/// What running a report's sealed executions again in the workspace at `root` asks for, as `config` configures it and `cargo` bounds its commands: no run of its own is measured, so nothing of one is kept.
#[must_use]
pub fn asking(
    root: &std::path::Path,
    config: crate::config::Config,
    (cargo, run_id, started): (
        crate::build::Cargo,
        rust_mutants::id::RunId,
        jiff::Timestamp,
    ),
) -> Request {
    Request {
        root: root.to_path_buf(),
        build: config.execution.build(),
        packages: run::asked_for(&[], &config),
        test_args: config.execution.test_binary_args.clone(),
        config,
        cargo,
        keep_temp: false,
        run_id,
        started,
        engine_trace: rust_mutants::trace::Recorder::disabled(),
        carried_evidence: None,
        evidence: crate::assure::identity::Evidence::default(),
        configuration: String::new(),
        changed: None,
        checkpoints: None,
        evidence_store: None,
        shard: None,
    }
}

/// Runs every sealed execution `stored` rests on again, build by build as `request` prepares each, and says whether all came out the same.
///
/// # Errors
/// What stopped the executions from running again: a build, a host, or an interruption.
pub fn rerun(
    stored: &Report,
    request: &Request,
    environment: &Environment,
    watch: Watch<'_>,
) -> Result<Rerun, RunnerError> {
    let builds: Vec<(&BuildEvidence, Vec<Executed<'_>>)> = stored
        .builds()
        .map(|build| (build, executed(build)))
        .filter(|(_, executions)| !executions.is_empty())
        .collect();
    if builds.is_empty() {
        return Ok(Rerun::NothingSealed);
    }
    let phase = watch.trace.phase("rerun");
    for (build, executions) in builds {
        let Some(configured) = configured(request, build) else {
            return Ok(Rerun::Unreproduced(UnreproducedError::Unconfigured {
                build: build.name().as_str().to_owned(),
            }));
        };
        let asked = Request {
            build: configured,
            keep_temp: false,
            ..request.clone()
        };
        if let Some(differed) = reproduced(&asked, (environment, watch), &executions)? {
            phase.end();
            return Ok(Rerun::Unreproduced(differed));
        }
    }
    phase.end();
    Ok(Rerun::Reproduced)
}

/// Every sealed execution `build`'s rows rest on, row by row and each row's in the order they ran.
fn executed(build: &BuildEvidence) -> Vec<Executed<'_>> {
    let mut executed = Vec::new();
    for row in build.rows() {
        match &row.evidence {
            Some(Evidence::Sealed { executions }) => {
                executed.extend(executions.iter().map(|execution| (row, execution)));
            }
            Some(Evidence::Unproven { .. }) | None => {}
        }
    }
    executed
}

/// The build this configuration makes under `build`'s name, where it makes it as `build` says it was made.
fn configured(
    request: &Request,
    build: &BuildEvidence,
) -> Option<rust_mutants::cargo::BuildConfig> {
    std::iter::once((crate::config::DEFAULT_CONFIGURATION, request.build.clone()))
        .chain(
            request
                .config
                .configuration
                .iter()
                .map(|one| (one.name.as_str(), one.build())),
        )
        .find(|(name, made)| {
            *name == build.name().as_str() && made.selection() == *build.selection()
        })
        .map(|(_, made)| made)
}

/// Runs `executions` again on a workspace prepared as `asked` prepares its build, records each that ran in the trace, and names the first that did not come out the same, where one did not.
///
/// # Errors
/// What stopped them from running again.
fn reproduced(
    asked: &Request,
    (environment, watch): (&Environment, Watch<'_>),
    executions: &[Executed<'_>],
) -> Result<Option<UnreproducedError>, RunnerError> {
    let recorded: Vec<Recorded> = executions
        .iter()
        .map(|(row, execution)| Recorded {
            mutant: row.id.clone(),
            target: execution.target.clone(),
            test: execution.test.clone(),
            came_to: execution.came_to,
        })
        .collect();
    let rerunnable = rust_mutants::workspace::Workspace::open(
        &asked.root,
        run::opening(asked, environment),
        watch.cancel,
    )?
    .prepare_to_rerun(&run::preparing(asked)?, watch.cancel)?;
    let reproduction = rerunnable.rerun(&recorded, watch.cancel)?;
    rerunnable.close()?;
    let (agreed, first) = match reproduction {
        Reproduction::Reproduced(agreed) => (agreed, None),
        Reproduction::Differed { agreed, first } => (agreed, Some(first)),
    };
    for (again, (row, _)) in agreed.iter().chain(first.iter()).zip(executions) {
        traced(watch, row, again);
    }
    let Some(first) = first else {
        return Ok(None);
    };
    let mutant = match executions.get(agreed.len()) {
        Some((row, _)) => row.display_id.clone(),
        None => first.recorded.mutant.clone(),
    };
    Ok(Some(UnreproducedError::Differed {
        mutant,
        target: first.recorded.target,
        test: first.recorded.test,
        stored: first.recorded.came_to.name(),
        now: first.now,
    }))
}

/// Records `again`, one execution `row` rests on, in the trace, where it ran.
fn traced(watch: Watch<'_>, row: &MutantRecord, again: &Reran) {
    match again.now {
        Now::Came(came_to) => watch.trace.sealed_exec(crate::trace::SealedExecRecord {
            mutant: row.display_id.clone(),
            target: again.recorded.target.clone(),
            test: again.recorded.test.clone(),
            came_to: came_to.name().to_owned(),
        }),
        Now::Unmade(why) => watch.trace.note(
            "rerun-unmade",
            &format!(
                "{} on {}, test {}: {why}",
                row.display_id, again.recorded.target, again.recorded.test
            ),
        ),
    }
}
