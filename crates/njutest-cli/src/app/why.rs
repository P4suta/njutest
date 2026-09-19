// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest why`: everything a run's recording says stands behind one claim.

use std::io::Write;
use std::path::Path;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Why as Arguments};
use crate::trace::{Event, read_events};

/// What a run recorded, or nothing, where nothing means the run kept no recording.
///
/// A file that is not there and a file that cannot be read are two answers.
/// The first is a run that recorded nothing and is the page's own
/// `NotRecorded`; the second is a recording this command failed to read, and
/// reporting that as "the run kept no recording" would tell somebody a fact
/// about their run that is really a fact about this command (ADR 0023).
///
/// # Errors
/// The recording exists and could not be read.
fn recording(root: &Path, run: &str) -> Result<Option<Vec<Event>>, String> {
    let stream = runs::recording(root, run).join(crate::trace::FILE_NAME);
    match std::fs::File::open(&stream) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", stream.display())),
        Ok(file) => read_events(std::io::BufReader::new(file))
            .map(Some)
            .map_err(|error| format!("{}: {error}", stream.display())),
    }
}

/// Says what a recording holds behind one claim.
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
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };
    let events = match recording(root, &run) {
        Ok(events) => events,
        Err(message) => {
            super::diagnose(
                stderr,
                &format!("{}: {message}", crate::error::RUN_NOT_FOUND.code),
            );
            return EXIT_ERROR;
        }
    };
    let claim = arguments.claim.asked();
    let why = crate::why::why(&claim, events.as_deref());
    let page = crate::presentation::why::page(&claim, &why, environment.terminal);
    let _written = stdout.write_all(page.as_bytes());
    EXIT_ASSURED
}
