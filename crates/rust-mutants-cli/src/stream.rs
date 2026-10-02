// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writing the run as it happens, one JSON object per line, for a program rather than a person.

use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rust_mutants::report::stream::{Line, MutantLine, SCHEMA};
use rust_mutants::run::{Judged, Observer};
use rust_mutants::session::Session;
use rust_mutants::trace::summary::SummaryError;
use rust_mutants::trace::{Event, Payload, PhaseRecord};

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

/// Writes each phase until its producer completes, using the event and completion wakes subscribed to this thread.
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
    watch_waiting(events, stream, working, &|| {
        use rust_mutants::observation::Clock as _;
        rust_mutants::observation::WallClock
            .park(None)
            .map_err(|source| crate::error::CliError::PreparationStartFailed { source })
    })
}

/// Watches the actual preparation producer through a subscription registered before it started.
///
/// # Errors
/// The producer observation, output or measured host wait could not be retained.
pub fn watch_observed<F>(
    events: &Receiver<Event>,
    stream: &mut dyn Write,
    working: &F,
    (observed, recorder): (
        &rust_mutants::observation::Observation,
        &rust_mutants::trace::Recorder,
    ),
) -> Result<(), crate::error::CliError>
where
    F: Fn() -> bool,
{
    watch_waiting(events, stream, working, &|| {
        observed
            .ensure_complete()
            .map_err(|source| crate::error::CliError::PreparationStartFailed { source })?;
        let waited = observed
            .wait(
                "workspace-preparation",
                "phase, cancellation or complete preparation",
                None,
            )
            .map_err(|source| crate::error::CliError::PreparationStartFailed { source })?;
        let detail = serde_json::to_string(&waited.note).map_err(|source| {
            crate::error::CliError::PreparationStartFailed {
                source: std::io::Error::other(source),
            }
        })?;
        recorder.note("host-wait", &detail);
        match waited
            .event
            .map_err(|source| crate::error::CliError::PreparationStartFailed { source })?
        {
            rust_mutants::observation::Event::Changed
            | rust_mutants::observation::Event::Completed
            | rust_mutants::observation::Event::Cancelled
            | rust_mutants::observation::Event::Deadline => Ok(()),
        }
    })?;
    observed
        .ensure_complete()
        .map_err(|source| crate::error::CliError::PreparationStartFailed { source })
}

fn watch_waiting<F>(
    events: &Receiver<Event>,
    stream: &mut dyn Write,
    working: &F,
    wait: &impl Fn() -> Result<(), crate::error::CliError>,
) -> Result<(), crate::error::CliError>
where
    F: Fn() -> bool,
{
    loop {
        match events.try_recv() {
            Ok(event) => {
                if let Some(line) = phase_of(&event)? {
                    say(stream, &line)?;
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if !working() {
                    return Ok(());
                }
                wait()?;
            }
        }
    }
}

/// How long a phase took, which the end of one always carries.
///
/// # Errors
/// [`crate::error::CliError::TraceSummary`] for an end that carries none, which the stream would otherwise have to say took nothing.
fn took(phase: &PhaseRecord) -> Result<u64, crate::error::CliError> {
    match phase.duration_ms {
        Some(took) => Ok(took),
        None => Err(crate::error::CliError::TraceSummary {
            source: SummaryError::MissingPhaseDuration {
                phase: phase.name.clone(),
            },
        }),
    }
}

/// One phase event as a line of the stream, when it is one.
///
/// # Errors
/// [`crate::error::CliError::TraceSummary`] for a phase's end that carries no duration.
fn phase_of(event: &Event) -> Result<Option<Line>, crate::error::CliError> {
    Ok(match &event.payload {
        Payload::PhaseStart { phase } => Some(Line::PhaseStart {
            phase: phase.name.clone(),
        }),
        Payload::PhaseEnd { phase } => Some(Line::PhaseEnd {
            phase: phase.name.clone(),
            duration_ms: took(phase)?,
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
        | Payload::SealedControl { .. }
        | Payload::SealedExec { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => None,
    })
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
                Payload::PhaseEnd { phase } => match took(phase) {
                    Ok(duration_ms) => Line::PhaseEnd {
                        phase: phase.name.clone(),
                        duration_ms,
                    },
                    Err(untimed) => {
                        if self.failure.is_none() {
                            self.failure = Some(untimed);
                        }
                        continue;
                    }
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
                | Payload::SealedControl { .. }
                | Payload::SealedExec { .. }
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
