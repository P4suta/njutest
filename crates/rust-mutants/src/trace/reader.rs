// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a recording back, and saying what is wrong with it.

use std::io::{self, BufRead};

use serde::Deserialize as _;

use super::event::{Event, Payload};

/// Why a stream could not be read.
/// Fail-closed: a malformed line is an error naming the line, never an event skipped in silence.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// The stream could not be read.
    #[error("reading the trace stream: {source}")]
    Io {
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// A line is not one event.
    #[error("trace stream line {line}: {source}")]
    Malformed {
        /// The 1-based line.
        line: usize,
        /// The parse failure.
        #[source]
        source: serde_json::Error,
    },
}

impl ReadError {
    /// The 1-based line the error is about, or 0 for an I/O failure.
    #[must_use]
    pub const fn line(&self) -> usize {
        match self {
            Self::Io { .. } => 0,
            Self::Malformed { line, .. } => *line,
        }
    }
}

/// Reads every event of a JSON Lines stream, in stream order.
/// Blank lines are skipped; anything else that is not one event is an error.
///
/// # Errors
/// See [`ReadError`].
pub fn read_events(reader: impl BufRead) -> Result<Vec<Event>, ReadError> {
    let mut events = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|source| ReadError::Io { source })?;
        if line.trim().is_empty() {
            continue;
        }
        let value = crate::strictjson::from_str(&line).map_err(|source| ReadError::Malformed {
            line: index.saturating_add(1),
            source,
        })?;
        let event = Event::deserialize(value).map_err(|source| ReadError::Malformed {
            line: index.saturating_add(1),
            source,
        })?;
        events.push(event);
    }
    Ok(events)
}

/// One thing wrong with a recording as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The first event is not `run-start`: the beginning was lost.
    MissingRunStart,
    /// The last event is not `run-end`: the run was killed, or the end was lost.
    MissingRunEnd,
    /// A sequence number was skipped: the sink lost the events between.
    SequenceGap {
        /// The sequence number expected next.
        expected: u64,
        /// The one found.
        found: u64,
    },
    /// The recording's own accounting says events were dropped.
    Dropped(u64),
    /// A phase began and never ended, or ended without beginning: the run was killed inside it, or the boundary was lost.
    UnbalancedPhase {
        /// The phase's name.
        name: String,
    },
}

/// Says what is wrong with a recording: a missing start or end, sequence gaps, and the drops the run-end admits to.
/// Empty for a complete recording.
#[must_use]
pub fn check(events: &[Event]) -> Vec<Problem> {
    let mut problems = Vec::new();
    if !matches!(
        events.first().map(|e| &e.payload),
        Some(Payload::RunStart { .. })
    ) {
        problems.push(Problem::MissingRunStart);
    }
    let mut expected = events.first().map_or(1, |e| e.seq);
    for event in events {
        if event.seq != expected {
            problems.push(Problem::SequenceGap {
                expected,
                found: event.seq,
            });
        }
        expected = event.seq.saturating_add(1);
    }
    problems.extend(unbalanced_phases(events));
    match events.last().map(|e| &e.payload) {
        Some(Payload::RunEnd { run }) => {
            if run.events_dropped > 0 {
                problems.push(Problem::Dropped(run.events_dropped));
            }
        }
        _ => problems.push(Problem::MissingRunEnd),
    }
    problems
}

/// Every phase whose beginning and end do not pair up, in the order they began.
fn unbalanced_phases(events: &[Event]) -> Vec<Problem> {
    let mut open: Vec<&str> = Vec::new();
    let mut problems = Vec::new();
    for event in events {
        match &event.payload {
            Payload::PhaseStart { phase } => open.push(&phase.name),
            Payload::PhaseEnd { phase } => {
                match open.iter().rposition(|name| *name == phase.name) {
                    Some(at) => {
                        let closed = open.remove(at);
                        debug_assert_eq!(closed, phase.name);
                    }
                    None => problems.push(Problem::UnbalancedPhase {
                        name: phase.name.clone(),
                    }),
                }
            }
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
            | Payload::RunEnd { .. } => {}
        }
    }
    problems.extend(open.into_iter().map(|name| Problem::UnbalancedPhase {
        name: name.to_owned(),
    }));
    problems
}
