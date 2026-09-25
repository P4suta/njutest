// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writing the run as it happens, one JSON object per line, for a program rather than a person.

use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rust_mutants::report::stream::{Line, MutantLine, SCHEMA};
use rust_mutants::run::{Judged, Observer};
use rust_mutants::session::Session;
use rust_mutants::trace::{Event, Payload};

/// Writes one complete line or returns the exact encoding or output failure.
///
/// # Errors
/// Returns the exact serialization or output-stream failure.
pub fn say(stream: &mut dyn Write, line: &Line) -> Result<(), crate::error::CliError> {
    let mut text = serde_json::to_string(line)
        .map_err(|source| crate::error::CliError::OutputEncodingFailed { source })?;
    text.push('\n');
    crate::app::write(stream, &text)
}

/// The line that opens the stream, written before anything is prepared.
///
/// # Errors
/// Returns the exact serialization or output-stream failure.
pub fn started(
    stream: &mut dyn Write,
    run_id: &str,
    root_name: &str,
    selection: rust_mutants::report::catalog::SelectionDocument,
) -> Result<(), crate::error::CliError> {
    say(
        stream,
        &Line::RunStart {
            schema: SCHEMA.to_owned(),
            tool_version: rust_mutants::VERSION.to_owned(),
            run_id: run_id.to_owned(),
            root_name: root_name.to_owned(),
            selection,
        },
    )
}

/// The line a failure writes instead of an ending.
pub(crate) fn failed(
    stream: &mut dyn Write,
    error: &crate::error::CliError,
) -> Result<(), crate::error::CliError> {
    let code = error.code();
    say(
        stream,
        &Line::Error {
            code: code.code.to_owned(),
            message: error.to_string(),
            remedy: code.remedy.map(ToOwned::to_owned),
        },
    )
}

/// Closes a successful run stream after every fallible postcondition has succeeded: every finding, then exactly one terminal line.
pub(crate) fn ended(
    stream: &mut dyn Write,
    document: &rust_mutants::report::run::RunDocument,
    report: Option<String>,
) -> Result<(), crate::error::CliError> {
    for finding in &document.findings {
        say(
            stream,
            &Line::Finding {
                finding: finding.clone(),
            },
        )?;
    }
    say(
        stream,
        &Line::RunEnd {
            exit_code: document.run.exit_code,
            interrupted: document.run.interrupted,
            accounting: document.accounting,
            score: document.score,
            report,
        },
    )
}

/// How long the writer waits for the next line before looking again at whether there will be one.
const LOOKING: Duration = Duration::from_millis(200);

/// Writes each phase as it ends, for as long as `working` says there is work.
///
/// # Errors
/// Returns the first output failure; no later event is claimed to have been written.
pub fn watch<F>(
    events: &Receiver<Event>,
    stream: &mut dyn Write,
    working: &F,
) -> Result<(), crate::error::CliError>
where
    F: Fn() -> bool,
{
    loop {
        match events.recv_timeout(LOOKING) {
            Ok(event) => {
                if let Some(line) = phase_of(&event) {
                    say(stream, &line)?;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if !working() {
                    return Ok(());
                }
            }
        }
    }
}

/// One phase event as a line of the stream, when it is one.
fn phase_of(event: &Event) -> Option<Line> {
    match &event.payload {
        Payload::PhaseStart { phase } => Some(Line::PhaseStart {
            phase: phase.name.clone(),
        }),
        Payload::PhaseEnd { phase } => Some(Line::PhaseEnd {
            phase: phase.name.clone(),
            duration_ms: phase.duration_ms.unwrap_or_default(),
        }),
        Payload::RunStart { .. }
        | Payload::Open { .. }
        | Payload::Snapshot { .. }
        | Payload::Exec { .. }
        | Payload::DiscoverFile { .. }
        | Payload::Instrument { .. }
        | Payload::ValidateRound { .. }
        | Payload::Bisect { .. }
        | Payload::Build { .. }
        | Payload::Verify { .. }
        | Payload::Touch { .. }
        | Payload::PerturbedControl { .. }
        | Payload::Witness { .. }
        | Payload::SkipClaim { .. }
        | Payload::Kept { .. }
        | Payload::Route { .. }
        | Payload::Cache { .. }
        | Payload::Select { .. }
        | Payload::Identical { .. }
        | Payload::Evidence { .. }
        | Payload::MutantExec { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => None,
    }
}

/// The lines a run writes while it is happening.
pub struct Writer<'a> {
    stream: &'a mut dyn Write,
    session: &'a Session,
    failure: Option<crate::error::CliError>,
}

impl std::fmt::Debug for Writer<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Writer").finish_non_exhaustive()
    }
}

impl<'a> Writer<'a> {
    /// A writer that writes what `session` judged to `stream`.
    pub const fn new(stream: &'a mut dyn Write, session: &'a Session) -> Self {
        Self {
            stream,
            session,
            failure: None,
        }
    }

    /// Returns the first output failure observed by an infallible observer callback.
    ///
    /// # Errors
    /// Returns the first output failure retained by the observer.
    pub fn finish(self) -> Result<(), crate::error::CliError> {
        match self.failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn record(&mut self, line: &Line) {
        if self.failure.is_some() {
            return;
        }
        if let Err(error) = say(self.stream, line) {
            self.failure = Some(error);
        }
    }

    /// One phase line for each phase the recorder has finished.
    pub fn phases(&mut self, events: &Receiver<Event>) {
        for event in events.try_iter() {
            let line = match &event.payload {
                Payload::PhaseStart { phase } => Line::PhaseStart {
                    phase: phase.name.clone(),
                },
                Payload::PhaseEnd { phase } => Line::PhaseEnd {
                    phase: phase.name.clone(),
                    duration_ms: phase.duration_ms.unwrap_or_default(),
                },
                Payload::RunStart { .. }
                | Payload::Open { .. }
                | Payload::Snapshot { .. }
                | Payload::Exec { .. }
                | Payload::DiscoverFile { .. }
                | Payload::Instrument { .. }
                | Payload::ValidateRound { .. }
                | Payload::Bisect { .. }
                | Payload::Build { .. }
                | Payload::Verify { .. }
                | Payload::Touch { .. }
                | Payload::PerturbedControl { .. }
                | Payload::Witness { .. }
                | Payload::SkipClaim { .. }
                | Payload::Kept { .. }
                | Payload::Route { .. }
                | Payload::Cache { .. }
                | Payload::Select { .. }
                | Payload::Identical { .. }
                | Payload::Evidence { .. }
                | Payload::MutantExec { .. }
                | Payload::Note { .. }
                | Payload::RunEnd { .. } => continue,
            };
            self.record(&line);
        }
    }
}

impl Observer for Writer<'_> {
    fn judged(&mut self, judged: &Judged, completed: u32, total: u32) {
        let mutant = match MutantLine::of(self.session, judged) {
            Ok(mutant) => mutant,
            Err(source) => {
                if self.failure.is_none() {
                    self.failure = Some(rust_mutants::EngineError::from(source).into());
                }
                return;
            }
        };
        self.record(&Line::Mutant {
            completed,
            total,
            mutant,
        });
    }

    fn finished(&mut self, _duration: Duration) {}
}
