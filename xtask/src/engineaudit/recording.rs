// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report held to the recording of what the run actually did.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{
    Audit, CheckedRecording, DECLINED, Decided, INCONCLUSIVE, KILLED, Layer, NOT_RUN, Notes,
    Report, Row, STOPPED_EARLY, StepNotice, UNREACHED, UNSELECTED, WAITED, array, count, number,
    numbers, string, strings,
};

/// The recording, against the report it is supposed to be the exhaust of.
pub(super) fn trace(
    report: &Report,
    recorded: Option<&CheckedRecording>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Trace);
    let Some(recorded) = recorded else {
        notes.unaudited(
            "recording",
            "the run kept no recording, so what it did cannot be held to what it reported"
                .to_owned(),
        );
        return notes.looked();
    };
    complete(&recorded.events, &mut notes);
    instrumented(&recorded.events, &mut notes);
    verified(&recorded.events, &mut notes);
    condemned(report, &recorded.events, &mut notes);
    routed(report, recorded, &mut notes);
    notes.looked()
}

/// Whether the recording begins, ends, lost nothing, and closed every phase it opened.
fn complete(events: &[Value], notes: &mut Notes<'_>) {
    match events.first() {
        None => {
            notes.unaudited("recording", "the recording holds no event".to_owned());
            return;
        }
        Some(first) if string(first, "type").as_deref() != Some("run-start") => notes.violated(
            "run-start",
            "the recording does not begin with run-start; its beginning was lost".to_owned(),
        ),
        Some(_) => {}
    }
    if sequence(events, notes) == Sequence::Incomplete {
        return;
    }
    phases(events, notes);
    ending(events, notes);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sequence {
    Complete,
    Incomplete,
}

fn sequence(events: &[Value], notes: &mut Notes<'_>) -> Sequence {
    let Some(mut expected) = events.first().and_then(|first| number(first, "seq")) else {
        notes.violated("seq", "the first event carries no sequence".to_owned());
        return Sequence::Incomplete;
    };
    for event in events {
        let Some(seq) = number(event, "seq") else {
            notes.violated("seq", "an event carries no sequence".to_owned());
            return Sequence::Incomplete;
        };
        if seq != expected {
            notes.violated(
                "seq",
                format!("sequence {expected} is missing and the next event is {seq}; the sink lost what was between"),
            );
        }
        let Some(next) = seq.checked_add(1) else {
            notes.violated(
                "seq",
                "the event sequence exhausted the trace contract's integer width".to_owned(),
            );
            return Sequence::Incomplete;
        };
        expected = next;
    }
    Sequence::Complete
}

fn phases(events: &[Value], notes: &mut Notes<'_>) {
    let mut open: Vec<String> = Vec::new();
    for event in events {
        match string(event, "type").as_deref() {
            Some("phase-start") => open.push(phase_name(event)),
            Some("phase-end") => {
                let name = phase_name(event);
                if let Some(at) = open.iter().rposition(|held| *held == name) {
                    let closed = open.remove(at);
                    if closed != name {
                        notes.violated(
                            "phase",
                            "the phase stack removed a different phase than it selected".to_owned(),
                        );
                    }
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
}

fn ending(events: &[Value], notes: &mut Notes<'_>) {
    match events.last() {
        Some(last) if string(last, "type").as_deref() == Some("run-end") => {
            let Some(dropped) = last
                .get("run")
                .and_then(|run| number(run, "events_dropped"))
            else {
                notes.violated(
                    "run-end",
                    "the final event carries no dropped-event count".to_owned(),
                );
                return;
            };
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
                let Some(next_rounds) = rounds.checked_add(1) else {
                    notes.violated(
                        "rejections",
                        "the number of validation rounds exceeds this platform's address space"
                            .to_owned(),
                    );
                    return;
                };
                rounds = next_rounds;
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
    let refused: BTreeSet<u64> = report.rejections.iter().map(|one| one.index).collect();
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
            .find(|one| one.index == *index)
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
fn routed(report: &Report, recorded: &CheckedRecording, notes: &mut Notes<'_>) {
    let routing = &recorded.routing;
    let tests = baseline_tests(&recorded.events);
    let excused = baseline_declines(&recorded.events, notes);
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
        .filter(|row| row.outcome == NOT_RUN && report.interrupted)
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
            || (row.outcome == NOT_RUN && report.interrupted)
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
        reported_route(row, route, notes);
        ran_in_order(row, route, &execs, notes);
        named_its_tests(row, &execs, &tests, notes);
        answered(row, &execs, notes);
        declined(row, &execs, &excused, notes);
        reached(row, route, &execs, notes);
        discharged(row, route, &execs, notes);
    }
}

/// Each decline each target's baseline made, by target, from the verify records: the only declines a mutant execution of the target is excused (ADR 0043).
fn baseline_declines(
    events: &[Value],
    notes: &mut Notes<'_>,
) -> BTreeMap<String, BTreeSet<(String, String)>> {
    let mut excused: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for event in events {
        if string(event, "type").as_deref() != Some("verify") {
            continue;
        }
        let Some(record) = event.get("verify") else {
            continue;
        };
        let Some(target) = string(record, "target") else {
            continue;
        };
        match crate::route::declines(record) {
            Ok(declines) => excused.entry(target).or_default().extend(declines),
            Err(error) => notes.violated(
                &target,
                format!("the baseline's declines cannot be read: {error}"),
            ),
        }
    }
    excused
}

/// A row's declines and what they made of it, re-derived from the execution it rests on and the declines its target's baseline made (ADR 0043).
///
/// A decline the baseline did not make, in the same words, is a detection; declines of every test the execution ran leave it measuring nothing; and a row says which tests declined exactly as its execution recorded them.
fn declined(
    row: &Row,
    execs: &[&crate::route::Exec],
    excused: &BTreeMap<String, BTreeSet<(String, String)>>,
    notes: &mut Notes<'_>,
) {
    let Some(answer) = execs.iter().rev().find(|exec| exec.target == row.target) else {
        return;
    };
    let recorded: BTreeSet<&(String, String)> = answer.declined.iter().collect();
    let claimed: BTreeSet<(String, String)> = row
        .declined
        .iter()
        .map(|one| (one.test.clone(), one.why.clone()))
        .collect();
    if claimed.iter().collect::<BTreeSet<_>>() != recorded {
        notes.violated(
            row.label(),
            format!(
                "the row says {} declined and its execution against {} recorded {}; a report \
                 that disagrees with its own recording is not evidence",
                claimed.len(),
                row.target,
                recorded.len()
            ),
        );
    }
    let changed = recorded.iter().find(|one| match excused.get(&row.target) {
        Some(baseline) => !baseline.contains(**one),
        None => true,
    });
    let every = !recorded.is_empty() && answer.tests_run == Some(count(recorded.len()));
    match (changed, every) {
        (Some((test, why)), _) if row.outcome != KILLED || row.killed_by != [test.clone()] => {
            notes.violated(
                row.label(),
                format!(
                    "{test} declined against {} saying {why:?}, which its baseline did not \
                     say, so the mutation changed what it did; the row says {} by {:?}",
                    row.target, row.outcome, row.killed_by
                ),
            );
        }
        (None, true) if !row.not_run(DECLINED) => notes.violated(
            row.label(),
            format!(
                "every test that ran against {} declined as its baseline did, so the execution \
                 measured nothing; the row says {}",
                row.target, row.outcome
            ),
        ),
        (None, false) if row.not_run(DECLINED) => notes.violated(
            row.label(),
            format!(
                "the row says every test declined and its execution against {} holds a test \
                 that measured",
                row.target
            ),
        ),
        _ => {}
    }
}

/// Every test each target's baseline passed, by target, from the baseline touch records the run kept.
fn baseline_tests(events: &[Value]) -> BTreeMap<String, BTreeSet<String>> {
    crate::drift::of_events(events)
        .touches
        .into_iter()
        .filter(|touch| touch.measured == crate::drift::Measured::Baseline)
        .map(|touch| (touch.target, touch.passed))
        .collect()
}

/// The targets a route says ran are the ones the recording ran, in the order it ran them, the retry of a waited execution apart: a recorded killer asked first is first in both.
fn ran_in_order(
    row: &Row,
    route: &crate::route::Route,
    execs: &[&crate::route::Exec],
    notes: &mut Notes<'_>,
) {
    let mut ran: Vec<&str> = execs.iter().map(|exec| exec.target.as_str()).collect();
    if row.retried {
        ran.pop();
    }
    if ran
        != route
            .executed
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    {
        notes.violated(
            row.label(),
            format!(
                "the route says {:?} ran, in that order, and the recording ran {ran:?}; a route \
                 that names work nobody did, or leaves out work somebody did, is not an account \
                 of the run",
                route.executed
            ),
        );
    }
}

/// Every failing test a kill names is a test its target's baseline ran, which is what tells a test that failed from a line of output read as one.
fn named_its_tests(
    row: &Row,
    execs: &[&crate::route::Exec],
    tests: &BTreeMap<String, BTreeSet<String>>,
    notes: &mut Notes<'_>,
) {
    for exec in execs.iter().filter(|exec| exec.outcome == KILLED) {
        let Some(known) = tests.get(&exec.target) else {
            continue;
        };
        for test in exec
            .failed_tests
            .iter()
            .filter(|test| !known.contains(*test))
        {
            notes.violated(
                row.label(),
                format!(
                    "an execution against {} is counted killed by {test}, which is not a test \
                     its baseline ran; a failure the harness never reported is no kill",
                    exec.target
                ),
            );
        }
    }
}

/// The route copied into the durable report must be the route the recording actually committed, field for field.
fn reported_route(row: &Row, recorded: &crate::route::Route, notes: &mut Notes<'_>) {
    let Some(reported) = row.route.as_ref() else {
        notes.violated(
            row.label(),
            "the recording holds a route and the report omits it".to_owned(),
        );
        return;
    };
    if reported.granularity.as_str() != recorded.granularity {
        notes.violated(
            row.label(),
            format!(
                "the report records route granularity {} and the recording says {}",
                reported.granularity, recorded.granularity
            ),
        );
    }
    if reported.fallback != recorded.fallback {
        notes.violated(
            row.label(),
            "the report and recording disagree about why routing widened".to_owned(),
        );
    }
    let reported_reaching: BTreeSet<&str> = reported.reaching.iter().map(String::as_str).collect();
    let recorded_reaching: BTreeSet<&str> = recorded.reaching.iter().map(String::as_str).collect();
    if reported_reaching != recorded_reaching {
        notes.violated(
            row.label(),
            "the report and recording name different reachable targets".to_owned(),
        );
    }
    let reported_executed: BTreeSet<&str> = reported.executed.iter().map(String::as_str).collect();
    let recorded_executed: BTreeSet<&str> = recorded.executed.iter().map(String::as_str).collect();
    if reported_executed != recorded_executed {
        notes.violated(
            row.label(),
            "the report and recording name different executed targets".to_owned(),
        );
    }
    let reported_discharged: BTreeSet<(&str, &str)> = reported
        .discharged
        .iter()
        .map(|(target, proof)| (target.as_str(), proof.as_str()))
        .collect();
    let recorded_discharged: BTreeSet<(&str, &str)> = recorded
        .discharged
        .iter()
        .map(|discharge| (discharge.target.as_str(), discharge.proof.as_str()))
        .collect();
    if reported_discharged != recorded_discharged {
        notes.violated(
            row.label(),
            "the report and recording name different proof discharges".to_owned(),
        );
    }
}

/// The row's own answer, against the execution of the target it names.
fn answered(row: &Row, execs: &[&crate::route::Exec], notes: &mut Notes<'_>) {
    if execs.is_empty() {
        return;
    }
    for exec in execs {
        attributed(row, exec, notes);
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
    if answer.outcome != row.outcome.as_str() {
        notes.violated(
            row.label(),
            format!(
                "the row says {} and its execution against {} says {}; a report that disagrees \
                 with its own recording is not evidence",
                row.outcome, row.target, answer.outcome
            ),
        );
    }
    let recorded_notice = match answer.step_notice.as_ref() {
        None => None,
        Some(document) => match serde_json::from_value::<StepNotice>(document.clone()) {
            Ok(notice) => Some(notice),
            Err(error) => {
                notes.violated(
                    row.label(),
                    format!("the execution's step-limit evidence is malformed: {error}"),
                );
                return;
            }
        },
    };
    if recorded_notice != row.step_notice {
        notes.violated(
            row.label(),
            "the row and its execution carry different step-limit evidence".to_owned(),
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

/// Whether the row says its process outlived its harness's answer exactly when a recorded execution of it did.
fn lingered(row: &Row, execs: &[&crate::route::Exec], notes: &mut Notes<'_>) {
    let recorded = execs
        .iter()
        .try_fold(false, |any, exec| match exec.lingered {
            crate::route::Linger::Unrecorded => None,
            crate::route::Linger::Ended => Some(any),
            crate::route::Linger::Outlived => Some(true),
        });
    match recorded {
        None => notes.unaudited(
            &row.display_id,
            "an execution of it does not say whether its process outlived its harness's answer, \
             so the row's `lingered` is not re-derived"
                .to_owned(),
        ),
        Some(recorded) if recorded != row.lingered => notes.violated(
            &row.display_id,
            format!(
                "the row says lingered is {} and the recording's executions of it say {recorded}; \
                 whether the harness or the clock decided it is a fact the recording holds",
                row.lingered
            ),
        ),
        Some(_) => {}
    }
}

/// Whether a killed execution that ended on a signal can be credited to the tests: a failing test named, or a signal the process raised by what it did.
fn attributed(row: &Row, exec: &crate::route::Exec, notes: &mut Notes<'_>) {
    let Some(signal) = exec.signal else {
        return;
    };
    if exec.outcome != KILLED || !exec.failed_tests.is_empty() {
        return;
    }
    match raised(signal) {
        Raised::Itself => {}
        Raised::Outside => notes.violated(
            row.label(),
            format!(
                "an execution against {} is counted killed on signal {signal}, which another \
                 process sends, and names no failing test; nothing the tests did ended it",
                exec.target
            ),
        ),
        Raised::Unknown => notes.unaudited(
            row.label(),
            format!(
                "signal {signal} means a different thing on different platforms and the \
                 recording does not say which it ran on, so whether the process raised it is \
                 not known"
            ),
        ),
    }
}

/// Who raised the signal a process died of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Raised {
    Itself,
    Outside,
    Unknown,
}

/// Who raised `signal`, read from the numbers every POSIX platform shares and nothing else.
const fn raised(signal: i64) -> Raised {
    match signal {
        4 | 5 | 6 | 8 | 11 => Raised::Itself,
        7 | 10 | 12 | 31 => Raised::Unknown,
        _ => Raised::Outside,
    }
}

/// A wall-clock expiry the run believed, against the serial retry that is what believing one takes.
fn retried(row: &Row, execs: &[&crate::route::Exec], notes: &mut Notes<'_>) {
    lingered(row, execs, notes);
    if row.lingered && row.outcome == WAITED {
        notes.violated(
            &row.display_id,
            "the row says its harness had already answered when the clock ended the process, and \
             that the clock decided it; a verdict the harness gave is not a wait"
                .to_owned(),
        );
    }
    let waited = execs.iter().filter(|exec| exec.outcome == WAITED).count();
    if row.outcome == WAITED && (waited < 2 || !row.retried) {
        notes.violated(
            row.label(),
            format!(
                "the row says this machine stopped waiting and the recording holds {waited} \
                 expiries with retried={}; a wall-clock expiry is believed only after it \
                 repeats on its own",
                row.retried
            ),
        );
    }
    if row.retried && waited == 0 {
        notes.violated(
            row.label(),
            "the row says it was retried and no wait expired; a retry is what a wall-clock \
             expiry costs and nothing else asks for one"
                .to_owned(),
        );
    }
    if row.outcome == INCONCLUSIVE && row.retried && waited < 1 {
        notes.violated(
            row.label(),
            "the row could not be decided after a retry and no wait expired; inconclusive \
             after a retry is what an expiry that did not repeat leaves behind"
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

/// The ledger of accepted survivors, against the run that was asked to hold to it.
/// The work the report claims, against the routes it claims it from and the recording of what ran.
pub(super) fn work(
    report: &Report,
    recorded: Option<&CheckedRecording>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Work);
    let targets = report.targets.len();
    if targets == 0 {
        notes.unaudited(
            "work",
            "the report names no targets, so what a whole run would have started cannot be \
             re-derived"
                .to_owned(),
        );
        return notes.looked();
    }
    let built: BTreeSet<&str> = report.targets.iter().map(String::as_str).collect();
    let mut started: u64 = 0;
    for row in &report.mutants {
        pairs_of(row, &built, targets, &mut notes);
        if row.source_run_id.is_none() {
            let Some(route) = row.route.as_ref() else {
                continue;
            };
            let Ok(executed) = u64::try_from(route.executed.len()) else {
                notes.violated(
                    row.label(),
                    "the route's execution count exceeds the report's integer width".to_owned(),
                );
                return notes.looked();
            };
            let Some(next) = started
                .checked_add(executed)
                .and_then(|total| total.checked_add(u64::from(row.retried)))
            else {
                notes.violated(
                    "executions",
                    "the execution total exceeds the report's integer width".to_owned(),
                );
                return notes.looked();
            };
            started = next;
        }
    }
    let Some(recorded) = recorded else {
        notes.unaudited(
            "executions",
            "no recording was given, so what the report says it started cannot be held to what \
             ran"
            .to_owned(),
        );
        return notes.looked();
    };
    let ran = recorded
        .events
        .iter()
        .filter(|event| {
            string(event, "type").as_deref() == Some("mutant-exec")
                && event
                    .get("mutant")
                    .and_then(|record| record.get("id"))
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.is_empty())
        })
        .count();
    let Ok(ran) = u64::try_from(ran) else {
        notes.violated(
            "executions",
            "the recording's execution count exceeds the report's integer width".to_owned(),
        );
        return notes.looked();
    };
    if ran != started {
        notes.violated(
            "executions",
            format!(
                "the report accounts for {started} processes and the recording holds {ran}; a \
                 report that undercounts its own work is one nobody can hold to doing less"
            ),
        );
    }
    notes.looked()
}

/// Whether one row's pairs are ones the run built targets for and its own route reached.
fn pairs_of(row: &Row, built: &BTreeSet<&str>, targets: usize, notes: &mut Notes<'_>) {
    let Some(route) = row.route.as_ref() else {
        return;
    };
    let reaching: BTreeSet<&str> = route.reaching.iter().map(String::as_str).collect();
    let discharged: BTreeSet<&str> = route.discharged.iter().map(|(id, _)| id.as_str()).collect();
    for name in route.executed.iter().map(String::as_str) {
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
    let Some(accounted) = reaching.len().checked_add(discharged.len()) else {
        notes.violated(
            row.label(),
            "the route's target count exceeds this platform's address space".to_owned(),
        );
        return;
    };
    if accounted > targets {
        notes.violated(
            row.label(),
            format!("the route accounts for {accounted} targets and the run built {targets}"),
        );
    }
    if row.source_run_id.is_some() && !route.executed.is_empty() {
        notes.violated(
            row.label(),
            "an earlier run established this and a process was started for it anyway".to_owned(),
        );
    }
}

/// The name of the phase one boundary is about.
fn phase_name(event: &Value) -> String {
    event
        .get("phase")
        .and_then(|phase| string(phase, "name"))
        .unwrap_or_default()
}
