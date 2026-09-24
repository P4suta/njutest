// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a runner recording says about the crashes a run put, read from the stream alone and decided again from it (ADR 0035).

use serde_json::Value;

/// One run of a test a crash was put to, in recording order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Run {
    /// The crash a person types.
    pub crash: String,
    /// The target the test is in.
    pub target: String,
    /// The test.
    pub test: String,
    /// Which run: `crash`, `next` or `fresh`.
    pub stage: String,
    /// The exit status.
    pub exit_code: i64,
    /// What the engine made of it.
    pub outcome: String,
    /// What a stopped run left.
    pub left: Vec<String>,
    /// What a next or fresh run failed.
    pub failed: Vec<String>,
}

/// What a crash came to, as a report writes it and as this audit decides it again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Site {
    /// The crash a person types.
    pub crash: String,
    /// The decision's wire name.
    pub decision: String,
    /// The target and test it is about, where it names one.
    pub on: String,
    /// What the stop left, where it says.
    pub left: Vec<String>,
    /// What the next run failed, where it says.
    pub failed: Vec<String>,
}

/// Every crash run a recording holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Crashed {
    /// Every run, in recording order.
    pub runs: Vec<Run>,
}

/// The exit status of a test process a crash stopped, written out again from the engine's contract rather than read from its code.
pub const CRASH_EXIT: i64 = 93;

/// Everything the recording says about the crashes.
///
/// # Errors
/// A corrupt non-empty line is rejected rather than disappearing from the evidence.
pub fn read(recorded: &str) -> Result<Crashed, crate::route::ReadError> {
    let mut crashed = Crashed::default();
    for event in crate::route::events(recorded)? {
        if event.get("type").and_then(Value::as_str) != Some("crash-exec") {
            continue;
        }
        let Some(record) = event.get("crash") else {
            continue;
        };
        crashed.runs.push(Run {
            crash: text(record, "crash"),
            target: text(record, "target"),
            test: text(record, "test"),
            stage: text(record, "stage"),
            exit_code: record
                .get("exit_code")
                .and_then(Value::as_i64)
                .unwrap_or(-1),
            outcome: text(record, "outcome"),
            left: texts(record, "left"),
            failed: texts(record, "failed"),
        });
    }
    Ok(crashed)
}

/// One site as a report writes it.
#[must_use]
pub fn site(record: &Value) -> Site {
    let decision = record.get("decision").cloned().unwrap_or_default();
    Site {
        crash: text(record, "display_id"),
        decision: text(&decision, "decision"),
        on: text(&decision, "on"),
        left: texts(&decision, "left"),
        failed: texts(&decision, "failed"),
    }
}

/// What the ordered runs of one crash decide, by the run's own steps and none of its code.
///
/// The first test that stopped at the call decides, a test that passed without stopping hands on to the next, and anything else is undecided.
/// A report whose decision is not this one claims something its runs do not show.
#[must_use]
pub fn decided(crash: &str, runs: &[&Run]) -> Site {
    let site = |decision: &str, on: String| Site {
        crash: crash.to_owned(),
        decision: decision.to_owned(),
        on,
        ..Site::default()
    };
    let mut rest = runs;
    while let Some((run, after)) = rest.split_first() {
        let on = format!("{}::{}", run.target, run.test);
        if run.stage != "crash" {
            return site("undecided", on);
        }
        if run.exit_code != CRASH_EXIT {
            if run.outcome == "survived" {
                rest = after;
                continue;
            }
            return site("undecided", on);
        }
        if run.left.is_empty() {
            return site("unshared", on);
        }
        let stage = |offset: usize| after.get(offset).map(|one| (one.stage.as_str(), *one));
        return match stage(0) {
            Some(("next", next)) if next.outcome == "survived" => Site {
                left: run.left.clone(),
                ..site("restarted", on)
            },
            Some(("next", next)) if next.outcome == "killed" => {
                let confirmed = matches!(stage(1), Some(("fresh", fresh)) if fresh.outcome == "survived")
                    && matches!(stage(2), Some(("crash", again)) if again.exit_code == CRASH_EXIT)
                    && matches!(stage(3), Some(("next", again)) if again.outcome == "killed");
                if confirmed {
                    Site {
                        failed: next.failed.clone(),
                        ..site("corrupt", on)
                    }
                } else {
                    site("undecided", on)
                }
            }
            _ => site("undecided", on),
        };
    }
    site("unreached", String::new())
}

/// Whether a report's record of a crash says what its runs decide, where it rests on runs at all.
///
/// `undecided` claims less than any run shows, so it agrees with every recording; a crash the compiler refused has no runs; and every other decision is the one the runs decide, with the same test, files and failures.
#[must_use]
pub fn agrees(reported: &Site, runs: &[&Run]) -> bool {
    if reported.decision == "undecided" {
        return true;
    }
    if runs.is_empty() {
        return matches!(
            reported.decision.as_str(),
            "unreached" | "not-put" | "undecided"
        );
    }
    let derived = decided(&reported.crash, runs);
    derived.decision == reported.decision
        && match derived.decision.as_str() {
            "restarted" => derived.on == reported.on && derived.left == reported.left,
            "corrupt" => derived.on == reported.on && derived.failed == reported.failed,
            "unshared" => derived.on == reported.on,
            _ => true,
        }
}

/// One string field, or the empty string where the recording does not carry it.
fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}

/// One list of strings, or none where the recording does not carry it.
fn texts(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
