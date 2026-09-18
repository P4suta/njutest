// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is happening, which is not what its report says afterwards.

use std::fmt::Write as _;
use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rust_mutants::run::{Judged, NotRunReason, Observer};
use rust_mutants::trace::{Event, Payload};

/// How much a run says while it is happening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Ui {
    /// A terminal gets the overwriting tally, anything else gets the plain lines.
    #[default]
    Auto,
    /// One line per phase and per mutant, and a tally every ten and at the end.
    Plain,
    /// Nothing until the summary.
    Quiet,
}

/// Whether a stream is written in colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Color {
    /// Colour a terminal that has not asked to go without.
    #[default]
    Auto,
    /// Always.
    Always,
    /// Never.
    Never,
}

/// What the stream a command writes to will take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stream {
    /// Whether the environment asked for no colour, which `NO_COLOR` says.
    pub no_color: bool,
    /// Whether the stream is a terminal rather than a file or a pipe.
    pub is_terminal: bool,
}

impl Color {
    /// Whether this setting paints a stream of this shape.
    #[must_use]
    pub const fn paints(self, stream: Stream) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => stream.is_terminal && !stream.no_color,
        }
    }
}

/// `name` in the style this engine draws `outcome` in, painted or not as the stream will take.
///
/// The style is `rust_mutants::telling`'s and so is the escape. This module
/// used to spell three of its own, which is how the same run came to draw a
/// timed-out mutation amber here and green in the dashboard: two modules had
/// each decided what a timeout was worth, and neither knew the other had.
#[must_use]
pub fn paint(outcome: rust_mutants::outcome::Outcome, name: &str, paints: bool) -> String {
    rust_mutants::telling::Style::of(outcome).painted(name, paints)
}

/// How often a plain run writes the tally out, so a log is readable rather than a wall of numbers.
pub const TALLY_EVERY: u32 = 10;

/// The tally of what a run has judged so far.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    killed: u32,
    survived: u32,
    timed_out: u32,
    inconclusive: u32,
    errored: u32,
    not_run: u32,
}

impl Tally {
    /// Puts one judgement in its column.
    ///
    /// Named rather than defaulted: an outcome added later and left to a `_`
    /// arm would be counted as a harness failure, which is a tally telling
    /// somebody their machine is broken about a thing the run established
    /// perfectly well (ADR 0023).
    const fn count(&mut self, judged: &Judged) {
        let slot = match judged.outcome {
            rust_mutants::outcome::Outcome::Killed => &mut self.killed,
            rust_mutants::outcome::Outcome::Survived => &mut self.survived,
            rust_mutants::outcome::Outcome::TimedOut => &mut self.timed_out,
            rust_mutants::outcome::Outcome::Inconclusive => &mut self.inconclusive,
            rust_mutants::outcome::Outcome::NotRun => &mut self.not_run,
            rust_mutants::outcome::Outcome::Errored => &mut self.errored,
        };
        *slot = slot.saturating_add(1);
    }

    fn line(&self, elapsed: Duration, remaining: Option<Duration>) -> String {
        let mut text = format!(
            "          killed {}  survived {}  timed_out {}  inconclusive {}  errored {}  \
             not_run {}   elapsed {}",
            self.killed,
            self.survived,
            self.timed_out,
            self.inconclusive,
            self.errored,
            self.not_run,
            clock(elapsed)
        );
        if let Some(left) = remaining {
            let written = write!(text, "  eta {}", clock(left));
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        text.push('\n');
        text
    }
}

/// A duration as `M:SS`, which is what a reader of a progress line wants.
#[must_use]
pub fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The lines a run prints while it is happening.
#[expect(
    missing_debug_implementations,
    reason = "a display holds a stream, which is a handle to the outside"
)]
pub struct Display<'a> {
    stream: &'a mut dyn Write,
    ui: Ui,
    paints: bool,
    jobs: usize,
    total: u32,
    started: Option<std::time::Instant>,
    tally: Tally,
}

impl<'a> Display<'a> {
    /// A display that writes to `stream`.
    pub fn new(stream: &'a mut dyn Write, ui: Ui, paints: bool, jobs: usize) -> Self {
        Self {
            stream,
            ui,
            paints,
            jobs,
            total: 0,
            started: None,
            tally: Tally::default(),
        }
    }

    fn say(&mut self, text: &str) {
        if self.ui == Ui::Quiet {
            return;
        }
        let _written = self.stream.write_all(text.as_bytes());
        let _flushed = self.stream.flush();
    }

    /// What is left, from what the run has done so far.
    fn eta(&self, completed: u32) -> Option<Duration> {
        let started = self.started?;
        let left = self.total.checked_sub(completed)?;
        if completed == 0 || left == 0 {
            return None;
        }
        let each = started.elapsed().checked_div(completed)?;
        let jobs = u32::try_from(self.jobs.max(1)).unwrap_or(1);
        each.checked_mul(left)?.checked_div(jobs)
    }
}

impl Observer for Display<'_> {
    fn starting(&mut self, total: u32) {
        self.total = total;
        self.started = Some(std::time::Instant::now());
        self.say(&format!(
            "run         {total} mutants on {} jobs\n",
            self.jobs
        ));
    }

    fn judged(&mut self, judged: &Judged, completed: u32, total: u32) {
        self.tally.count(judged);
        let width = total.to_string().len();
        let name = judged.outcome.name();
        let painted = paint(judged.outcome, name, self.paints);
        let padding = " ".repeat(12usize.saturating_sub(name.len()));
        let mut line = format!(
            "[{completed:>width$}/{total}] {} {painted}{padding}",
            judged.display_id,
        );
        if let Some(said) = beside(judged) {
            let written = write!(line, "  {said}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        line.push('\n');
        if completed.is_multiple_of(TALLY_EVERY) {
            let elapsed = self.started.map(|at| at.elapsed()).unwrap_or_default();
            line.push_str(&self.tally.line(elapsed, self.eta(completed)));
        }
        self.say(&line);
    }

    fn finished(&mut self, duration: Duration) {
        let line = self.tally.line(duration, None);
        self.say(&line);
    }
}

/// What a row says after the outcome: the target that reached the verdict, or why no target did.
fn beside(judged: &Judged) -> Option<&str> {
    if !judged.target.is_empty() {
        return Some(&judged.target);
    }
    judged.not_run_reason.map(NotRunReason::name)
}

/// One phase line, from the recording the engine keeps while it prepares.
#[must_use]
pub fn phase_line(event: &Event) -> Option<String> {
    match &event.payload {
        Payload::PhaseStart { phase } => Some(format!("{:<12}{:>28}\n", phase.name, "started")),
        Payload::PhaseEnd { phase } => {
            let milliseconds = phase.duration_ms.unwrap_or_default();
            Some(format!(
                "{:<12}{:>28}\n",
                phase.name,
                format!("{}.{:02}s", milliseconds / 1000, milliseconds % 1000 / 10)
            ))
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
        | Payload::RunEnd { .. } => None,
    }
}

/// One line per phase the recorder has started or finished, in the order it did.
#[must_use]
pub fn phases(events: &Receiver<Event>) -> String {
    let mut text = String::new();
    for event in events.try_iter() {
        if let Some(line) = phase_line(&event) {
            text.push_str(&line);
        }
    }
    text
}

/// How long the display waits for the next thing to say before looking again at whether there will be one.
const LOOKING: Duration = Duration::from_millis(200);

/// Writes each phase as it ends, for as long as `working` says there is work.
pub fn watch(events: &Receiver<Event>, stream: &mut dyn Write, working: &dyn Fn() -> bool) {
    loop {
        match events.recv_timeout(LOOKING) {
            Ok(event) => {
                if let Some(line) = phase_line(&event) {
                    let _written = stream.write_all(line.as_bytes());
                    let _flushed = stream.flush();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if !working() {
                    return;
                }
            }
        }
    }
}
