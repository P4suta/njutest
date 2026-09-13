// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest fix`: what repairs a run was offered, and writing the ones that still hold up.
//!
//! Nothing is written on the strength of what a run recorded. `--apply` puts
//! every test a provider wrote to the compiler and the tests again, in a
//! snapshot, and checks that the file it patches is still the file the
//! provider saw. A corpus entry is an input rather than a claim, so what is
//! checked of it is that the file it would create is still not there.

use std::io::Write;
use std::path::Path;

use crate::app::runs;
use crate::assure::repair::{Checking, Verdict};
use crate::build::Cargo;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, EXIT_INSUFFICIENT, Environment, Fix as Arguments};
use crate::report::CandidateRecord;
use crate::trace::Recorder;
use crate::watch::Watch;

/// The bound on each build or execution when a recorded repair is checked again.
pub const RECHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Says what a run was offered, and writes what still holds up.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let report = match runs::report(root, &run) {
        Ok(report) => report,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let selected: Vec<&CandidateRecord> = report
        .candidates
        .iter()
        .filter(|candidate| {
            arguments
                .candidate
                .as_ref()
                .is_none_or(|prefix| candidate.digest.starts_with(prefix))
        })
        .collect();
    if let Some(prefix) = arguments.candidate.as_deref()
        && selected.is_empty()
        && !report.candidates.is_empty()
    {
        super::diagnose(
            stderr,
            &format!(
                "{}: no candidate of {run} starts with {prefix}; it was offered {}",
                crate::error::RUN_NOT_FOUND.code,
                report.candidates.len()
            ),
        );
        return EXIT_ERROR;
    }
    if selected.is_empty() {
        super::say(stdout, &format!("{run} was offered no repair"));
        return EXIT_ASSURED;
    }
    if !arguments.apply {
        for candidate in &selected {
            super::say(stdout, &listed(candidate));
        }
        return EXIT_ASSURED;
    }
    apply(
        &selected,
        &Applying {
            root,
            environment,
            arguments,
        },
        stdout,
        stderr,
    )
}

/// What one candidate looks like to a reader.
fn listed(candidate: &CandidateRecord) -> String {
    let standing = if candidate.accepted {
        format!(
            "held up ({} stable, {} killing)",
            candidate.stability_runs, candidate.kill_runs
        )
    } else {
        candidate
            .why
            .clone()
            .unwrap_or_else(|| "did not hold up".to_owned())
    };
    format!(
        "{} {} {} — {standing}",
        candidate.digest.get(..12).unwrap_or(&candidate.digest),
        candidate.kind,
        candidate.path
    )
}

/// What an application runs with.
struct Applying<'a> {
    root: &'a Path,
    environment: &'a Environment,
    arguments: &'a Arguments,
}

/// Puts every selected candidate to the tests again and writes the ones that hold.
fn apply(
    selected: &[&CandidateRecord],
    applying: &Applying<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let Applying {
        root,
        environment,
        arguments,
    } = *applying;
    let trace = Recorder::disabled();
    let watch = Watch::new(&environment.cancel, &trace);
    let config = match crate::config::Config::load(root) {
        Ok(config) => config,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let build = config.execution.build();
    let checking = Checking {
        root,
        environment,
        cargo: Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
        build,
        harness_args: config.execution.test_binary_args,
        skip_targets: config.execution.skip_targets,
        timeout: RECHECK_TIMEOUT,
    };
    let mut written = 0u32;
    let mut already = 0u32;
    let mut refused = 0u32;
    for candidate in selected {
        match one(candidate, &checking, watch) {
            Ok(Taken::Written(proposal)) => match write(root, &proposal) {
                Ok(()) => {
                    written = written.saturating_add(1);
                    super::say(stdout, &format!("wrote {}", proposal.path));
                }
                Err(why) => {
                    refused = refused.saturating_add(1);
                    super::diagnose(stderr, &why);
                }
            },
            Ok(Taken::Already(path)) => {
                already = already.saturating_add(1);
                super::say(
                    stdout,
                    &format!("{path} is already what the candidate would write"),
                );
            }
            Err(why) => {
                refused = refused.saturating_add(1);
                super::diagnose(stderr, &why);
            }
        }
    }
    super::say(
        stdout,
        &format!("{written} written, {already} already there, {refused} not"),
    );
    if refused > 0 {
        EXIT_INSUFFICIENT
    } else {
        EXIT_ASSURED
    }
}

/// What became of one candidate a `--apply` looked at.
enum Taken {
    /// It holds up and is the file to write.
    Written(crate::repair::Proposal),
    /// The tree already holds exactly what it would write.
    Already(String),
}

/// One candidate, checked afresh: what a run recorded is where to look, never a reason to write.
fn one(
    candidate: &CandidateRecord,
    checking: &Checking<'_>,
    watch: Watch<'_>,
) -> Result<Taken, String> {
    if !candidate.accepted {
        return Err(format!("skipped {}: {}", candidate.path, listed(candidate)));
    }
    let content = crate::repair::load(checking.root, &candidate.digest).ok_or_else(|| {
        format!(
            "{}: the content of {} is not in {}",
            crate::error::GENERATION_PROTOCOL.code,
            candidate.path,
            crate::repair::STORE
        )
    })?;
    let kind = crate::repair::Kind::parse(&candidate.kind).ok_or_else(|| {
        format!(
            "{}: {} is a candidate of a kind this release does not write",
            crate::error::GENERATION_PROTOCOL.code,
            candidate.path
        )
    })?;
    let proposal = crate::repair::Proposal {
        kind,
        path: candidate.path.clone(),
        preimage: candidate.preimage.clone(),
        digest: candidate.digest.clone(),
        content,
    };
    let on_disk = crate::repair::preimage_of(checking.root, &proposal.path);
    if on_disk.as_deref() == Some(proposal.digest.as_str()) {
        return Ok(Taken::Already(proposal.path));
    }
    if on_disk != proposal.preimage {
        return Err(format!(
            "{}: {} is not the file the provider saw",
            crate::error::GENERATION_PREIMAGE_MOVED.code,
            proposal.path
        ));
    }
    if proposal.kind == crate::repair::Kind::Corpus {
        return Ok(Taken::Written(proposal));
    }
    match crate::assure::repair::check(checking, &proposal, &candidate.mutant, watch) {
        Ok(Verdict { accepted: true, .. }) => Ok(Taken::Written(proposal)),
        Ok(verdict) => Err(format!(
            "{} no longer holds up: {}",
            proposal.path,
            verdict.why.unwrap_or_else(|| "no reason given".to_owned())
        )),
        Err(error) => Err(error.to_string()),
    }
}

/// Writes one candidate into the tree a person is working in.
fn write(root: &Path, proposal: &crate::repair::Proposal) -> Result<(), String> {
    let path = root.join(&proposal.path);
    rust_mutants::replace::file(&path, &proposal.content).map_err(|failure| {
        format!(
            "cannot write {}: {}",
            failure.path.display(),
            failure.source
        )
    })
}
