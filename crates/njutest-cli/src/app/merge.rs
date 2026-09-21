// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest merge`: the report the whole catalog would have written, from the reports of its parts.

use std::io::Write;
use std::path::Path;

use crate::cli::{EXIT_ERROR, Merge as Arguments};
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

/// Combines the parts and writes the whole.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
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
