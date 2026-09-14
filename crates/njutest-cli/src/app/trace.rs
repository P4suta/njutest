// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest trace summary` and `njutest trace diff`: reading a recording back.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, EXIT_INSUFFICIENT, Environment, TraceCommand};
use crate::trace::{Event, Payload, Problem, check, read_events};

/// The two streams a command writes to, as one argument.
struct Streams<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

/// Reads a recording.
pub fn run(
    command: &TraceCommand,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
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
fn summary(root: &Path, named: Option<&str>, streams: &mut Streams<'_>) -> u8 {
    let Some((run, events)) = load(root, named, streams.err) else {
        return EXIT_ERROR;
    };
    super::say(streams.out, &format!("RUN\t{run}"));
    super::say(streams.out, &format!("EVENTS\t{}", events.len()));
    for (kind, count) in counts(&events) {
        super::say(streams.out, &format!("TYPE\t{kind}\t{count}"));
    }
    for (name, duration) in phases(&events) {
        super::say(streams.out, &format!("PHASE\t{name}\t{duration}ms"));
    }
    for (program, count) in commands(&events) {
        super::say(streams.out, &format!("COMMAND\t{program}\t{count}"));
    }
    for (proof, count) in proofs(&events) {
        super::say(streams.out, &format!("PROOF\t{proof}\t{count}"));
    }
    for (duration, command) in slowest(&events) {
        super::say(streams.out, &format!("SLOWEST\t{duration}ms\t{command}"));
    }
    engine(root, &run, streams.out);

    let problems = check(&events);
    if problems.is_empty() {
        super::say(streams.out, "PROBLEMS\tno problems");
        return EXIT_ASSURED;
    }
    for problem in &problems {
        super::say(streams.out, &format!("PROBLEM\t{}", describe(problem)));
    }
    EXIT_INSUFFICIENT
}

/// How many executions each proof removed, most first.
///
/// This is the answer to the question
/// [ADR 0004](../../../../docs/adr/0004-proof-layers-not-budgets.md) says a
/// reader will ask when a run goes faster: which proof did it. It is a count of
/// discharges, never of seconds, because a layer that removes an execution is
/// the only thing in this program allowed to make a run shorter.
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

/// The commands that took the longest, most first.
///
/// A run is mostly the time its subprocesses take, and the question a person
/// asks of a slow run is which of them it was. Answering it used to mean
/// writing a script over the recording.
#[must_use]
pub fn slowest(events: &[Event]) -> Vec<(u64, String)> {
    let mut timed: Vec<(u64, String)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::Exec { exec } => Some((exec.duration_ms, said(&exec.argv))),
            _ => None,
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
///
/// A line that was cut says so. Cutting one and leaving it looking whole hands
/// a reader a command they can neither run nor recognise, and the ones they
/// could run look exactly the same.
#[must_use]
pub fn said(argv: &[String]) -> String {
    let mut line = argv
        .first()
        .and_then(|path| Path::new(path).file_name())
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
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
///
/// The engine does most of a run — the snapshot, the instrumentation, the
/// validation rounds, the builds — and keeps its own recording in a directory
/// of its own. A summary that read only the runner's would leave the larger
/// part of every run unaccounted for.
fn engine(root: &Path, run: &str, out: &mut dyn Write) {
    let stream = runs::recording(root, run)
        .join(ENGINE_DIRECTORY)
        .join(rust_mutants::trace::FILE_NAME);
    let Ok(file) = std::fs::File::open(&stream) else {
        return;
    };
    let Ok(events) = rust_mutants::trace::read_events(std::io::BufReader::new(file)) else {
        super::say(out, "ENGINE\tunreadable");
        return;
    };
    let summary = rust_mutants::trace::summary::summarize(&events, SLOWEST_KEPT);
    for line in rust_mutants::trace::summary::render(&summary).lines() {
        super::say(out, &format!("ENGINE\t{line}"));
    }
}

/// What moved between two recordings.
fn diff(root: &Path, a: &str, b: &str, streams: &mut Streams<'_>) -> u8 {
    let (Some((left, before)), Some((right, after))) = (
        load(root, Some(a), streams.err),
        load(root, Some(b), streams.err),
    ) else {
        return EXIT_ERROR;
    };
    super::say(streams.out, &format!("A\t{left}\t{} events", before.len()));
    super::say(streams.out, &format!("B\t{right}\t{} events", after.len()));

    let (was, is) = (counts(&before), counts(&after));
    for kind in keys(&was, &is) {
        let (from, to) = (
            was.get(&kind).copied().unwrap_or_default(),
            is.get(&kind).copied().unwrap_or_default(),
        );
        if from != to {
            super::say(streams.out, &format!("TYPE\t{kind}\t{from}\t{to}"));
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
            &format!("PHASE\t{name}\t{from}ms\t{to}ms\t{:+}ms", delta(from, to)),
        );
    }
    EXIT_ASSURED
}

/// The recording of one run, read back.
fn load(root: &Path, named: Option<&str>, stderr: &mut dyn Write) -> Option<(String, Vec<Event>)> {
    let run = match named {
        Some(run) => run.to_owned(),
        None => match runs::resolve(root, None) {
            Ok(run) => run,
            Err(error) => {
                super::diagnose(stderr, &error.to_string());
                return None;
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
            );
            return None;
        }
    };
    match read_events(std::io::BufReader::new(file)) {
        Ok(events) => Some((run, events)),
        Err(error) => {
            super::diagnose(
                stderr,
                &format!("{}: {error}", crate::error::RUN_NOT_FOUND.code),
            );
            None
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
            let name = Path::new(program).file_name().map_or_else(
                || program.to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
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

/// The signed difference, which is what a reader is actually looking at.
#[must_use]
pub fn delta(from: u64, to: u64) -> i64 {
    i64::try_from(to)
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::try_from(from).unwrap_or(i64::MAX))
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
