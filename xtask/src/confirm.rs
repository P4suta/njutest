// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-derivation, from the runner's recording alone, of how each kill and wait was confirmed: the control that answered and what the second run came to.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

/// Each thing a confirmation must be, by which a violation says what it broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum ConfirmRule {
    /// A kill, wait or unconfirmed disposition has a confirmation recorded for it against its target.
    Missing,
    /// A confirmation rests on a control recorded before it, for the same test and asked for the mutation it names.
    Uncontrolled,
    /// The original code is asked each question once.
    Twice,
    /// A mutation is run a second time only where its control passed.
    Rerun,
    /// A mutation whose control passed is run a second time.
    NoRerun,
    /// A kill or wait stands only where its second run came to it again.
    Unconfirmed,
    /// A disposition left unconfirmed has a last confirmation that failed.
    Confirmed,
}

impl ConfirmRule {
    /// What a violation of it is prefixed with.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Uncontrolled => "uncontrolled",
            Self::Twice => "twice",
            Self::Rerun => "rerun",
            Self::NoRerun => "no-rerun",
            Self::Unconfirmed => "unconfirmed",
            Self::Confirmed => "confirmed",
        }
    }
}

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

/// What one run of a mutation came to, as the engine names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Never run.
    NotRun,
    /// A test failed.
    Killed,
    /// Every test passed.
    Survived,
    /// A process reached the step limit.
    StepLimitReached,
    /// A bound expired while the machine watched.
    Waited,
    /// Nothing could be decided.
    Inconclusive,
    /// The harness failed.
    Errored,
}

impl Expected {
    /// The outcome of a second run that confirms it.
    #[must_use]
    pub const fn outcome(self) -> Outcome {
        match self {
            Self::Killed => Outcome::Killed,
            Self::Waited => Outcome::Waited,
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
    pub reproduced: Option<Outcome>,
}

/// One kill inherited from an interrupted run's checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resumed {
    /// The mutation, in full.
    pub mutant: String,
    /// The target the interrupted run said killed it.
    pub killed_by: String,
}

/// Every control, confirmation and inherited kill of one recording, each control and confirmation with its sequence number.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Confirmations {
    /// Every control, in recording order.
    pub controls: Vec<(u64, Control)>,
    /// Every confirmation, in recording order.
    pub confirms: Vec<(u64, Confirm)>,
    /// Every mutation inherited from a checkpoint, in full.
    pub resumed: BTreeSet<String>,
}

/// What one confirmation decides, re-derived from its control and its second run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decided {
    /// The control passed and the second run came to what the first did.
    Stands,
    /// The control failed, or the second run came to something else.
    Unconfirmed,
    /// The recording contradicts itself about it.
    Broke(ConfirmRule, String),
}

/// The one record of type `kind` in `event` at `line`, with its sequence number.
fn body<T: serde::de::DeserializeOwned>(
    event: &Value,
    kind: &str,
    line: usize,
) -> Result<(u64, T), crate::route::ReadError> {
    let refused = |source: serde_json::Error| crate::route::ReadError { line, source };
    let seq = event.get("seq").and_then(Value::as_u64).ok_or_else(|| {
        refused(serde::de::Error::custom(
            "a recorded event has a whole sequence number",
        ))
    })?;
    let record = event
        .get(kind)
        .cloned()
        .ok_or_else(|| refused(serde::de::Error::missing_field("record")))?;
    Ok((seq, T::deserialize(record).map_err(refused)?))
}

/// Every control, confirmation and inherited kill in `recorded`.
///
/// # Errors
/// A line that is not JSON, and a control, confirmation or inherited kill that is not the shape the runner writes, or has no whole sequence number.
pub fn read(recorded: &str) -> Result<Confirmations, crate::route::ReadError> {
    let mut read = Confirmations::default();
    for (line, event) in (1_usize..).zip(crate::route::events(recorded)?) {
        match event.get("type").and_then(Value::as_str) {
            Some("control") => read.controls.push(body(&event, "control", line)?),
            Some("confirm") => read.confirms.push(body(&event, "confirm", line)?),
            Some("resumed") => {
                let (_, resumed): (u64, Resumed) = body(&event, "resumed", line)?;
                read.resumed.insert(resumed.mutant);
            }
            _ => {}
        }
    }
    Ok(read)
}

impl Confirmations {
    /// Every question the original code was asked more than once, as its target and test.
    #[must_use]
    pub fn asked_twice(&self) -> Vec<(Option<String>, Option<String>)> {
        let mut asked: BTreeMap<(Option<String>, Option<String>), usize> = BTreeMap::new();
        for (_, control) in &self.controls {
            let count = asked
                .entry((control.target.clone(), control.test.clone()))
                .or_default();
            *count = count.saturating_add(1);
        }
        asked
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(question, _)| question)
            .collect()
    }

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
            return Decided::Broke(
                ConfirmRule::Uncontrolled,
                format!(
                    "its confirmation rests on a control asked for {} that the recording does \
                     not hold before it",
                    confirm.answered_for
                ),
            );
        };
        match (&control.answer, confirm.reproduced) {
            (Answer::Failed { .. }, Some(outcome)) => Decided::Broke(
                ConfirmRule::Rerun,
                format!(
                    "its control failed on the original code, and the recording says it was run \
                     a second time anyway and came to {outcome:?}"
                ),
            ),
            (Answer::Passed, None) => Decided::Broke(
                ConfirmRule::NoRerun,
                "its control passed, and the recording holds no second run of it".to_owned(),
            ),
            (Answer::Passed, Some(outcome)) if outcome == confirm.expected.outcome() => {
                Decided::Stands
            }
            (Answer::Failed { .. }, None) | (Answer::Passed, Some(_)) => Decided::Unconfirmed,
        }
    }
}
