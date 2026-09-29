// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest merge`: the report the whole catalog would have written, from the reports of its parts.

use std::io::Write;
use std::path::Path;

use crate::cli::{EXIT_ERROR, Environment, Merge as Arguments};
use crate::report::merge::merge;
use crate::report::{ReportDocument, ShardReport};

/// Why one report part could not be read as a report.
#[derive(Debug, thiserror::Error)]
enum ReadError {
    /// The named path could not be read.
    #[error("{}: {source}", path.display())]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The named path is not a report document.
    #[error("{}: {source}", path.display())]
    Json {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// A complete report was offered where a shard document is required.
    #[error("{}: this is already a complete report, not a shard document", path.display())]
    Complete { path: std::path::PathBuf },
}

/// Combines the parts and writes the whole, where `--rerun` asks, only once every sealed execution it rests on came out the same on this machine.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let mut parts = Vec::new();
    for path in &arguments.reports {
        match read(path) {
            Ok(part) => parts.push(part),
            Err(error) => {
                super::diagnose(stderr, &error.to_string())?;
                return Ok(EXIT_ERROR);
            }
        }
    }
    let final_run_id = match crate::run_id::mint(jiff::Timestamp::now(), std::process::id()) {
        Ok(run_id) => run_id,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let latticed = match merge(&final_run_id, &parts) {
        Ok(latticed) => latticed,
        Err(refused) => {
            super::diagnose(stderr, &refused.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let whole = match latticed.complete_without_models() {
        Ok(whole) => whole,
        Err(refused) => {
            super::diagnose(stderr, &refused.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    if arguments.rerun && !reproduced(arguments, environment, &whole, stderr)? {
        return Ok(EXIT_ERROR);
    }
    let verdict = whole.verdict();
    let document = match crate::report::json::document_any(&ReportDocument::Complete(whole)) {
        Ok(document) => document,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    if let Some(path) = &arguments.output {
        if let Err(error) = std::fs::write(path, &document) {
            super::diagnose(stderr, &format!("{}: {error}", path.display()))?;
            return Ok(EXIT_ERROR);
        }
    } else {
        super::say(stdout, document.trim_end())?;
    }
    Ok(verdict.exit_code())
}

/// Whether every sealed execution `whole` rests on came to what its part recorded, run again in the workspace the arguments name; says which did not, or what stopped them, where one did not.
///
/// # Errors
/// Returns the diagnostic stream's write failure.
fn reproduced(
    arguments: &Arguments,
    environment: &Environment,
    whole: &crate::report::Report,
    stderr: &mut dyn Write,
) -> std::io::Result<bool> {
    let root = environment.rooted(arguments.directory.as_deref());
    let config = match crate::config::Config::load(&root) {
        Ok(config) => config,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(false);
        }
    };
    let run_id = match crate::run_id::mint(jiff::Timestamp::now(), std::process::id()) {
        Ok(run_id) => run_id,
        Err(error) => {
            super::diagnose(stderr, &error.to_string())?;
            return Ok(false);
        }
    };
    let request = crate::assure::rerun::asking(
        &root,
        config,
        (
            crate::build::Cargo {
                offline: arguments.offline,
                locked: arguments.locked,
            },
            run_id,
            jiff::Timestamp::now(),
        ),
    );
    let trace = crate::trace::Recorder::disabled();
    let watch = crate::watch::Watch::new(&environment.cancel, &trace);
    match crate::assure::rerun::rerun(whole, &request, environment, watch) {
        Ok(crate::assure::rerun::Rerun::NothingSealed) => {
            super::diagnose(
                stderr,
                "the merged report rests on no sealed execution, so nothing was run again",
            )?;
            Ok(true)
        }
        Ok(crate::assure::rerun::Rerun::Reproduced) => {
            super::diagnose(
                stderr,
                "every sealed execution the merged report rests on came out the same on this \
                 machine",
            )?;
            Ok(true)
        }
        Ok(crate::assure::rerun::Rerun::Unreproduced(error)) => {
            super::complain(stderr, &error, error.code())?;
            Ok(false)
        }
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            Ok(false)
        }
    }
}

/// One part, read from the file a person named.
fn read(path: &Path) -> Result<ShardReport, ReadError> {
    let text = std::fs::read_to_string(path).map_err(|source| ReadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let document: ReportDocument =
        crate::strictjson::decode_str(&text).map_err(|source| ReadError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    match document {
        ReportDocument::Shard(report) => Ok(report),
        ReportDocument::Complete(_) => Err(ReadError::Complete {
            path: path.to_path_buf(),
        }),
    }
}
