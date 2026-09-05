// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is running.
//!
//! Progress goes to the error stream and the report goes to the output
//! stream, so `mjutest verify > report.lines` is a report and not a report
//! with a progress log mixed into it.
//!
//! Everything written here is escaped the way a report record is
//! ([`crate::report::lines::escape`]): the messages come from test binaries
//! and providers, and a newline inside one would put a line on the stream
//! that this program never wrote.

use std::io::Write;

use crate::cli::Ui;
use crate::report::lines::escape;

/// Where a run says what it is doing.
///
/// A run calls these unconditionally; [`Silent`] is what a caller that wants
/// nothing passes, so there is no branch at the call site.
pub trait Notes {
    /// A phase began.
    fn phase(&mut self, name: &str);
    /// One step of a phase finished.
    fn progress(&mut self, message: &str, done: u64, total: u64);
    /// Something worth saying that is not progress.
    fn note(&mut self, kind: &str, text: &str);
}

/// Says nothing at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct Silent;

impl Notes for Silent {
    fn phase(&mut self, _name: &str) {}
    fn progress(&mut self, _message: &str, _done: u64, _total: u64) {}
    fn note(&mut self, _kind: &str, _text: &str) {}
}

/// Lines a person reads.
pub struct Plain<'a> {
    out: &'a mut dyn Write,
}

impl std::fmt::Debug for Plain<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Plain")
    }
}

impl<'a> Plain<'a> {
    /// Writes to `out`.
    pub fn new(out: &'a mut dyn Write) -> Self {
        Self { out }
    }
}

impl Notes for Plain<'_> {
    fn phase(&mut self, name: &str) {
        say(self.out, &format!("== {}", escape(name)));
    }

    fn progress(&mut self, message: &str, done: u64, total: u64) {
        say(
            self.out,
            &format!("   [{done}/{total}] {}", escape(message)),
        );
    }

    fn note(&mut self, kind: &str, text: &str) {
        say(self.out, &format!("   {}: {}", escape(kind), escape(text)));
    }
}

/// One JSON object per line, for a program.
pub struct Jsonl<'a> {
    out: &'a mut dyn Write,
}

impl std::fmt::Debug for Jsonl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Jsonl")
    }
}

impl<'a> Jsonl<'a> {
    /// Writes to `out`.
    pub fn new(out: &'a mut dyn Write) -> Self {
        Self { out }
    }

    fn emit(&mut self, value: &serde_json::Value) {
        say(self.out, &value.to_string());
    }
}

impl Notes for Jsonl<'_> {
    fn phase(&mut self, name: &str) {
        self.emit(&serde_json::json!({ "type": "phase", "name": name }));
    }

    fn progress(&mut self, message: &str, done: u64, total: u64) {
        self.emit(&serde_json::json!({
            "type": "progress",
            "message": message,
            "done": done,
            "total": total,
        }));
    }

    fn note(&mut self, kind: &str, text: &str) {
        self.emit(&serde_json::json!({ "type": "note", "kind": kind, "detail": text }));
    }
}

/// The notes of the requested kind, writing to `out`.
#[must_use]
pub fn notes(kind: Ui, out: &mut dyn Write) -> Box<dyn Notes + '_> {
    match kind {
        Ui::Plain => Box::new(Plain::new(out)),
        Ui::Jsonl => Box::new(Jsonl::new(out)),
    }
}

/// Writes one line. A closed stream is the reader's choice, not a failure of
/// ours, and never a reason to stop a verification.
fn say(out: &mut dyn Write, line: &str) {
    let _written = writeln!(out, "{line}");
}
