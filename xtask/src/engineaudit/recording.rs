// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report held to the recording of what the run actually did.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{
    Audit, INCONCLUSIVE, Layer, NOT_RUN, Notes, Report, Row, STOPPED_EARLY, TIMED_OUT, UNREACHED,
    UNSELECTED, array, number, numbers, string, strings,
};

/// The recording, against the report it is supposed to be the exhaust of.
pub(super) fn trace(report: &Report, recorded: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Trace);
    let Some(recorded) = recorded else {
        notes.unaudited(
            "recording",
            "the run kept no recording, so what it did cannot be held to what it reported"
                .to_owned(),
        );
        return;
    };
    let events = events(recorded);
    complete(&events, &mut notes);
    instrumented(&events, &mut notes);
    verified(&events, &mut notes);
    condemned(report, &events, &mut notes);
    routed(report, recorded, &mut notes);
}

/// Whether the recording begins, ends, lost nothing, and closed every phase it opened.
fn complete(events: &[Value], notes: &mut Notes<'_>) {
    let kind = |event: &Value| string(event, "type").unwrap_or_default();
    match events.first() {
        None => {
            notes.unaudited("recording", "the recording holds no event".to_owned());
            return;
        }
        Some(first) if kind(first) != "run-start" => notes.violated(
            "run-start",
            "the recording does not begin with run-start; its beginning was lost".to_owned(),
        ),
        Some(_) => {}
    }
    let mut expected = events
        .first()
        .and_then(|first| number(first, "seq"))
        .unwrap_or(1);
    for event in events {
        let seq = number(event, "seq").unwrap_or_default();
        if seq != expected {
            notes.violated(
                "seq",
                format!("sequence {expected} is missing and the next event is {seq}; the sink lost what was between"),
            );
        }
        expected = seq.saturating_add(1);
    }
    let mut open: Vec<String> = Vec::new();
    for event in events {
        match kind(event).as_str() {
            "phase-start" => open.push(phase_name(event)),
            "phase-end" => {
                let name = phase_name(event);
                if let Some(at) = open.iter().rposition(|held| *held == name) {
                    let _closed = open.remove(at);
                } else {
                    let said = format!("the phase {name} ended without beginning");
                    notes.violated("phase", said);
                }
            }
            _ => {}
        }
    }
    for name in open {
        notes.violated("phase", format!("the phase {name} began and never ended"));
    }
    match events.last() {
        Some(last) if kind(last) == "run-end" => {
            let dropped = last
                .get("run")
                .and_then(|run| number(run, "events_dropped"))
                .unwrap_or_default();
            if dropped > 0 {
                notes.violated(
                    "run-end",
                    format!(
                        "the run admits it dropped {dropped} events; what it did is not all here"
                    ),
                );
            }
        }
        _ => notes.violated(
            "run-end",
            "the recording does not end with run-end; the run was killed, or the end was lost"
                .to_owned(),
        ),
    }
}

/// Every instrumentation, held to the one thing it must not change.
fn instrumented(events: &[Value], notes: &mut Notes<'_>) {
    for event in events {
        if string(event, "type").as_deref() != Some("instrument") {
            continue;
        }
        let Some(record) = event.get("instrument") else {
            continue;
        };
        let (before, after) = (
            number(record, "lines_before"),
            number(record, "lines_after"),
        );
        let path = string(record, "path").unwrap_or_default();
        match (before, after) {
            (Some(before), Some(after)) if before != after => notes.violated(
                &path,
                format!(
                    "instrumenting moved the file from {before} lines to {after}; a position \
                     the report names would be a position in a file nobody has"
                ),
            ),
            (None, _) | (_, None) => notes.unaudited(
                &path,
                "the record omits the line counts, so whether instrumenting moved a line \
                 cannot be re-derived"
                    .to_owned(),
            ),
            _ => {}
        }
    }
}

/// Every target the build produced, against the verification of it.
fn verified(events: &[Value], notes: &mut Notes<'_>) {
    let mut built: BTreeSet<String> = BTreeSet::new();
    let mut verified: BTreeSet<String> = BTreeSet::new();
    let mut skipped: BTreeSet<String> = BTreeSet::new();
    for event in events {
        match string(event, "type").as_deref() {
            Some("build") => {
                if let Some(record) = event.get("build") {
                    built.extend(strings(record, "targets"));
                    for detail in array(record, "details") {
                        if strings(detail, "limitations")
                            .iter()
                            .any(|one| one == "target-skipped-by-configuration")
                            && let Some(target) = string(detail, "id")
                        {
                            skipped.insert(target);
                        }
                    }
                }
            }
            Some("verify") => {
                if let Some(record) = event.get("verify")
                    && let Some(target) = string(record, "target")
                {
                    verified.insert(target);
                }
            }
            _ => {}
        }
    }
    if verified.is_empty() {
        notes.unaudited(
            "verify",
            "nothing was verified, so what the suite says with nothing active is not on record"
                .to_owned(),
        );
        return;
    }
    for target in built.difference(&verified) {
        if skipped.contains(target) {
            continue;
        }
        notes.violated(
            target,
            "the build produced this target and nothing verified it; an outcome from a target \
             whose baseline nobody read is not about the mutation"
                .to_owned(),
        );
    }
}

/// The refusals, against the rounds that condemned them.
fn condemned(report: &Report, events: &[Value], notes: &mut Notes<'_>) {
    let mut named: BTreeSet<u64> = BTreeSet::new();
    let mut rounds = 0usize;
    for event in events {
        match string(event, "type").as_deref() {
            Some("validate-round") => {
                rounds = rounds.saturating_add(1);
                if let Some(record) = event.get("round") {
                    for one in array(record, "attributed") {
                        if let Some(index) = number(one, "index") {
                            named.insert(index);
                        }
                    }
                }
            }
            Some("bisect") => {
                if let Some(record) = event.get("bisect") {
                    named.extend(numbers(record, "offenders"));
                }
            }
            _ => {}
        }
    }
    if rounds == 0 {
        if !report.rejections.is_empty() {
            notes.unaudited(
                "rejections",
                "the recording holds no validation round and the report holds refusals, so \
                 what condemned each candidate cannot be re-derived"
                    .to_owned(),
            );
        }
        return;
    }
    let refused: BTreeSet<u64> = report
        .rejections
        .iter()
        .filter_map(|one| one.index)
        .collect();
    if refused.len() != report.rejections.len() {
        notes.unaudited(
            "rejections",
            "a refusal carries no catalog index, so what condemned it cannot be re-derived"
                .to_owned(),
        );
        return;
    }
    for index in named.difference(&refused) {
        notes.violated(
            &index.to_string(),
            "a validation round condemned this candidate and the report does not refuse it; a \
             candidate the compiler refused is one the catalog does not hold"
                .to_owned(),
        );
    }
    for index in refused.difference(&named) {
        let subject = report
            .rejections
            .iter()
            .find(|one| one.index == Some(*index))
            .map_or_else(|| index.to_string(), |one| one.display_id.clone());
        notes.violated(
            &subject,
            "the report refuses this candidate and no round condemned it; a refusal nothing \
             accounts for is a mutant somebody dropped"
                .to_owned(),
        );
    }
}

/// Every row against the route and the executions the recording holds for it.
fn routed(report: &Report, recorded: &str, notes: &mut Notes<'_>) {
    let routing = crate::route::read(recorded);
    if routing.routes.is_empty() && routing.execs.is_empty() {
        notes.unaudited(
            "route",
            "the recording holds no routing decision and no execution, so nothing holds a row \
             to what it did"
                .to_owned(),
        );
        return;
    }
    let reused = report
        .mutants
        .iter()
        .filter(|row| row.source_run_id.is_some())
        .count();
    if reused > 0 {
        notes.unaudited(
            "reuse",
            format!(
                "{reused} rows were read back from an earlier run, so this recording says \
                 nothing about how they were decided"
            ),
        );
    }
    let stopped = report
        .mutants
        .iter()
        .filter(|row| row.outcome == NOT_RUN && report.interrupted == Some(true))
        .count();
    if stopped > 0 {
        notes.unaudited(
            "interrupted",
            format!(
                "{stopped} rows were never reached because the run was stopped, so there is \
                 nothing about them to hold the recording to"
            ),
        );
    }
    for row in &report.mutants {
        if row.source_run_id.is_some()
            || (row.outcome == NOT_RUN && report.interrupted == Some(true))
            || row.not_run(UNSELECTED)
            || row.not_run(STOPPED_EARLY)
        {
            continue;
        }
        let Some(route) = routing.route_of(&row.id, &row.display_id) else {
            notes.violated(
                row.label(),
                "the report judges this mutant and the recording holds no route for it; a \
                 decision nobody recorded is one nobody can check"
                    .to_owned(),
            );
            continue;
        };
        let execs: Vec<&crate::route::Exec> = routing.execs_for(&row.id, &row.display_id).collect();
        answered(row, &execs, notes);
        reached(row, route, &execs, notes);
        discharged(row, route, &execs, notes);
    }
}

/// The row's own answer, against the execution of the target it names.
fn answered(row: &Row, execs: &[&crate::route::Exec], notes: &mut Notes<'_>) {
    if execs.is_empty() {
        return;
    }
    let Some(answer) = execs.iter().rev().find(|exec| exec.target == row.target) else {
        notes.violated(
            row.label(),
            format!(
                "the row says {} answered and the recording holds no execution against it; a \
                 report that names a target its own recording did not run is not evidence",
                row.target
            ),
        );
        return;
    };
    if answer.outcome != row.outcome {
        notes.violated(
            row.label(),
            format!(
                "the row says {} and its execution against {} says {}; a report that disagrees \
                 with its own recording is not evidence",
                row.outcome, row.target, answer.outcome
            ),
        );
    }
    if let (Some(recorded), Some(reported)) = (answer.tests_run, row.tests_run)
        && recorded != reported
    {
        notes.violated(
            row.label(),
            format!("the row says {reported} tests ran and the execution says {recorded}"),
        );
    }
    retried(row, execs, notes);
}

/// A timeout the run believed, against the retry that is what believing one takes.
fn retried(row: &Row, execs: &[&crate::route::Exec], notes: &mut Notes<'_>) {
    let timed_out = execs
        .iter()
        .filter(|exec| exec.outcome == TIMED_OUT)
        .count();
    if row.outcome == TIMED_OUT && (timed_out < 2 || !row.retried) {
        notes.violated(
            row.label(),
            format!(
                "the row is a timeout and the recording holds {timed_out} of them with \
                 retried={}; a timeout is believed only after it repeats on its own",
                row.retried
            ),
        );
    }
    if row.retried && timed_out == 0 {
        notes.violated(
            row.label(),
            "the row says it was retried and nothing timed out; a retry is what a timeout \
             costs and nothing else asks for one"
                .to_owned(),
        );
    }
    if row.outcome == INCONCLUSIVE && row.retried && timed_out < 1 {
        notes.violated(
            row.label(),
            "the row could not be decided after a retry and nothing timed out; inconclusive \
             after a retry is what a timeout that did not repeat leaves behind"
                .to_owned(),
        );
    }
}

/// The route's granularity, against whether anything ran.
fn reached(
    row: &Row,
    route: &crate::route::Route,
    execs: &[&crate::route::Exec],
    notes: &mut Notes<'_>,
) {
    match (route.granularity.as_str(), execs.is_empty()) {
        (UNREACHED, false) => notes.violated(
            row.label(),
            "the route says no measured target reaches this mutation and the recording then \
             runs one against it; a claim its own run contradicts is not a claim"
                .to_owned(),
        ),
        ("all" | "block", true) if row.outcome != NOT_RUN => notes.violated(
            row.label(),
            format!(
                "the route says {} targets could notice this mutation and nothing ran; an \
                 outcome of {} rests on an execution the recording does not hold",
                route.reaching.len(),
                row.outcome
            ),
        ),
        _ => {}
    }
    if row.unreached && route.granularity != UNREACHED {
        notes.violated(
            row.label(),
            format!(
                "the row says nothing reaches this mutation and the route says {}",
                route.granularity
            ),
        );
    }
}

/// Every target a proof removed, against the executions of the mutant it was removed from.
fn discharged(
    row: &Row,
    route: &crate::route::Route,
    execs: &[&crate::route::Exec],
    notes: &mut Notes<'_>,
) {
    for exec in execs {
        if route.discharges(&exec.target) {
            notes.violated(
                row.label(),
                format!(
                    "a proof removed {} from what could notice this mutation and it then ran \
                     against it; a layer that removes work is not a layer that also does it",
                    exec.target
                ),
            );
        }
    }
}

/// The ledger of accepted survivors, against the run that was asked to hold to it. The work the report claims, against the routes it claims it from and the recording of what ran.
pub(super) fn work(report: &Report, recorded: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Work);
    let targets = report.targets.len();
    if targets == 0 {
        notes.unaudited(
            "work",
            "the report names no targets, so what a whole run would have started cannot be \
             re-derived"
                .to_owned(),
        );
        return;
    }
    let built: BTreeSet<&str> = report.targets.iter().map(String::as_str).collect();
    let mut started: u64 = 0;
    for row in &report.mutants {
        pairs_of(row, &built, targets, &mut notes);
        if row.source_run_id.is_none() {
            started = started
                .saturating_add(u64::try_from(row.executed.len()).unwrap_or(u64::MAX))
                .saturating_add(u64::from(row.retried));
        }
    }
    let Some(recorded) = recorded else {
        notes.unaudited(
            "executions",
            "no recording was given, so what the report says it started cannot be held to what \
             ran"
            .to_owned(),
        );
        return;
    };
    let events = events(recorded);
    let ran = events
        .iter()
        .filter(|event| string(event, "type").as_deref() == Some("mutant-exec"))
        .count();
    let ran = u64::try_from(ran).unwrap_or(u64::MAX);
    if ran != started {
        notes.violated(
            "executions",
            format!(
                "the report accounts for {started} processes and the recording holds {ran}; a \
                 report that undercounts its own work is one nobody can hold to doing less"
            ),
        );
    }
}

/// Whether one row's pairs are ones the run built targets for and its own route reached.
fn pairs_of(row: &Row, built: &BTreeSet<&str>, targets: usize, notes: &mut Notes<'_>) {
    let reaching: BTreeSet<&str> = row.reaching.iter().map(String::as_str).collect();
    let discharged: BTreeSet<&str> = row.discharged.iter().map(|(id, _)| id.as_str()).collect();
    for name in row.executed.iter().map(String::as_str) {
        if !reaching.contains(name) {
            notes.violated(
                row.label(),
                format!(
                    "a process was started against {name}, which the route this row carries never \
                     reached; work nothing routed is work nobody asked for"
                ),
            );
        }
    }
    for name in reaching.union(&discharged) {
        if !built.contains(name) {
            notes.violated(
                row.label(),
                format!("the route names {name}, which is not a target the run built"),
            );
        }
    }
    if !reaching.is_disjoint(&discharged) {
        notes.violated(
            row.label(),
            "a target is both one the route reached and one a proof removed; it is one or the \
             other"
                .to_owned(),
        );
    }
    if reaching.len().saturating_add(discharged.len()) > targets {
        notes.violated(
            row.label(),
            format!(
                "the route accounts for {} targets and the run built {targets}",
                reaching.len().saturating_add(discharged.len())
            ),
        );
    }
    if row.source_run_id.is_some() && !row.executed.is_empty() {
        notes.violated(
            row.label(),
            "an earlier run established this and a process was started for it anyway".to_owned(),
        );
    }
}

/// Every event of a recording, skipping what is not one.
pub(super) fn events(recorded: &str) -> Vec<Value> {
    recorded
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

/// The name of the phase one boundary is about.
fn phase_name(event: &Value) -> String {
    event
        .get("phase")
        .and_then(|phase| string(phase, "name"))
        .unwrap_or_default()
}
