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

/// Puts one finding back to the tests.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = &environment.working_directory;
    let found = match selected(arguments, environment) {
        Ok(found) => found,
        Err(message) => {
            super::diagnose(stderr, &message);
            return EXIT_ERROR;
        }
    };
    let config = match Config::load(root) {
        Ok(config) => config,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };
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
            reports: crate::app::reports::Store::of(root, &config.reports.directory),
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
            );
            said(outcome)
        }
        Err(error) => {
            super::complain(stderr, &error, error.code());
            EXIT_ERROR
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
}

/// The finding the arguments name, from the run they name.
fn selected(arguments: &Arguments, environment: &Environment) -> Result<Selected, String> {
    let root = &environment.working_directory;
    let run = runs::resolve(root, arguments.run.as_deref()).map_err(|error| error.to_string())?;
    let report = runs::report(root, &run).map_err(|error| error.to_string())?;
    let every: Vec<&crate::report::MutantRecord> = report.mutants.iter().collect();
    let named: Vec<&str> = crate::naming::matching(&every, &arguments.finding)
        .iter()
        .map(|mutant| mutant.display_id.as_str())
        .collect();
    let matching: Vec<&crate::report::Finding> = report
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
        }),
        [] => Err(format!(
            "{}: no finding of {run} starts with {}",
            crate::error::RUN_NOT_FOUND.code,
            arguments.finding
        )),
        several => Err(format!(
            "{}: {} names {} findings: {}",
            crate::error::RUN_NOT_FOUND.code,
            arguments.finding,
            several.len(),
            several
                .iter()
                .map(|one| format!("{} ({})", one.subject, one.kind_name()))
                .collect::<Vec<String>>()
                .join(", ")
        )),
    }
}
