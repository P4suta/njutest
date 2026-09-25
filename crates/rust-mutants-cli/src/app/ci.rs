// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Carrying a run into a continuous integration job: the step summary, the step outputs, and one annotation per survivor.

use std::io::Write;
use std::path::{Path, PathBuf};

use rust_mutants::outcome::Outcome;

use crate::error::CliError;
use crate::report::run::{RunDocument, RunMutantDocument};
use crate::{CiHost, Environment, cli, report, run};

/// How many error annotations GitHub Actions shows for one step; the survivors past it are counted rather than written.
pub const ERRORS_SHOWN_PER_STEP: usize = 10;

/// The verdict a run earned, as the exit code a job fails on and the word a later step reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Verdict {
    /// Every mutant the run decided, the tests noticed.
    Detected,
    /// The run found something a reader has to act on.
    Found,
    /// The run itself failed rather than answered.
    Failed,
    /// The run was interrupted.
    Interrupted,
}

impl Verdict {
    /// The verdict a run report's exit code carries, when it is one this release knows.
    #[must_use]
    pub fn of(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|verdict| verdict.code() == code)
    }

    /// The exit code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Detected => run::EXIT_DETECTED,
            Self::Found => run::EXIT_UNDETECTED,
            Self::Failed => run::EXIT_FAILED,
            Self::Interrupted => run::EXIT_INTERRUPTED,
        }
    }

    /// The word a later step reads from `verdict=`.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Found => "found",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// One run, and where the documents a later step reads are.
#[derive(Debug, Clone, Copy)]
pub struct Gated<'a> {
    /// The run.
    pub document: &'a RunDocument,
    /// The report it was read from.
    pub report: &'a Path,
    /// The workspace root its paths are relative to.
    pub root: &'a Path,
    /// Where its findings are written as SARIF, when they are.
    pub sarif: Option<&'a Path>,
}

/// Does what a continuous integration job asks.
///
/// # Errors
/// What the gate could not read, resolve, or write.
pub fn dispatch(
    command: &cli::CiCommand,
    environment: &Environment,
    stdout: &mut dyn Write,
) -> Result<u8, CliError> {
    match command {
        cli::CiCommand::Gate {
            root,
            run,
            report,
            sarif,
            host,
        } => {
            let root = environment.rooted(root.as_deref());
            let path = match report {
                Some(named) => environment.working_directory.join(named),
                None => {
                    let config = crate::config::Config::load(&root)?;
                    let directory =
                        super::stored::Store::of(&root, &config.reports.directory).root();
                    super::stored::report_of(&directory, run.as_deref())?
                }
            };
            let document = super::read_run_document(&path)?;
            let sarif: Option<PathBuf> = sarif
                .as_deref()
                .map(|named| environment.working_directory.join(named));
            gate(
                &Gated {
                    document: &document,
                    report: &path,
                    root: &root,
                    sarif: sarif.as_deref(),
                },
                &hosted(*host, &environment.ci)?,
                stdout,
            )
        }
    }
}

/// The host to write for: the one asked for, which has to be there, or the one the environment names.
fn hosted(asked: Option<cli::HostArg>, named: &CiHost) -> Result<CiHost, CliError> {
    match asked {
        None => Ok(named.clone()),
        Some(cli::HostArg::Plain) => Ok(CiHost::None),
        Some(cli::HostArg::Gitlab) => Ok(CiHost::GitLab),
        Some(cli::HostArg::Github) => match named {
            CiHost::GitHub { .. } => Ok(named.clone()),
            CiHost::GitLab | CiHost::None => Err(CliError::CiHostUnavailable { host: "github" }),
        },
    }
}

/// Writes a run where `host` shows it, and returns the exit code its verdict carries.
///
/// # Errors
/// A report whose exit code is no verdict, a root outside the checkout, a path no step output can hold, or a file that could not be written.
pub fn gate(gated: &Gated<'_>, host: &CiHost, stdout: &mut dyn Write) -> Result<u8, CliError> {
    let verdict =
        Verdict::of(gated.document.run.exit_code).ok_or_else(|| CliError::ReportMissing {
            message: format!(
                "{} exits {}, which is no verdict this release knows",
                gated.report.display(),
                gated.document.run.exit_code
            ),
        })?;
    if let Some(path) = gated.sarif {
        let log = serde_json::to_string(&report::sarif::log(gated.document))
            .map_err(|source| CliError::OutputEncodingFailed { source })?;
        std::fs::write(path, format!("{log}\n")).map_err(|error| CliError::writing(path, error))?;
    }
    let lines = report::lines(gated.document)?;
    match host {
        CiHost::GitHub {
            summary,
            output,
            workspace,
        } => {
            let prefix = inside(gated.root, workspace)?;
            let outputs = outputs(verdict, gated)?;
            let survivors = survivors(gated.document);
            let shown = survivors.len().min(ERRORS_SHOWN_PER_STEP);
            let mut markdown = report::markdown::document(gated.document);
            if shown < survivors.len() {
                crate::text::line(
                    &mut markdown,
                    format_args!(
                        "\n{}",
                        truncated(shown, survivors.len(), gated.sarif.is_some())
                    ),
                );
            }
            append(summary, &markdown)?;
            append(output, &outputs)?;
            let mut said = lines;
            for mutant in survivors.into_iter().take(shown) {
                crate::text::line(
                    &mut said,
                    format_args!("{}", report::annotations::surviving(&prefix, mutant)),
                );
            }
            super::write(stdout, &said)?;
        }
        CiHost::GitLab | CiHost::None => super::write(stdout, &lines)?,
    }
    Ok(verdict.code())
}

/// The sentence that says how many survivors a reader was not shown, and where every one of them is.
#[must_use]
pub fn truncated(shown: usize, survivors: usize, sarif: bool) -> String {
    let held = if sarif {
        "SARIF and the report"
    } else {
        "the report"
    };
    format!("Shown {shown} of {survivors} survivors; all {survivors} are in {held}.")
}

/// The survivors no claim accounts for, in catalog order.
fn survivors(document: &RunDocument) -> Vec<&RunMutantDocument> {
    document
        .mutants
        .iter()
        .filter(|mutant| mutant.outcome == Outcome::Survived && !mutant.expected)
        .collect()
}

/// The `name=value` lines a later step reads.
fn outputs(verdict: Verdict, gated: &Gated<'_>) -> Result<String, CliError> {
    let mut said = format!("verdict={}\n", verdict.word());
    for (name, flag, path) in [
        ("report", "--report", Some(gated.report)),
        ("sarif", "--sarif", gated.sarif),
    ] {
        let Some(path) = path else {
            continue;
        };
        let text = path.display().to_string();
        if text.contains(['\n', '\r']) {
            return Err(CliError::InvalidValue {
                flag: flag.to_owned(),
                value: text,
                expected: "a path without a line break, which one step output line cannot hold"
                    .to_owned(),
            });
        }
        crate::text::line(&mut said, format_args!("{name}={text}"));
    }
    Ok(said)
}

/// The root's place inside the checkout, as the slash-terminated prefix an annotation's file is named with.
fn inside(root: &Path, checkout: &Path) -> Result<String, CliError> {
    let resolved = |path: &Path| {
        std::fs::canonicalize(path).map_err(|source| CliError::CiPathUnresolved {
            path: path.to_path_buf(),
            source,
        })
    };
    let (real_root, real_checkout) = (resolved(root)?, resolved(checkout)?);
    let within = real_root
        .strip_prefix(&real_checkout)
        .map_err(|_not_a_prefix| CliError::CiRootOutsideCheckout {
            root: root.to_path_buf(),
            checkout: checkout.to_path_buf(),
        })?;
    if within.as_os_str().is_empty() {
        return Ok(String::new());
    }
    rust_mutants::id::slashed(within)
        .map(|text| format!("{text}/"))
        .map_err(|source| CliError::PathNotUtf8 {
            context: "the workspace root inside the checkout",
            source,
        })
}

/// Appends to a file the host named, which an earlier step may already have written to.
fn append(path: &Path, text: &str) -> Result<(), CliError> {
    let unwritable = |source| CliError::CiSinkUnwritable {
        path: path.to_path_buf(),
        source,
    };
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(unwritable)?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.flush())
        .map_err(unwritable)
}
