// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a recording as numbers: where a run went, and what moved between two of them.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use super::{Event, Payload};

/// Why a recording could not be summarized exactly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SummaryError {
    /// A collection length does not fit the trace wire's u64 counters.
    #[error("the {field} count does not fit u64")]
    CountOutsideWire {
        /// The counter that could not represent the exact value.
        field: &'static str,
    },
    /// Adding one event or attempt overflowed a u64 counter.
    #[error("the {field} counter overflowed u64")]
    CounterOverflow {
        /// The counter that overflowed.
        field: &'static str,
    },
    /// Summing process durations overflowed the trace wire's u64 milliseconds.
    #[error("the accumulated duration for {program:?} overflowed u64 milliseconds")]
    DurationOverflow {
        /// The program whose exact total no longer fit.
        program: String,
    },
    /// A phase-end event omitted the duration it is required to carry.
    #[error("phase {phase:?} ended without a duration")]
    MissingPhaseDuration {
        /// The phase whose end was incomplete.
        phase: String,
    },
}

/// How long one phase took, by the path a reader would follow to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseTiming {
    /// The phase's place in the nesting, as `prepare/validate`.
    pub path: String,
    /// How long it took.
    pub duration_ms: u64,
}

/// One command a run started, and what it cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTiming {
    /// The command line, shortened to the program and its arguments.
    pub command: String,
    /// How long it ran.
    pub duration_ms: u64,
}

/// What one change between two summaries moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// What moved: an event type, an outcome, or a route granularity.
    pub what: String,
    /// What it was.
    pub from: u64,
    /// What it is.
    pub to: u64,
}

/// What a recording adds up to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Summary {
    /// How many events it holds.
    pub events: u64,
    /// How many the run admits it lost.
    pub dropped: u64,
    /// How the run ended, in its own words.
    pub outcome: Option<super::RunOutcome>,
    /// How many events of each type.
    pub counts: BTreeMap<String, u64>,
    /// Every phase, in the order they began, with what each took.
    pub phases: Vec<PhaseTiming>,
    /// How long every command of one program took together.
    pub programs: BTreeMap<String, u64>,
    /// How many times each program was started, which is what a run costs regardless of the machine it ran on.
    pub invocations: BTreeMap<String, u64>,
    /// The slowest commands, longest first.
    pub slowest: Vec<CommandTiming>,
    /// How many mutant executions ended each way.
    pub executions: BTreeMap<String, u64>,
    /// How many routes were decided at each granularity.
    pub routes: BTreeMap<String, u64>,
    /// How many validation rounds the run took.
    pub rounds: u64,
    /// How many compilations isolation cost.
    pub bisections: u64,
}

/// Folds one event into the numbers, which is every kind of event a recording holds.
///
/// Every payload the summary does not count is named rather than swept up,
/// because a kind nobody counted reads exactly like a kind that happened nought times.
/// Naming them makes the next one somebody adds a question the compiler asks here.
fn counted(
    summary: &mut Summary,
    open: &mut Vec<String>,
    commands: &mut Vec<CommandTiming>,
    event: &Event,
) -> Result<(), SummaryError> {
    match &event.payload {
        Payload::PhaseStart { phase } => open.push(phase.name.clone()),
        Payload::PhaseEnd { phase } => {
            let path = path_of(open, &phase.name);
            if let Some(at) = open.iter().rposition(|name| *name == phase.name) {
                let closed = open.remove(at);
                debug_assert_eq!(closed, phase.name);
            }
            summary.phases.push(PhaseTiming {
                path,
                duration_ms: phase.duration_ms.ok_or_else(|| {
                    SummaryError::MissingPhaseDuration {
                        phase: phase.name.clone(),
                    }
                })?,
            });
        }
        Payload::Exec { exec } => count_exec(summary, commands, exec)?,
        Payload::MutantExec { mutant } => {
            let count = summary
                .executions
                .entry(mutant.outcome.clone())
                .or_default();
            *count = count.checked_add(1).ok_or(SummaryError::CounterOverflow {
                field: "mutant executions",
            })?;
        }
        Payload::Route { route } => {
            let count = summary
                .routes
                .entry(route.granularity.name().to_owned())
                .or_default();
            *count = count
                .checked_add(1)
                .ok_or(SummaryError::CounterOverflow { field: "routes" })?;
        }
        Payload::ValidateRound { .. } => {
            summary.rounds =
                summary
                    .rounds
                    .checked_add(1)
                    .ok_or(SummaryError::CounterOverflow {
                        field: "validation rounds",
                    })?;
        }
        Payload::Bisect { bisect } => {
            summary.bisections = summary
                .bisections
                .checked_add(u64::from(bisect.attempts))
                .ok_or(SummaryError::CounterOverflow {
                    field: "bisection attempts",
                })?;
        }
        Payload::RunEnd { run } => {
            summary.outcome = Some(run.outcome);
            summary.dropped = run.events_dropped;
        }
        Payload::RunStart { .. }
        | Payload::Open { .. }
        | Payload::Snapshot { .. }
        | Payload::DiscoverFile { .. }
        | Payload::Instrument { .. }
        | Payload::Build { .. }
        | Payload::Verify { .. }
        | Payload::Touch { .. }
        | Payload::Witness { .. }
        | Payload::SkipClaim { .. }
        | Payload::Kept { .. }
        | Payload::Cache { .. }
        | Payload::Select { .. }
        | Payload::Identical { .. }
        | Payload::Evidence { .. }
        | Payload::Note { .. } => {}
    }
    Ok(())
}

fn count_exec(
    summary: &mut Summary,
    commands: &mut Vec<CommandTiming>,
    exec: &super::ExecRecord,
) -> Result<(), SummaryError> {
    let command = said(&exec.argv);
    let program = program_of(&exec.argv);
    let spent = summary.programs.entry(program.clone()).or_default();
    *spent = spent
        .checked_add(exec.duration_ms)
        .ok_or_else(|| SummaryError::DurationOverflow {
            program: program.clone(),
        })?;
    let started = summary.invocations.entry(program).or_default();
    *started = started
        .checked_add(1)
        .ok_or(SummaryError::CounterOverflow {
            field: "program invocations",
        })?;
    commands.push(CommandTiming {
        command,
        duration_ms: exec.duration_ms,
    });
    Ok(())
}

/// Reads a recording as numbers, keeping the `slowest` longest commands.
///
/// # Errors
/// A count or duration cannot be represented exactly by the trace wire.
pub fn summarize(events: &[Event], slowest: usize) -> Result<Summary, SummaryError> {
    let event_count = u64::try_from(events.len())
        .map_err(|_outside_wire| SummaryError::CountOutsideWire { field: "events" })?;
    let mut summary = Summary {
        events: event_count,
        ..Summary::default()
    };
    let mut open: Vec<String> = Vec::new();
    let mut commands: Vec<CommandTiming> = Vec::new();
    for event in events {
        let name = event.payload.type_name();
        let count = summary.counts.entry(name.to_owned()).or_default();
        *count = count.checked_add(1).ok_or(SummaryError::CounterOverflow {
            field: "event types",
        })?;
        counted(&mut summary, &mut open, &mut commands, event)?;
    }
    summary.phases.sort_by(|a, b| a.path.cmp(&b.path));
    commands.sort_by(|a, b| {
        b.duration_ms
            .cmp(&a.duration_ms)
            .then_with(|| a.command.cmp(&b.command))
    });
    commands.truncate(slowest);
    summary.slowest = commands;
    Ok(summary)
}

/// The summary as lines a person reads, one fact per line, tab separated.
#[must_use]
pub fn render(summary: &Summary) -> String {
    let mut out = String::new();
    line(&mut out, format_args!("EVENTS\t{}", summary.events));
    if summary.dropped > 0 {
        line(&mut out, format_args!("DROPPED\t{}", summary.dropped));
    }
    if let Some(outcome) = summary.outcome {
        line(&mut out, format_args!("OUTCOME\t{}", outcome.name()));
    }
    for (name, count) in &summary.counts {
        line(&mut out, format_args!("TYPE\t{name}\t{count}"));
    }
    for phase in &summary.phases {
        line(
            &mut out,
            format_args!("PHASE\t{}\t{}ms", phase.path, phase.duration_ms),
        );
    }
    for (program, started) in &summary.invocations {
        let duration = summary.programs.get(program).copied().unwrap_or_default();
        line(
            &mut out,
            format_args!("PROGRAM\t{program}\t{started} started\t{duration}ms"),
        );
    }
    for command in &summary.slowest {
        line(
            &mut out,
            format_args!("SLOWEST\t{}ms\t{}", command.duration_ms, command.command),
        );
    }
    for (outcome, count) in &summary.executions {
        line(&mut out, format_args!("EXEC\t{outcome}\t{count}"));
    }
    for (granularity, count) in &summary.routes {
        line(&mut out, format_args!("ROUTE\t{granularity}\t{count}"));
    }
    if summary.rounds > 0 {
        line(&mut out, format_args!("ROUNDS\t{}", summary.rounds));
    }
    if summary.bisections > 0 {
        line(&mut out, format_args!("BISECTIONS\t{}", summary.bisections));
    }
    out
}

fn line(out: &mut String, arguments: fmt::Arguments<'_>) {
    let written = out.write_fmt(arguments).and_then(|()| out.write_char('\n'));
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
}

/// What moved between two recordings: every count that is not the same in both, in name order.
#[must_use]
pub fn diff(before: &Summary, after: &Summary) -> Vec<Change> {
    let mut changes = Vec::new();
    for (prefix, was, is) in [
        ("", &before.counts, &after.counts),
        ("exec ", &before.executions, &after.executions),
        ("route ", &before.routes, &after.routes),
    ] {
        let mut names: Vec<&String> = was.keys().chain(is.keys()).collect();
        names.sort_unstable();
        names.dedup();
        for name in names {
            let (from, to) = (
                was.get(name).copied().unwrap_or_default(),
                is.get(name).copied().unwrap_or_default(),
            );
            if from != to {
                changes.push(Change {
                    what: format!("{prefix}{name}"),
                    from,
                    to,
                });
            }
        }
    }
    for (what, from, to) in [
        ("events", before.events, after.events),
        ("rounds", before.rounds, after.rounds),
        ("bisections", before.bisections, after.bisections),
    ] {
        if from != to {
            changes.push(Change {
                what: what.to_owned(),
                from,
                to,
            });
        }
    }
    changes
}

/// The path a phase sits at, ending in its own name.
fn path_of(open: &[String], name: &str) -> String {
    let mut path: Vec<&str> = open.iter().map(String::as_str).collect();
    if path.last() != Some(&name) {
        path.push(name);
    }
    path.join("/")
}

/// The program a command line starts with, by file name, without the suffix a platform puts on an executable.
fn program_of(argv: &[String]) -> String {
    argv.first().map_or_else(String::new, |first| {
        let name = match std::path::Path::new(first)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
        {
            Some(name) => name.to_owned(),
            None => first.clone(),
        };
        let suffix = std::env::consts::EXE_SUFFIX;
        if suffix.is_empty() || name.len() <= suffix.len() {
            return name;
        }
        let Some(stem_length) = name.len().checked_sub(suffix.len()) else {
            return name;
        };
        let (stem, end) = name.split_at(stem_length);
        if end.eq_ignore_ascii_case(suffix) {
            stem.to_owned()
        } else {
            name
        }
    })
}

/// The command line as a reader would quote it: the program by name, then its arguments.
fn said(argv: &[String]) -> String {
    let mut parts = vec![program_of(argv)];
    parts.extend(argv.iter().skip(1).cloned());
    parts.join(" ")
}
