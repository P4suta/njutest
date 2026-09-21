// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest why`: everything a run's recording says stands behind one claim.

use std::io::Write;
use std::path::Path;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Why as Arguments};
use crate::trace::{Event, read_events};
use rust_mutants::id::StoredRunId;

/// Why an existing recording could not be read completely.
#[derive(Debug, thiserror::Error)]
enum RecordingError {
    /// The trace stream could not be opened.
    #[error("{}: {source}", path.display())]
    Open {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The trace stream did not contain a valid complete event sequence.
    #[error("{}: {source}", path.display())]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: crate::trace::ReadError,
    },
}

/// What a run recorded, or nothing, where nothing means the run kept no recording.
///
/// A file that is not there and a file that cannot be read are two answers.
/// The first is a run that recorded nothing and is the page's own `NotRecorded`; the second is a recording this command failed to read, and reporting that as "the run kept no recording" would tell somebody a fact about their run that is really a fact about this command (ADR 0023).
///
/// # Errors
/// The recording exists and could not be read.
fn recording(root: &Path, run: &StoredRunId) -> Result<Option<Vec<Event>>, RecordingError> {
    let stream = runs::recording(root, run).join(crate::trace::FILE_NAME);
    match std::fs::File::open(&stream) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(RecordingError::Open {
            path: stream,
            source,
        }),
        Ok(file) => read_events(std::io::BufReader::new(file))
            .map(Some)
            .map_err(|source| RecordingError::Read {
                path: stream,
                source,
            }),
    }
}

/// Says what a recording holds behind one claim.
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
    let events = match recording(root, run.id()) {
        Ok(events) => events,
        Err(error) => {
            super::diagnose(
                stderr,
                &format!("{}: {error}", crate::error::RUN_NOT_FOUND.code),
            )?;
            return Ok(EXIT_ERROR);
        }
    };
    let claim = arguments.claim.asked();
    let why = crate::why::why(&claim, events.as_deref());
    let page = crate::presentation::why::page(&claim, &why, environment.terminal);
    stdout.write_all(page.as_bytes())?;
    Ok(EXIT_ASSURED)
}
