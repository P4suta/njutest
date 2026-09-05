// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is running.

use std::io::Write;

use crate::cli::Ui;
use crate::report::lines::escape;

/// Where a run says what it is doing.
#[non_exhaustive]
pub enum Notes<'a> {
    /// Says nothing at all.
    Silent,
    /// Lines a person reads.
    Plain(&'a mut dyn Write),
    /// One JSON object per line, for a program.
    Jsonl(&'a mut dyn Write),
    /// One block that says where the run is, rewritten in place. What a reader wants from a run under way is where it is, not everywhere it has been.
    Dashboard(Dashboard<'a>),
}

/// The one line a dashboard keeps, and what it last said.
#[expect(
    missing_debug_implementations,
    reason = "a stream is a handle to the outside; there is nothing to print about one"
)]
#[non_exhaustive]
pub struct Dashboard<'a> {
    /// Where the block goes.
    pub out: &'a mut dyn Write,
    /// The phase the run is in.
    pub phase: String,
    /// How wide the last block was, so the next one covers all of it.
    pub width: usize,
}

impl std::fmt::Debug for Notes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Silent => "Silent",
            Self::Plain(_) => "Plain",
            Self::Jsonl(_) => "Jsonl",
            Self::Dashboard(_) => "Dashboard",
        })
    }
}

impl<'a> Notes<'a> {
    /// The notes of the requested kind, writing to `out`.
    pub fn of(kind: Ui, out: &'a mut dyn Write) -> Self {
        match kind {
            Ui::Plain => Self::Plain(out),
            Ui::Jsonl => Self::Jsonl(out),
            Ui::Dashboard => Self::Dashboard(Dashboard {
                out,
                phase: String::new(),
                width: 0,
            }),
        }
    }

    /// The run is over: the line after a dashboard starts at the left margin, and every other interface has nothing to add.
    pub fn finish(&mut self) {
        if let Self::Dashboard(dashboard) = self {
            dashboard.end();
        }
    }

    /// A phase began.
    pub fn phase(&mut self, name: &str) {
        match self {
            Self::Silent => {}
            Self::Plain(out) => say(*out, &format!("== {}", escape(name))),
            Self::Jsonl(out) => {
                emit(*out, &serde_json::json!({ "type": "phase", "name": name }));
            }
            Self::Dashboard(dashboard) => {
                dashboard.phase = escape(name);
                dashboard.redraw("");
            }
        }
    }

    /// One step of a phase finished.
    pub fn progress(&mut self, message: &str, done: u64, total: u64) {
        match self {
            Self::Silent => {}
            Self::Plain(out) => say(*out, &format!("   [{done}/{total}] {}", escape(message))),
            Self::Jsonl(out) => emit(
                *out,
                &serde_json::json!({
                    "type": "progress",
                    "message": message,
                    "done": done,
                    "total": total,
                }),
            ),
            Self::Dashboard(dashboard) => {
                dashboard.redraw(&format!("{done}/{total} {}", escape(message)));
            }
        }
    }

    /// Something worth saying that is not progress.
    pub fn note(&mut self, kind: &str, text: &str) {
        match self {
            Self::Silent => {}
            Self::Plain(out) => say(*out, &format!("   {}: {}", escape(kind), escape(text))),
            Self::Jsonl(out) => emit(
                *out,
                &serde_json::json!({ "type": "note", "kind": kind, "detail": text }),
            ),
            Self::Dashboard(dashboard) => {
                dashboard.aside(&format!("{}: {}", escape(kind), escape(text)));
            }
        }
    }
}

impl Dashboard<'_> {
    /// Rewrites the block where it already wrote, padded to cover whatever was longer before it.
    fn redraw(&mut self, detail: &str) {
        let line = if detail.is_empty() {
            format!("{} ...", self.phase)
        } else {
            format!("{} {detail}", self.phase)
        };
        let padding = self.width.saturating_sub(line.chars().count());
        let _written = write!(self.out, "\r{line}{: <padding$}\r", "");
        let _flushed = self.out.flush();
        self.width = line.chars().count();
    }

    /// Says something the progress line never will, on a line of its own, and puts the block back under it.
    fn aside(&mut self, text: &str) {
        let padding = self.width;
        let _written = writeln!(self.out, "\r{text}{: <padding$}", "");
        self.width = 0;
        self.redraw("");
    }

    /// Ends the block so the next thing written starts at the left margin.
    fn end(&mut self) {
        if self.width > 0 {
            let _written = writeln!(self.out);
            self.width = 0;
        }
    }
}

/// Writes one JSON object.
fn emit(out: &mut dyn Write, value: &serde_json::Value) {
    say(out, &value.to_string());
}

/// Writes one line. A closed stream is the reader's choice, not a failure of ours, and never a reason to stop a verification.
fn say(out: &mut dyn Write, line: &str) {
    let _written = writeln!(out, "{line}");
}
