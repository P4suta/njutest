// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a recording as numbers: where a run went, and what moved between two of them.
//!
//! A trace is diagnostic exhaust and never evidence
//! ([ADR 0002](../../../../docs/adr/0002-trace-is-not-evidence.md)), so nothing
//! here is a claim about a program. What it is for is the question a person
//! asks of a run that took eleven minutes: which part of it did.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{Event, Payload};

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
    pub outcome: String,
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

/// Reads a recording as numbers, keeping the `slowest` longest commands.
#[must_use]
pub fn summarize(events: &[Event], slowest: usize) -> Summary {
    let mut summary = Summary {
        events: u64::try_from(events.len()).unwrap_or(u64::MAX),
        ..Summary::default()
    };
    let mut open: Vec<String> = Vec::new();
    let mut commands: Vec<CommandTiming> = Vec::new();
    for event in events {
        let name = event.payload.type_name();
        let count = summary.counts.entry(name.to_owned()).or_default();
        *count = count.saturating_add(1);
        match &event.payload {
            Payload::PhaseStart { phase } => open.push(phase.name.clone()),
            Payload::PhaseEnd { phase } => {
                let path = path_of(&open, &phase.name);
                if let Some(at) = open.iter().rposition(|name| *name == phase.name) {
                    let _closed = open.remove(at);
                }
                summary.phases.push(PhaseTiming {
                    path,
                    duration_ms: phase.duration_ms.unwrap_or_default(),
                });
            }
            Payload::Exec { exec } => {
                let command = said(&exec.argv);
                let program = program_of(&exec.argv);
                let spent = summary.programs.entry(program.clone()).or_default();
                *spent = spent.saturating_add(exec.duration_ms);
                let started = summary.invocations.entry(program).or_default();
                *started = started.saturating_add(1);
                commands.push(CommandTiming {
                    command,
                    duration_ms: exec.duration_ms,
                });
            }
            Payload::MutantExec { mutant } => {
                let count = summary
                    .executions
                    .entry(mutant.outcome.clone())
                    .or_default();
                *count = count.saturating_add(1);
            }
            Payload::Route { route } => {
                let count = summary.routes.entry(route.granularity.clone()).or_default();
                *count = count.saturating_add(1);
            }
            Payload::ValidateRound { .. } => summary.rounds = summary.rounds.saturating_add(1),
            Payload::Bisect { bisect } => {
                summary.bisections = summary
                    .bisections
                    .saturating_add(u64::from(bisect.attempts));
            }
            Payload::RunEnd { run } => {
                summary.outcome.clone_from(&run.outcome);
                summary.dropped = run.events_dropped;
            }
            _ => {}
        }
    }
    summary.phases.sort_by(|a, b| a.path.cmp(&b.path));
    commands.sort_by(|a, b| {
        b.duration_ms
            .cmp(&a.duration_ms)
            .then_with(|| a.command.cmp(&b.command))
    });
    commands.truncate(slowest);
    summary.slowest = commands;
    summary
}

/// The summary as lines a person reads, one fact per line, tab separated.
#[must_use]
pub fn render(summary: &Summary) -> String {
    let mut out = String::new();
    let _written = writeln!(out, "EVENTS\t{}", summary.events);
    if summary.dropped > 0 {
        let _written = writeln!(out, "DROPPED\t{}", summary.dropped);
    }
    if !summary.outcome.is_empty() {
        let _written = writeln!(out, "OUTCOME\t{}", summary.outcome);
    }
    for (name, count) in &summary.counts {
        let _written = writeln!(out, "TYPE\t{name}\t{count}");
    }
    for phase in &summary.phases {
        let _written = writeln!(out, "PHASE\t{}\t{}ms", phase.path, phase.duration_ms);
    }
    for (program, started) in &summary.invocations {
        let duration = summary.programs.get(program).copied().unwrap_or_default();
        let _written = writeln!(out, "PROGRAM\t{program}\t{started} started\t{duration}ms");
    }
    for command in &summary.slowest {
        let _written = writeln!(
            out,
            "SLOWEST\t{}ms\t{}",
            command.duration_ms, command.command
        );
    }
    for (outcome, count) in &summary.executions {
        let _written = writeln!(out, "EXEC\t{outcome}\t{count}");
    }
    for (granularity, count) in &summary.routes {
        let _written = writeln!(out, "ROUTE\t{granularity}\t{count}");
    }
    if summary.rounds > 0 {
        let _written = writeln!(out, "ROUNDS\t{}", summary.rounds);
    }
    if summary.bisections > 0 {
        let _written = writeln!(out, "BISECTIONS\t{}", summary.bisections);
    }
    out
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
///
/// What a run cost is the same question on every machine, and a summary that
/// counted `cargo` on one and `cargo.exe` on another would answer it twice.
/// Only the platform's own suffix goes: a program genuinely named `build.sh`
/// keeps its name, because that is its name rather than a spelling of one.
fn program_of(argv: &[String]) -> String {
    argv.first().map_or_else(String::new, |first| {
        let name = std::path::Path::new(first)
            .file_name()
            .map_or_else(|| first.clone(), |name| name.to_string_lossy().into_owned());
        let suffix = std::env::consts::EXE_SUFFIX;
        if suffix.is_empty() || name.len() <= suffix.len() {
            return name;
        }
        let (stem, end) = name.split_at(name.len().saturating_sub(suffix.len()));
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
