// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a runner recording says about the crashes a run put, read from the stream alone (ADR 0035).

use serde_json::Value;

/// One run of a test a crash was put to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Run {
    /// The crash a person types.
    pub crash: String,
    /// Which run: `crash`, `next` or `fresh`.
    pub stage: String,
    /// The exit status.
    pub exit_code: i64,
    /// What the engine made of it.
    pub outcome: String,
}

/// What the report says one call that writes came to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Site {
    /// The crash a person types.
    pub crash: String,
    /// The decision's wire name.
    pub decision: String,
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
            stage: text(record, "stage"),
            exit_code: record
                .get("exit_code")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            outcome: text(record, "outcome"),
        });
    }
    Ok(crashed)
}

/// One site as a report writes it.
#[must_use]
pub fn site(record: &Value) -> Site {
    Site {
        crash: text(record, "display_id"),
        decision: record
            .get("decision")
            .map(|decision| text(decision, "decision"))
            .unwrap_or_default(),
    }
}

/// What the runs of one crash contradict about the decision the run gave it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Contradiction {
    /// A decision that rests on a stop has no run that stopped.
    #[error(
        "the run says it is {decision}, and no run of it the recording holds stopped at the call"
    )]
    NeverStopped {
        /// The decision given.
        decision: String,
    },
    /// A decision about a next run has none, or one that came to something else.
    #[error("the run says it is {decision}, and no next run of it came to {expected}")]
    NextRun {
        /// The decision given.
        decision: String,
        /// What the next run had to come to.
        expected: &'static str,
    },
    /// A corrupt crash lacks its fresh pass or its second stop.
    #[error(
        "the run says it is corrupt, and the recording holds no passing fresh run and two failing next runs of it"
    )]
    Unconfirmed,
    /// A crash said never to have stopped stopped.
    #[error("the run says it was {decision}, and a run of it stopped at the call")]
    Stopped {
        /// The decision given.
        decision: String,
    },
    /// A decision no crash can come to.
    #[error("{decision:?} is no decision a crash can come to")]
    Unknown {
        /// The decision given.
        decision: String,
    },
}

/// Whether the runs of one crash support the decision the run gave it.
///
/// # Errors
/// The [`Contradiction`] the runs hold.
pub fn supports(site: &Site, runs: &[&Run]) -> Result<(), Contradiction> {
    let stopped = runs
        .iter()
        .filter(|run| run.stage == "crash" && run.exit_code == CRASH_EXIT)
        .count();
    let next = |outcome: &str| {
        runs.iter()
            .filter(|run| run.stage == "next" && run.outcome == outcome)
            .count()
    };
    let decision = site.decision.clone();
    match site.decision.as_str() {
        "restarted" | "unshared" | "corrupt" if stopped == 0 => {
            Err(Contradiction::NeverStopped { decision })
        }
        "restarted" if next("survived") == 0 => Err(Contradiction::NextRun {
            decision,
            expected: "survived",
        }),
        "corrupt"
            if next("killed") < 2
                || !runs
                    .iter()
                    .any(|run| run.stage == "fresh" && run.outcome == "survived") =>
        {
            Err(Contradiction::Unconfirmed)
        }
        "unreached" | "not-put" if stopped > 0 => Err(Contradiction::Stopped { decision }),
        "restarted" | "unshared" | "corrupt" | "unreached" | "not-put" | "undecided" => Ok(()),
        _ => Err(Contradiction::Unknown { decision }),
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
