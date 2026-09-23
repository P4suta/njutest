// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest replay`: put one finding back to the tests and say whether it is still there.

use std::io::Write;

use crate::app::runs;
use crate::assure::replay::{Outcome, Replaying, replay};
use crate::build::Cargo;
use crate::cli::{EXIT_ERROR, Environment, Replay as Arguments};
use crate::config::Config;
use crate::report::FindingKind;
use crate::trace::Recorder;
use crate::watch::Watch;

/// Why a finding prefix does not select exactly one stored finding.
#[derive(Debug, thiserror::Error)]
enum SelectionError {
    /// The stored run or its report could not be read.
    #[error(transparent)]
    Run(#[from] runs::RunError),
    /// The completed report could not reproduce its exact projection.
    #[error(transparent)]
    Count(#[from] crate::report::CountError),
    /// No finding has the requested prefix.
    #[error(
        "{}: no finding of {run} starts with {prefix}",
        crate::error::RUN_NOT_FOUND.code
    )]
    Missing { run: String, prefix: String },
    /// More than one finding has the requested prefix.
    #[error(
        "{}: {prefix} names {count} findings: {matches}",
        crate::error::RUN_NOT_FOUND.code
    )]
    Ambiguous {
        prefix: String,
        count: usize,
        matches: String,
    },
}

/// Puts one finding back to the tests.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let found = match selected(arguments, environment) {
        Ok(found) => found,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let config = found.config.clone();
    let build = config.execution.build();
    let cancel = environment.cancel.clone();
    let trace = Recorder::disabled();
    let watch = Watch::new(&cancel, &trace);
    let outcome = replay(
        &Replaying {
            root,
            environment,
            cargo: Cargo {
                offline: arguments.offline,
                locked: arguments.locked,
            },
            build,
            harness_args: config.execution.test_binary_args,
            skip_targets: config.execution.skip_targets,
            timeout: None,
            steps: config.execution.steps,
            reports: config.reports.directory,
        },
        &found.subject,
        found.kind,
        watch,
    );
    match outcome {
        Ok(outcome) => {
            super::say(
                stdout,
                &format!(
                    "REPLAY\t{}\t{}\t{}",
                    found.subject,
                    crate::report::lines::escape(&found.named),
                    outcome.name()
                ),
            )?;
            Ok(said(outcome))
        }
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            Ok(EXIT_ERROR)
        }
    }
}

/// The exit code a replay leaves by, which is the one its finding earns.
const fn said(outcome: Outcome) -> u8 {
    outcome.exit_code()
}

/// One finding of a stored run, named by any prefix of its subject.
struct Selected {
    subject: String,
    kind: FindingKind,
    named: String,
    config: Config,
}

/// The finding the arguments name, from the run they name.
fn selected(arguments: &Arguments, environment: &Environment) -> Result<Selected, SelectionError> {
    let root = &environment.working_directory;
    let run = runs::resolve(root, arguments.run.as_deref())?;
    let report = runs::report(&run)?;
    let conclusion = report.conclusion()?;
    let every: Vec<&crate::report::ProjectedMutant> = conclusion.mutants.iter().collect();
    let named: Vec<&str> = crate::naming::matching(&every, &arguments.finding)
        .iter()
        .map(|mutant| mutant.display_id())
        .collect();
    let matching: Vec<&crate::report::Finding> = conclusion
        .findings
        .iter()
        .filter(|finding| {
            finding.subject.starts_with(&arguments.finding)
                || named.contains(&finding.subject.as_str())
        })
        .collect();
    match matching.as_slice() {
        [only] => Ok(Selected {
            subject: only.subject.clone(),
            kind: only.kind,
            named: only.kind_name(),
            config: run.config().clone(),
        }),
        [] => Err(SelectionError::Missing {
            run: run.id().to_string(),
            prefix: arguments.finding.clone(),
        }),
        several => Err(SelectionError::Ambiguous {
            prefix: arguments.finding.clone(),
            count: several.len(),
            matches: several
                .iter()
                .map(|one| format!("{} ({})", one.subject, one.kind_name()))
                .collect::<Vec<String>>()
                .join(", "),
        }),
    }
}
