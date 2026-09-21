// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is running.

use std::io::Write;

use crate::cli::Ui;
use crate::report::lines::escape;

/// Where a run says what it is doing.
pub enum Notes<'a> {
    /// Says nothing at all.
    #[cfg(feature = "testkit")]
    Silent,
    /// Lines a person reads.
    Plain(&'a mut dyn Write),
    /// One JSON object per line, for a program.
    Jsonl(&'a mut dyn Write),
    /// One block that says where the run is, rewritten in place.
    /// What a reader wants from a run under way is where it is, not everywhere it has been.
    Dashboard(Dashboard<'a>),
}

/// The one line a dashboard keeps, and what it last said.
#[non_exhaustive]
pub struct Dashboard<'a> {
    /// Where the block goes.
    pub out: &'a mut dyn Write,
    /// The phase the run is in.
    pub phase: String,
    /// How wide the last block was, so the next one covers all of it.
    pub width: usize,
}

impl std::fmt::Debug for Dashboard<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Dashboard")
            .field("phase", &self.phase)
            .field("width", &self.width)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Notes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            #[cfg(feature = "testkit")]
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
    ///
    /// # Errors
    /// Returns the output stream's write failure.
    pub fn finish(&mut self) -> std::io::Result<()> {
        match self {
            Self::Dashboard(dashboard) => dashboard.end(),
            #[cfg(feature = "testkit")]
            Self::Silent => Ok(()),
            Self::Plain(_) | Self::Jsonl(_) => Ok(()),
        }
    }

    /// A phase began.
    ///
    /// # Errors
    /// Returns the output stream's write failure.
    pub fn phase(&mut self, name: &str) -> std::io::Result<()> {
        match self {
            #[cfg(feature = "testkit")]
            Self::Silent => Ok(()),
            Self::Plain(out) => say(*out, &format!("== {}", escape(name))),
            Self::Jsonl(out) => emit(*out, &serde_json::json!({ "type": "phase", "name": name })),
            Self::Dashboard(dashboard) => {
                dashboard.phase = escape(name);
                dashboard.redraw("")
            }
        }
    }

    /// One step of a phase finished.
    ///
    /// # Errors
    /// Returns the output stream's write failure.
    pub fn progress(&mut self, message: &str, done: u64, total: u64) -> std::io::Result<()> {
        match self {
            #[cfg(feature = "testkit")]
            Self::Silent => Ok(()),
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
                dashboard.redraw(&format!("{done}/{total} {}", escape(message)))
            }
        }
    }

    /// Something worth saying that is not progress.
    ///
    /// # Errors
    /// Returns the output stream's write failure.
    pub fn note(&mut self, kind: &str, text: &str) -> std::io::Result<()> {
        match self {
            #[cfg(feature = "testkit")]
            Self::Silent => Ok(()),
            Self::Plain(out) => say(*out, &format!("   {}: {}", escape(kind), escape(text))),
            Self::Jsonl(out) => emit(
                *out,
                &serde_json::json!({ "type": "note", "kind": kind, "detail": text }),
            ),
            Self::Dashboard(dashboard) => {
                dashboard.aside(&format!("{}: {}", escape(kind), escape(text)))
            }
        }
    }
}

impl Dashboard<'_> {
    /// Rewrites the block where it already wrote, padded to cover whatever was longer before it.
    fn redraw(&mut self, detail: &str) -> std::io::Result<()> {
        let line = if detail.is_empty() {
            format!("{} ...", self.phase)
        } else {
            format!("{} {detail}", self.phase)
        };
        let padding = self.width.saturating_sub(line.chars().count());
        write!(self.out, "\r{line}{: <padding$}\r", "")?;
        self.out.flush()?;
        self.width = line.chars().count();
        Ok(())
    }

    /// Says something the progress line never will, on a line of its own, and puts the block back under it.
    fn aside(&mut self, text: &str) -> std::io::Result<()> {
        let padding = self.width;
        writeln!(self.out, "\r{text}{: <padding$}", "")?;
        self.width = 0;
        self.redraw("")
    }

    /// Ends the block so the next thing written starts at the left margin.
    fn end(&mut self) -> std::io::Result<()> {
        if self.width > 0 {
            writeln!(self.out)?;
            self.width = 0;
        }
        Ok(())
    }
}

/// Writes one JSON object.
fn emit(out: &mut dyn Write, value: &serde_json::Value) -> std::io::Result<()> {
    say(out, &value.to_string())
}

/// Writes one line without deciding whether a broken pipe is success; only the composition root knows whether the output stream was the command's result.
fn say(out: &mut dyn Write, line: &str) -> std::io::Result<()> {
    writeln!(out, "{line}")
}
