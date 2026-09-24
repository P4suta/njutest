// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest trace summary` and `njutest trace diff`: reading a recording back.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use rust_mutants::id::StoredRunId;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, EXIT_INSUFFICIENT, Environment, TraceCommand};
use crate::trace::{Event, Payload, Problem, check, read_events};

/// The two streams a command writes to, as one argument.
struct Streams<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

/// Reads a recording.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    command: &TraceCommand,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let mut streams = Streams {
        out: stdout,
        err: stderr,
    };
    match command {
        TraceCommand::Summary { run } => summary(root, run.as_deref(), &mut streams),
        TraceCommand::Diff { a, b } => diff(root, a, b, &mut streams),
    }
}

/// What one recording holds and what is wrong with it.
fn summary(root: &Path, named: Option<&str>, streams: &mut Streams<'_>) -> std::io::Result<u8> {
    let Some((run, events)) = load(root, named, streams.err)? else {
        return Ok(EXIT_ERROR);
    };
    super::say(streams.out, &format!("RUN\t{run}"))?;
    super::say(streams.out, &format!("EVENTS\t{}", events.len()))?;
    for (kind, count) in counts(&events) {
        super::say(streams.out, &format!("TYPE\t{kind}\t{count}"))?;
    }
    for (name, duration) in phases(&events) {
        super::say(streams.out, &format!("PHASE\t{name}\t{duration}ms"))?;
    }
    for (program, count) in commands(&events) {
        super::say(streams.out, &format!("COMMAND\t{program}\t{count}"))?;
    }
    for (proof, count) in proofs(&events) {
        super::say(streams.out, &format!("PROOF\t{proof}\t{count}"))?;
    }
    for (duration, command) in slowest(&events) {
        super::say(streams.out, &format!("SLOWEST\t{duration}ms\t{command}"))?;
    }
    engine(root, &run, streams.out)?;

    let problems = check(&events);
    if problems.is_empty() {
        super::say(streams.out, "PROBLEMS\tno problems")?;
        return Ok(EXIT_ASSURED);
    }
    for problem in &problems {
        super::say(streams.out, &format!("PROBLEM\t{}", describe(problem)))?;
    }
    Ok(EXIT_INSUFFICIENT)
}

/// How many executions each proof removed, most first.
#[must_use]
pub fn proofs(events: &[Event]) -> Vec<(String, u64)> {
    let mut counted: BTreeMap<String, u64> = BTreeMap::new();
    for event in events {
        let Payload::Route { route } = &event.payload else {
            continue;
        };
        for discharge in &route.discharged {
            let count = counted.entry(discharge.proof.clone()).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
    let mut ordered: Vec<(String, u64)> = counted.into_iter().collect();
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ordered
}

/// The directory of a run's recording that holds the engine's own, so the name is spelled once.
pub const ENGINE_DIRECTORY: &str = "engine";

/// The directory containing one ordinal namespace per configured build.
pub const BUILDS_DIRECTORY: &str = "builds";

/// The commands that took the longest, most first.
///
/// Every payload is named rather than defaulted away, which is how the measurements got in here: a `_` arm had been dropping every `MutantExec`,
/// so a summary of what made a run long named the builds and not the thing a run spends most of itself doing.
/// A kind that gains a duration later is one the compiler makes somebody place (ADR 0023).
///
/// A `WireExchange` carries a duration and is deliberately not one of these.
/// It is a round trip inside a process this list already counts, so adding it would count the same seconds twice, and five hundred exchanges of twenty milliseconds would fill a list of five with nothing anybody can act on while hiding the measurement that took twelve seconds.
#[must_use]
pub fn slowest(events: &[Event]) -> Vec<(u64, String)> {
    let mut timed: Vec<(u64, String)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::Exec { exec } => Some((exec.duration_ms, said(&exec.argv))),
            Payload::MutantExec { mutant } => Some((
                mutant.duration_ms,
                format!("{} against {}", mutant.mutant, mutant.target),
            )),
            Payload::RunStart { .. }
            | Payload::PhaseStart { .. }
            | Payload::PhaseEnd { .. }
            | Payload::Progress { .. }
            | Payload::Artifact { .. }
            | Payload::Route { .. }
            | Payload::ProbeExec { .. }
            | Payload::WireExchange { .. }
            | Payload::WireExec { .. }
            | Payload::Sentinel { .. }
            | Payload::Model { .. }
            | Payload::Drift { .. }
            | Payload::Control { .. }
            | Payload::Confirm { .. }
            | Payload::Resumed { .. }
            | Payload::Note { .. }
            | Payload::RunEnd { .. } => None,
        })
        .collect();
    timed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    timed.truncate(SLOWEST_KEPT);
    timed
}

/// How many of the slowest commands a summary names.
pub const SLOWEST_KEPT: usize = 5;

/// How wide a rendered command may be before the rest of it is elided.
pub const COMMAND_WIDTH: usize = 72;

/// One command, short enough to read in a line: the program by its file name, then as much of its arguments as fits.
#[must_use]
pub fn said(argv: &[String]) -> String {
    let mut line = match argv.first() {
        Some(path) => match Path::new(path)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
        {
            Some(name) => name.to_owned(),
            None => path.clone(),
        },
        None => String::new(),
    };
    let mut rest = argv.iter().skip(1);
    while line.chars().count() < COMMAND_WIDTH {
        let Some(argument) = rest.next() else {
            break;
        };
        line.push(' ');
        line.push_str(argument);
    }
    if rest.next().is_some() || line.chars().count() > COMMAND_WIDTH {
        line = line.chars().take(COMMAND_WIDTH).collect();
        line.push_str(" …");
    }
    line
}

/// What the engine recorded beside this run, when it recorded anything.
fn engine(root: &Path, run: &StoredRunId, out: &mut dyn Write) -> std::io::Result<()> {
    let builds = runs::recording(root, run).join(BUILDS_DIRECTORY);
    let entries = match std::fs::read_dir(&builds) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            super::say(out, &format!("ENGINE\tunreadable\t{error}"))?;
            return Ok(());
        }
    };
    let mut recordings = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                super::say(out, &format!("ENGINE\tunreadable\t{error}"))?;
                return Ok(());
            }
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            super::say(out, "ENGINE\tunreadable\tnon-UTF-8 build namespace")?;
            return Ok(());
        };
        if name.len() != 10 || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            super::say(
                out,
                &format!("ENGINE\tunreadable\tinvalid build namespace {name:?}"),
            )?;
            return Ok(());
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                super::say(out, &format!("ENGINE\t{name}\tunreadable\t{error}"))?;
                return Ok(());
            }
        };
        if !file_type.is_dir() {
            super::say(out, &format!("ENGINE\t{name}\tunreadable\tnot a directory"))?;
            return Ok(());
        }
        recordings.push((name.to_owned(), entry.path()));
    }
    recordings.sort_by(|left, right| left.0.cmp(&right.0));
    for (ordinal, directory) in recordings {
        let stream = directory
            .join(ENGINE_DIRECTORY)
            .join(rust_mutants::trace::FILE_NAME);
        let file = match std::fs::File::open(&stream) {
            Ok(file) => file,
            Err(error) => {
                super::say(out, &format!("ENGINE\t{ordinal}\tunreadable\t{error}"))?;
                continue;
            }
        };
        let events = match rust_mutants::trace::read_events(std::io::BufReader::new(file)) {
            Ok(events) => events,
            Err(error) => {
                super::say(out, &format!("ENGINE\t{ordinal}\tunreadable\t{error}"))?;
                continue;
            }
        };
        let summary = match rust_mutants::trace::summary::summarize(&events, SLOWEST_KEPT) {
            Ok(summary) => summary,
            Err(error) => {
                super::say(out, &format!("ENGINE\t{ordinal}\tunreadable\t{error}"))?;
                continue;
            }
        };
        for line in rust_mutants::trace::summary::render(&summary).lines() {
            super::say(out, &format!("ENGINE\t{ordinal}\t{line}"))?;
        }
    }
    Ok(())
}

/// What moved between two recordings.
fn diff(root: &Path, a: &str, b: &str, streams: &mut Streams<'_>) -> std::io::Result<u8> {
    let (Some((left, before)), Some((right, after))) = (
        load(root, Some(a), streams.err)?,
        load(root, Some(b), streams.err)?,
    ) else {
        return Ok(EXIT_ERROR);
    };
    super::say(streams.out, &format!("A\t{left}\t{} events", before.len()))?;
    super::say(streams.out, &format!("B\t{right}\t{} events", after.len()))?;

    let (was, is) = (counts(&before), counts(&after));
    for kind in keys(&was, &is) {
        let (from, to) = (
            was.get(&kind).copied().unwrap_or_default(),
            is.get(&kind).copied().unwrap_or_default(),
        );
        if from != to {
            super::say(streams.out, &format!("TYPE\t{kind}\t{from}\t{to}"))?;
        }
    }
    let (before_phases, after_phases) = (phases(&before), phases(&after));
    for name in keys(&before_phases, &after_phases) {
        let (from, to) = (
            before_phases.get(&name).copied().unwrap_or_default(),
            after_phases.get(&name).copied().unwrap_or_default(),
        );
        super::say(
            streams.out,
            &format!("PHASE\t{name}\t{from}ms\t{to}ms\t{}ms", delta(from, to)),
        )?;
    }
    Ok(EXIT_ASSURED)
}

/// The recording of one run, read back.
fn load(
    root: &Path,
    named: Option<&str>,
    stderr: &mut dyn Write,
) -> std::io::Result<Option<(StoredRunId, Vec<Event>)>> {
    let run = match named {
        Some(run) => match StoredRunId::try_from(run) {
            Ok(run) => run,
            Err(error) => {
                super::diagnose(
                    stderr,
                    &format!("{}: {error}", crate::error::RUN_NOT_FOUND.code),
                )?;
                return Ok(None);
            }
        },
        None => match runs::resolve(root, None) {
            Ok(run) => run.id().clone(),
            Err(error) => {
                super::complain(stderr, &error, error.code())?;
                return Ok(None);
            }
        },
    };
    let stream = runs::recording(root, &run).join(crate::trace::FILE_NAME);
    let file = match std::fs::File::open(&stream) {
        Ok(file) => file,
        Err(error) => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: no recording of {run}: {} ({error})",
                    crate::error::RUN_NOT_FOUND.code,
                    stream.display()
                ),
            )?;
            return Ok(None);
        }
    };
    match read_events(std::io::BufReader::new(file)) {
        Ok(events) => Ok(Some((run, events))),
        Err(error) => {
            super::diagnose(
                stderr,
                &format!("{}: {error}", crate::error::RUN_NOT_FOUND.code),
            )?;
            Ok(None)
        }
    }
}

/// How many events of each type.
#[must_use]
pub fn counts(events: &[Event]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for event in events {
        let count = counts
            .entry(event.payload.type_name().to_owned())
            .or_insert(0_u64);
        *count = count.saturating_add(1);
    }
    counts
}

/// How long each phase took, summed over however many times it ran.
#[must_use]
pub fn phases(events: &[Event]) -> BTreeMap<String, u64> {
    let mut durations: BTreeMap<String, u64> = BTreeMap::new();
    for event in events {
        if let Payload::PhaseEnd { phase } = &event.payload {
            let total = durations.entry(phase.name.clone()).or_insert(0);
            *total = total.saturating_add(phase.duration_ms.unwrap_or_default());
        }
    }
    durations
}

/// How many times each program ran, by the name it was started as.
#[must_use]
pub fn commands(events: &[Event]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for event in events {
        if let Payload::Exec { exec } = &event.payload {
            let program = exec.argv.first().map_or("?", String::as_str);
            let name = match Path::new(program)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
            {
                Some(name) => name.to_owned(),
                None => program.to_owned(),
            };
            let count = counts.entry(name).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
    counts
}

/// Every key of both maps, in order.
#[must_use]
pub fn keys(left: &BTreeMap<String, u64>, right: &BTreeMap<String, u64>) -> Vec<String> {
    let mut names: Vec<String> = left.keys().chain(right.keys()).cloned().collect();
    names.sort();
    names.dedup();
    names
}

/// An exact signed difference between two wire-sized counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedDelta {
    direction: Direction,
    magnitude: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Increase,
    Decrease,
}

impl std::fmt::Display for SignedDelta {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.direction {
            Direction::Increase => write!(formatter, "+{}", self.magnitude),
            Direction::Decrease => write!(formatter, "-{}", self.magnitude),
        }
    }
}

/// The exact signed difference, which is what a reader is actually looking at.
///
/// The sign and magnitude stay separate so every pair of `u64` values is representable without overflow, clamping, or a sentinel.
#[must_use]
pub const fn delta(from: u64, to: u64) -> SignedDelta {
    SignedDelta {
        direction: if to >= from {
            Direction::Increase
        } else {
            Direction::Decrease
        },
        magnitude: to.abs_diff(from),
    }
}

/// One problem, in a line.
#[must_use]
pub fn describe(problem: &Problem) -> String {
    match problem {
        Problem::MissingRunStart => {
            "the recording has no run-start: its beginning was lost".to_owned()
        }
        Problem::MissingRunEnd => {
            "the recording has no run-end: the run was killed, or the end was lost".to_owned()
        }
        Problem::SequenceGap { expected, found } => {
            format!("a gap in the sequence: expected {expected}, found {found}")
        }
        Problem::Dropped(count) => format!("the run says it dropped {count} events"),
        Problem::PhaseRepeated { name, times } => format!(
            "the phase {name} began {times} times, so its duration above is the sum of \
             {times} of them and not how long it took"
        ),
    }
}
