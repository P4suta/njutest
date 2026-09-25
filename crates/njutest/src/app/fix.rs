// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest fix`: what repairs a run was offered, and writing the ones that still hold up.

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
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let report = match runs::report(&run) {
        Ok(report) => report,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let conclusion = match report.conclusion() {
        Ok(conclusion) => conclusion,
        Err(error) => {
            super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
            return Ok(EXIT_ERROR);
        }
    };
    let selected: Vec<&CandidateRecord> = conclusion
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
        && !conclusion.candidates.is_empty()
    {
        super::diagnose(
            stderr,
            &format!(
                "{}: no candidate of {} starts with {prefix}; it was offered {}",
                crate::error::RUN_NOT_FOUND.code,
                run.id(),
                conclusion.candidates.len()
            ),
        )?;
        return Ok(EXIT_ERROR);
    }
    if selected.is_empty() {
        super::say(stdout, &format!("{} was offered no repair", run.id()))?;
        return Ok(EXIT_ASSURED);
    }
    if !arguments.apply {
        for candidate in &selected {
            super::say(stdout, &listed(candidate))?;
        }
        return Ok(EXIT_ASSURED);
    }
    apply(
        &selected,
        &Applying {
            root,
            environment,
            arguments,
            config: run.config(),
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

/// What a recorded candidate is checked again with, before anything is written.
pub(crate) fn checking<'a>(
    (root, environment): (&'a Path, &'a Environment),
    config: &crate::config::Config,
    cargo: Cargo,
) -> Checking<'a> {
    let config = config.clone();
    Checking {
        root,
        environment,
        cargo,
        build: config.execution.build(),
        harness_args: config.execution.test_binary_args,
        skip_targets: config.execution.skip_targets,
        timeout: RECHECK_TIMEOUT,
        steps: config.execution.steps,
        build_timeout: config.execution.build_timeout,
        reports: config.reports.directory,
    }
}

/// What an application runs with.
struct Applying<'a> {
    root: &'a Path,
    environment: &'a Environment,
    arguments: &'a Arguments,
    config: &'a crate::config::Config,
}

/// Puts every selected candidate to the tests again and writes the ones that hold.
fn apply(
    selected: &[&CandidateRecord],
    applying: &Applying<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let Applying {
        root,
        environment,
        arguments,
        config,
    } = *applying;
    let trace = Recorder::disabled();
    let watch = Watch::new(&environment.cancel, &trace);
    let checking = checking(
        (root, environment),
        config,
        Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
    );
    let mut written = 0u32;
    let mut already = 0u32;
    let mut refused = 0u32;
    for candidate in selected {
        match one(candidate, &checking, watch) {
            Ok(Taken::Written(proposal)) => match write(root, &proposal) {
                Ok(()) => {
                    written = written.saturating_add(1);
                    super::say(stdout, &format!("wrote {}", proposal.path))?;
                }
                Err(why) => {
                    refused = refused.saturating_add(1);
                    super::diagnose(stderr, &why.to_string())?;
                }
            },
            Ok(Taken::Already(path)) => {
                already = already.saturating_add(1);
                super::say(
                    stdout,
                    &format!("{path} is already what the candidate would write"),
                )?;
            }
            Err(why) => {
                refused = refused.saturating_add(1);
                super::diagnose(stderr, &why.to_string())?;
            }
        }
    }
    super::say(
        stdout,
        &format!("{written} written, {already} already there, {refused} not"),
    )?;
    if refused > 0 {
        Ok(EXIT_INSUFFICIENT)
    } else {
        Ok(EXIT_ASSURED)
    }
}

/// What became of one candidate a `--apply` looked at.
pub(crate) enum Taken {
    /// It holds up and is the file to write.
    Written(crate::repair::Proposal),
    /// The tree already holds exactly what it would write.
    Already(String),
}

/// Why a recorded repair cannot be written to the current tree.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CandidateError {
    /// The earlier run did not establish the candidate.
    #[error("skipped {path}: {standing}")]
    NotAccepted { path: String, standing: String },
    /// The content-addressed candidate body is absent or corrupt.
    #[error(
        "{}: the content of {path} is not in {}",
        crate::error::GENERATION_PROTOCOL.code,
        crate::repair::STORE
    )]
    ContentMissing { path: String },
    /// This release cannot interpret the candidate kind.
    #[error(
        "{}: {path} is a candidate of a kind this release does not write",
        crate::error::GENERATION_PROTOCOL.code
    )]
    UnknownKind { path: String },
    /// The file has changed since the provider read it.
    #[error(
        "{}: {path} is not the file the provider saw",
        crate::error::GENERATION_PREIMAGE_MOVED.code
    )]
    PreimageMoved { path: String },
    /// A fresh check no longer supports the candidate.
    #[error("{path} no longer holds up: {reason}")]
    Rejected { path: String, reason: String },
    /// The fresh assurance run itself could not complete.
    #[error(transparent)]
    Check(Box<crate::error::RunnerError>),
    /// The checked content could not be installed.
    #[error("cannot write {}: {source}", path.display())]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One candidate, checked afresh: what a run recorded is where to look, never a reason to write.
pub(crate) fn one(
    candidate: &CandidateRecord,
    checking: &Checking<'_>,
    watch: Watch<'_>,
) -> Result<Taken, CandidateError> {
    if !candidate.accepted {
        return Err(CandidateError::NotAccepted {
            path: candidate.path.clone(),
            standing: listed(candidate),
        });
    }
    let content = crate::repair::load(checking.root, &candidate.digest).ok_or_else(|| {
        CandidateError::ContentMissing {
            path: candidate.path.clone(),
        }
    })?;
    let kind =
        crate::repair::Kind::parse(&candidate.kind).ok_or_else(|| CandidateError::UnknownKind {
            path: candidate.path.clone(),
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
        return Err(CandidateError::PreimageMoved {
            path: proposal.path,
        });
    }
    if proposal.kind == crate::repair::Kind::Corpus {
        return Ok(Taken::Written(proposal));
    }
    match crate::assure::repair::check(checking, &proposal, &candidate.mutant, watch) {
        Ok(Verdict { accepted: true, .. }) => Ok(Taken::Written(proposal)),
        Ok(verdict) => Err(CandidateError::Rejected {
            path: proposal.path,
            reason: verdict.why.unwrap_or_else(|| "no reason given".to_owned()),
        }),
        Err(error) => Err(CandidateError::Check(Box::new(error))),
    }
}

/// Writes one candidate into the tree a person is working in.
pub(crate) fn write(root: &Path, proposal: &crate::repair::Proposal) -> Result<(), CandidateError> {
    let path = root.join(&proposal.path);
    rust_mutants::replace::file(&path, &proposal.content).map_err(|failure| CandidateError::Write {
        path: failure.path,
        source: failure.source,
    })
}
