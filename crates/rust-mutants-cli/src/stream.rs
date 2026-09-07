// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writing the run as it happens, one JSON object per line, for a program rather than a person.

use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rust_mutants::report::stream::{Line, MutantLine, SCHEMA};
use rust_mutants::run::{Judged, Observer};
use rust_mutants::session::Session;
use rust_mutants::trace::{Event, Payload};

/// Writes one line, and says nothing at all when it cannot.
///
/// A stream is what a consumer reads, and a consumer that has closed the pipe
/// is a consumer that has what it wanted. Losing the line it did not wait for
/// is not a reason to fail a run that measured everything it was asked to.
pub fn say(stream: &mut dyn Write, line: &Line) {
    let Ok(text) = serde_json::to_string(line) else {
        return;
    };
    let _written = stream.write_all(text.as_bytes());
    let _ended = stream.write_all(b"\n");
    let _flushed = stream.flush();
}

/// The line that opens the stream, written before anything is prepared.
///
/// A consumer that has to wait for the snapshot, the instrumented build and
/// the validation rounds before it hears anything cannot tell a slow run from
/// a hung one, which is the one thing a stream is for. This says what the run
/// is about at the moment it begins.
pub fn started(
    stream: &mut dyn Write,
    run_id: &str,
    root_name: &str,
    selection: rust_mutants::report::catalog::SelectionDocument,
) {
    say(
        stream,
        &Line::RunStart {
            schema: SCHEMA.to_owned(),
            tool_version: rust_mutants::VERSION.to_owned(),
            run_id: run_id.to_owned(),
            root_name: root_name.to_owned(),
            selection,
        },
    );
}

/// The lines a run writes while it is happening.
#[expect(
    missing_debug_implementations,
    reason = "a writer holds a stream, which is a handle to the outside"
)]
pub struct Writer<'a> {
    stream: &'a mut dyn Write,
    session: &'a Session,
}

impl<'a> Writer<'a> {
    /// A writer that writes what `session` judged to `stream`.
    pub const fn new(stream: &'a mut dyn Write, session: &'a Session) -> Self {
        Self { stream, session }
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
                _ => continue,
            };
            say(self.stream, &line);
        }
    }

    /// The lines that close the stream: every finding, then how it ended.
    pub fn ended(
        &mut self,
        document: &rust_mutants::report::run::RunDocument,
        report: Option<String>,
    ) {
        for finding in &document.findings {
            say(
                self.stream,
                &Line::Finding {
                    finding: finding.clone(),
                },
            );
        }
        say(
            self.stream,
            &Line::RunEnd {
                exit_code: document.run.exit_code,
                interrupted: document.run.interrupted,
                accounting: document.accounting,
                score: document.score,
                report,
            },
        );
    }

    /// The line a failure writes instead of an ending.
    pub fn failed(&mut self, code: &str, message: &str, remedy: Option<&str>) {
        say(
            self.stream,
            &Line::Error {
                code: code.to_owned(),
                message: message.to_owned(),
                remedy: remedy.map(ToOwned::to_owned),
            },
        );
    }
}

impl Observer for Writer<'_> {
    fn judged(&mut self, judged: &Judged, completed: u32, total: u32) {
        let mutant = MutantLine::of(self.session, judged);
        say(
            self.stream,
            &Line::Mutant {
                completed,
                total,
                mutant,
            },
        );
    }

    fn finished(&mut self, _duration: Duration) {}
}
