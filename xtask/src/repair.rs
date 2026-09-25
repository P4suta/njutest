// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a runner recording says each run again against a moved target came to (ADR 0036), read from the stream alone.

use serde_json::Value;

/// One disposition run again against a target whose reach moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repair {
    /// The mutant, by the name a person types.
    pub mutant: String,
    /// The moved target it was run against.
    pub target: String,
    /// The disposition it had.
    pub was: String,
    /// The one it has now.
    pub now: String,
    /// Whether that run reached the mutant's site: `reached`, `not-reached` or `unrecorded`.
    pub reached: String,
}

/// Every repair record of a runner recording, in the order it was written.
///
/// # Errors
/// A corrupt non-empty line is rejected rather than disappearing from the evidence.
pub fn read(recorded: &str) -> Result<Vec<Repair>, crate::route::ReadError> {
    let mut repairs = Vec::new();
    for event in crate::route::events(recorded, crate::schemas::Producer::Runner)? {
        if event.get("type").and_then(Value::as_str) != Some("repair") {
            continue;
        }
        let Some(record) = event.get("repair") else {
            continue;
        };
        let text = |key: &str| {
            record
                .get(key)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_default()
        };
        repairs.push(Repair {
            mutant: text("mutant"),
            target: text("target"),
            was: text("was"),
            now: text("now"),
            reached: text("reached"),
        });
    }
    Ok(repairs)
}

/// What one repair's own evidence decides: whether its run reached the site, and the dispositions it may now carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    /// `reached`, `not-reached` or `unrecorded`, from the repair's own touch record.
    pub reached: &'static str,
    /// Every disposition its last execution allows: a kill is `killed` or, where its control failed, `unconfirmed`.
    pub now: Vec<String>,
}

/// Where a repair's evidence cannot be read into a decision.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Contradiction {
    /// No execution of the mutant against the target was recorded.
    #[error("no execution of it against {target} was recorded")]
    NotRun {
        /// The moved target.
        target: String,
    },
    /// An execution came to something no run comes to.
    #[error("its last execution came to {outcome:?}, which no run comes to")]
    Outcome {
        /// What it said.
        outcome: String,
    },
}

/// What a repair of a mutation at `index` whose last execution came to `outcome`, with the repair touch record `touch`, decides, `was` being what it had.
///
/// # Errors
/// [`Contradiction::Outcome`] for an outcome no run comes to.
pub fn derived(
    was: &str,
    outcome: &str,
    (index, touch): (u64, Option<&crate::drift::Touch>),
) -> Result<Derived, Contradiction> {
    let reached = match touch {
        None => "unrecorded",
        Some(touch) if touch.reached.contains(&index) => "reached",
        Some(_) => "not-reached",
    };
    let now: Vec<String> = match outcome {
        "survived" if reached == "reached" => vec!["survived".to_owned()],
        "survived" => vec![was.to_owned()],
        "killed" => vec!["killed".to_owned(), "unconfirmed".to_owned()],
        "waited" => vec!["waited".to_owned()],
        "step_limit_reached" => vec!["step-limit-reached".to_owned()],
        "errored" | "inconclusive" | "not_run" => vec!["errored".to_owned()],
        other => {
            return Err(Contradiction::Outcome {
                outcome: other.to_owned(),
            });
        }
    };
    Ok(Derived { reached, now })
}
