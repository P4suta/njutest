// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest trace summary` and `mjutest trace diff`: reading a recording back.

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
fn counts(events: &[Event]) -> BTreeMap<String, u64> {
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
fn phases(events: &[Event]) -> BTreeMap<String, u64> {
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
fn commands(events: &[Event]) -> BTreeMap<String, u64> {
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
fn keys(left: &BTreeMap<String, u64>, right: &BTreeMap<String, u64>) -> Vec<String> {
    let mut names: Vec<String> = left.keys().chain(right.keys()).cloned().collect();
    names.sort();
    names.dedup();
    names
}

/// The signed difference, which is what a reader is actually looking at.
fn delta(from: u64, to: u64) -> i64 {
    i64::try_from(to)
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::try_from(from).unwrap_or(i64::MAX))
}

/// One problem, in a line.
fn describe(problem: &Problem) -> String {
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
    }
}
