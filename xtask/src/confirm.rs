// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-derivation, from the runner's recording alone, of how each kill and wait was confirmed: the control that answered and what the second run came to.

use serde::Deserialize;
use serde_json::Value;

/// What the original code answered to one test.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Answer {
    /// It passed.
    Passed,
    /// It failed on the original code too.
    Failed {
        /// What it said.
        detail: String,
    },
}

/// One control the run recorded, the one time it ran.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// The target.
    pub target: Option<String>,
    /// The one test, where the question named one.
    pub test: Option<String>,
    /// The mutation whose asking ran it.
    pub asked_for: String,
    /// What the original code said.
    pub answer: Answer,
}

/// What a confirmation expected the second run to come to again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Expected {
    /// A kill.
    Killed,
    /// A wait.
    Waited,
}

impl Expected {
    /// The outcome a report spells it with.
    #[must_use]
    pub const fn outcome(self) -> &'static str {
        match self {
            Self::Killed => "killed",
            Self::Waited => "waited",
        }
    }
}

/// One confirmation the run recorded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm {
    /// The mutation, in full.
    pub mutant: String,
    /// The target.
    pub target: Option<String>,
    /// The one test, where it named one.
    pub test: Option<String>,
    /// What it expected again.
    pub expected: Expected,
    /// The mutation whose asking ran the control that answered.
    pub answered_for: String,
    /// What the second run came to, or nothing where there was none.
    pub reproduced: Option<String>,
}

/// Every control and confirmation of one recording, each with its sequence number.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Confirmations {
    /// Every control, in recording order.
    pub controls: Vec<(u64, Control)>,
    /// Every confirmation, in recording order.
    pub confirms: Vec<(u64, Confirm)>,
}

/// What one confirmation decides, re-derived from its control and its second run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decided {
    /// The control passed and the second run came to what the first did, so the disposition stands.
    Stands(Expected),
    /// The control failed, or the second run came to something else.
    Unconfirmed,
    /// The recording contradicts itself about it.
    Contradicted(String),
}

/// Every control and confirmation in `recorded`.
///
/// # Errors
/// A line that is not JSON, and a control or confirmation that is not the shape the runner writes.
pub fn read(recorded: &str) -> Result<Confirmations, crate::route::ReadError> {
    let mut read = Confirmations::default();
    for (line, event) in (1_usize..).zip(crate::route::events(recorded)?) {
        let seq = event.get("seq").and_then(Value::as_u64).unwrap_or_default();
        match event.get("type").and_then(Value::as_str) {
            Some("control") => {
                if let Some(record) = event.get("control").cloned() {
                    let control = Control::deserialize(record)
                        .map_err(|source| crate::route::ReadError { line, source })?;
                    read.controls.push((seq, control));
                }
            }
            Some("confirm") => {
                if let Some(record) = event.get("confirm").cloned() {
                    let confirm = Confirm::deserialize(record)
                        .map_err(|source| crate::route::ReadError { line, source })?;
                    read.confirms.push((seq, confirm));
                }
            }
            _ => {}
        }
    }
    Ok(read)
}

impl Confirmations {
    /// What the confirmation recorded at `seq` decides, held to the control recorded before it for the same test and asker.
    #[must_use]
    pub fn decided(&self, seq: u64, confirm: &Confirm) -> Decided {
        let Some((_, control)) = self.controls.iter().rev().find(|(at, control)| {
            let asked_for_whom_it_answered = control.asked_for == confirm.answered_for;
            *at < seq
                && asked_for_whom_it_answered
                && control.target == confirm.target
                && control.test == confirm.test
        }) else {
            return Decided::Contradicted(format!(
                "its confirmation rests on a control asked for {} that the recording does not \
                 hold before it",
                confirm.answered_for
            ));
        };
        match (&control.answer, confirm.reproduced.as_deref()) {
            (Answer::Failed { .. }, Some(outcome)) => Decided::Contradicted(format!(
                "its control failed on the original code, and the recording says it was run a \
                 second time anyway and came to {outcome}"
            )),
            (Answer::Passed, None) => Decided::Contradicted(
                "its control passed, and the recording holds no second run of it".to_owned(),
            ),
            (Answer::Passed, Some(outcome)) if outcome == confirm.expected.outcome() => {
                Decided::Stands(confirm.expected)
            }
            (Answer::Failed { .. }, None) | (Answer::Passed, Some(_)) => Decided::Unconfirmed,
        }
    }
}
