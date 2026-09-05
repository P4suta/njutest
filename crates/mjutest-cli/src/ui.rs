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
}

impl std::fmt::Debug for Notes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Silent => "Silent",
            Self::Plain(_) => "Plain",
            Self::Jsonl(_) => "Jsonl",
        })
    }
}

impl<'a> Notes<'a> {
    /// The notes of the requested kind, writing to `out`.
    pub fn of(kind: Ui, out: &'a mut dyn Write) -> Self {
        match kind {
            Ui::Plain => Self::Plain(out),
            Ui::Jsonl => Self::Jsonl(out),
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
