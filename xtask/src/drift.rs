// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-derivation, from the engine's recording alone, of which targets reached something different on a control than on their baseline.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// Which run of a whole target one touch record was measured on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Measured {
    /// The one run of every target with nothing active, which routing rests on.
    Baseline,
    /// An original-code control of the whole target, run to confirm a kill.
    Control,
    /// A mutant run again against a target whose reach moved (ADR 0036).
    Repair,
}

impl Measured {
    /// The name the engine's recording spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Control => "control",
            Self::Repair => "repair",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }
}

/// One touch record, as the engine wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touch {
    /// The target.
    pub target: String,
    /// Which run it was measured on.
    pub measured: Measured,
    /// The tests that run passed, which is what its reach is the reach of.
    pub passed: BTreeSet<String>,
    /// Whether the tests the record names are the harness's answer: under libtest, whether they come to its summary's count; in any other protocol, which names none, nothing to fall short of.
    pub whole: bool,
    /// Every mutant site anything of it reached.
    pub reached: BTreeSet<u64>,
    /// Every branch body anything of it entered.
    pub bodies: BTreeSet<u64>,
    /// Every mutation anything of it saw its guard's two branches differ over.
    pub infected: BTreeSet<u64>,
}

impl Touch {
    const fn parsed_whole(&self) -> bool {
        self.whole
    }

    fn unions_equal(&self, other: &Self) -> bool {
        self.reached == other.reached
            && self.bodies == other.bodies
            && self.infected == other.infected
    }
}

/// What the touch records of one engine recording say about one measured target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Standing {
    /// A control that passed the same tests reached exactly what the baseline did.
    Held,
    /// A control that passed the same tests reached something else, so the target's reach is not a function of the target.
    Moved,
    /// No control that passed the same tests recorded what it reached.
    NotMeasured,
}

impl Standing {
    /// The name a report spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Moved => "moved",
            Self::NotMeasured => "not-measured",
        }
    }

    /// The standing of that name, or nothing where no standing is spelled that way.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }
}

/// Every touch record of one engine recording, and how many it could not read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Touched {
    /// Every record that carried what a re-derivation needs.
    pub touches: Vec<Touch>,
    /// How many records lacked it, which a re-derivation cannot count as agreement.
    pub unreadable: usize,
    /// Every target whose baseline passed only when run again, so it was not measured under the conditions a control is.
    pub retried: BTreeSet<String>,
}

/// Every touch record of one engine recording.
///
/// # Errors
/// A non-empty line that is not JSON rejects the whole recording.
pub fn read(recorded: &str) -> Result<Touched, crate::route::ReadError> {
    let mut touched = Touched::default();
    for event in crate::route::events(recorded)? {
        if event.get("type").and_then(Value::as_str) == Some("verify")
            && let Some(verify) = event.get("verify")
            && verify.get("retried").and_then(Value::as_bool) == Some(true)
            && let Some(target) = verify.get("target").and_then(Value::as_str)
        {
            touched.retried.insert(target.to_owned());
        }
        if event.get("type").and_then(Value::as_str) != Some("touch") {
            continue;
        }
        match event.get("touch").and_then(touch) {
            Some(one) => touched.touches.push(one),
            None => touched.unreadable = touched.unreadable.saturating_add(1),
        }
    }
    Ok(touched)
}

fn touch(record: &Value) -> Option<Touch> {
    let named = record.get("passed")?.as_array()?.len();
    let summary = record.get("summary")?;
    let whole = match summary.get("protocol")?.as_str()? {
        "libtest" => match summary.get("tests_run")? {
            Value::Null => false,
            count => usize::try_from(count.as_u64()?).is_ok_and(|count| count == named),
        },
        "custom" | "remembered" => true,
        "unanswered" => false,
        _ => return None,
    };
    Some(Touch {
        whole,
        target: record.get("target")?.as_str()?.to_owned(),
        measured: Measured::parse(record.get("measured")?.as_str()?)?,
        passed: record
            .get("passed")?
            .as_array()?
            .iter()
            .map(|test| test.as_str().map(ToOwned::to_owned))
            .collect::<Option<BTreeSet<String>>>()?,
        reached: indices(record.get("reached_sites")?)?,
        bodies: indices(record.get("entered_bodies")?)?,
        infected: indices(record.get("infected_sites")?)?,
    })
}

fn indices(value: &Value) -> Option<BTreeSet<u64>> {
    value.as_array()?.iter().map(Value::as_u64).collect()
}

/// What the records say about every target a baseline record names, re-derived without the engine; a baseline that passed only on retry is compared with nothing.
#[must_use]
pub fn standings(touched: &Touched) -> BTreeMap<String, Standing> {
    let mut baselines: BTreeMap<&str, &Touch> = BTreeMap::new();
    for touch in touched
        .touches
        .iter()
        .filter(|touch| touch.measured == Measured::Baseline)
    {
        baselines.insert(touch.target.as_str(), touch);
    }
    baselines
        .into_iter()
        .map(|(target, baseline)| {
            let comparable: Vec<&Touch> = touched
                .touches
                .iter()
                .filter(|touch| touch.measured == Measured::Control)
                .filter(|touch| touch.target == target && touch.passed == baseline.passed)
                .collect();
            let standing = if comparable.is_empty()
                || touched.retried.contains(target)
                || !baseline.parsed_whole()
                || comparable.iter().any(|control| !control.parsed_whole())
            {
                Standing::NotMeasured
            } else if comparable
                .iter()
                .all(|control| control.unions_equal(baseline))
            {
                Standing::Held
            } else {
                Standing::Moved
            };
            (target.to_owned(), standing)
        })
        .collect()
}
