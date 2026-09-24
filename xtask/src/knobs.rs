// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-derivation, from the engine's recording alone, of what each control started under a knob established against its target's baseline.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::drift::{Measured, Touch, Touched};

/// One thing a control is started with differently from its baseline, by the name a report gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Knob {
    /// The zone local time is read in.
    Timezone,
    /// The locale text is folded and formatted under.
    Locale,
    /// Where temporary files go.
    TempDirectory,
    /// The home directory.
    Home,
    /// The mode a new file is created with.
    Umask,
    /// The size of the terminal.
    Columns,
    /// How many tests run beside each other.
    Threads,
}

impl Knob {
    /// The name a report spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Timezone => "timezone",
            Self::Locale => "locale",
            Self::TempDirectory => "temp-directory",
            Self::Home => "home",
            Self::Umask => "umask",
            Self::Columns => "columns",
            Self::Threads => "threads",
        }
    }

    /// The knob of that name, or nothing where no knob is spelled that way.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }

    /// Whether `started` is this knob put, by this audit's own reading of what each knob sets, written without the runner's.
    fn puts(self, started: &Started) -> bool {
        if started.delayed.is_some() {
            return false;
        }
        let names: Vec<&str> = started
            .environment
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        let plain = started.launcher.is_none() && started.arguments.is_empty();
        let text = started
            .environment
            .iter()
            .all(|(_, value)| value.as_deref().is_some_and(|value| !value.is_empty()));
        match self {
            Self::Timezone => plain && text && names == ["TZ"],
            Self::Locale => plain && text && names == ["LC_ALL"],
            Self::TempDirectory => {
                let mut values = started.environment.iter().map(|(_, value)| value);
                let first = values.next();
                plain && names == ["TMPDIR", "TMP", "TEMP"] && values.all(|one| Some(one) == first)
            }
            Self::Home => {
                plain && (names == ["HOME"] || names == ["HOME", "CARGO_HOME", "RUSTUP_HOME"])
            }
            Self::Umask => {
                names.is_empty()
                    && started.arguments.is_empty()
                    && started
                        .launcher
                        .as_deref()
                        .is_some_and(|launcher| launcher.starts_with("umask "))
            }
            Self::Columns => plain && text && names == ["COLUMNS", "LINES"],
            Self::Threads => {
                names.is_empty()
                    && started.launcher.is_none()
                    && started.arguments == ["--test-threads=1"]
            }
        }
    }
}

/// What one control was started with beyond what its baseline was, as the engine wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// Each variable set over its environment, with its value where the value is text.
    pub environment: Vec<(String, Option<String>)>,
    /// What the shell it was started through ran first.
    pub launcher: Option<String>,
    /// The harness arguments added.
    pub arguments: Vec<String>,
    /// The catalog index of the guard it paused its threads at, which makes it a schedule and never a knob.
    pub delayed: Option<u64>,
    /// The guard whose delayed failure it is the undelayed half of a confirming round for.
    pub confirms: Option<u64>,
}

/// What one perturbed control was started as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The putting of one knob.
    Knob(Knob),
    /// A schedule: its threads paused at one guard, with nothing else set.
    Delayed,
    /// Nothing beyond its baseline, named as the undelayed half of a round confirming a delayed failure.
    Undelayed,
    /// Nothing a run starts a control as.
    Unknown,
}

impl Started {
    /// What it was started as, read from everything it was started with.
    #[must_use]
    pub fn role(&self) -> Role {
        let plain =
            self.environment.is_empty() && self.launcher.is_none() && self.arguments.is_empty();
        match (self.delayed, self.confirms, plain) {
            (Some(_), None, true) => Role::Delayed,
            (None, Some(_), true) => Role::Undelayed,
            (None, None, true) => Role::Unknown,
            (None, None, false) => self.knob().map_or(Role::Unknown, Role::Knob),
            (Some(_), Some(_), _) | (Some(_), None, false) | (None, Some(_), false) => {
                Role::Unknown
            }
        }
    }

    /// The one knob this is the putting of, or nothing where no knob starts a control this way.
    #[must_use]
    pub fn knob(&self) -> Option<Knob> {
        let mut putting = Knob::ALL.into_iter().filter(|knob| knob.puts(self));
        let one = putting.next()?;
        putting.next().is_none().then_some(one)
    }

    /// What it was started with, as a reader types it.
    #[must_use]
    pub fn said(&self) -> String {
        let mut words: Vec<String> = self
            .environment
            .iter()
            .map(|(name, value)| format!("{name}={}", value.as_deref().unwrap_or("<not text>")))
            .collect();
        words.extend(self.launcher.iter().cloned());
        words.extend(self.arguments.iter().cloned());
        if words.is_empty() {
            "nothing beyond its baseline".to_owned()
        } else {
            words.join(" ")
        }
    }
}

/// How one perturbed control ended, read from the outcome the engine wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Every test it ran passed.
    Passed,
    /// A test failed.
    Failed,
    /// It ran past its bound.
    Waited,
    /// It established nothing about its tests: it did not run, stopped at a step limit, could not be read, or errored.
    Unsettled,
}

impl Ended {
    fn parse(outcome: &str) -> Option<Self> {
        match outcome {
            "survived" => Some(Self::Passed),
            "killed" => Some(Self::Failed),
            "waited" => Some(Self::Waited),
            "not_run" | "step_limit_reached" | "inconclusive" | "errored" => Some(Self::Unsettled),
            _ => None,
        }
    }
}

/// What became of the reach one perturbed control could have recorded, as the engine wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// It was not asked to record.
    NotAsked,
    /// It did not pass, so its reach was not read.
    NotRead,
    /// Its process could not write what its guards reached.
    Unrecorded,
    /// What it wrote did not read back.
    Unreadable,
    /// What its guards recorded.
    Recorded(Touch),
}

/// One perturbed control, as the engine wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Perturbed {
    /// The target.
    pub target: String,
    /// What it was started with.
    pub started: Started,
    /// How it ended.
    pub ended: Ended,
    /// The tests that failed, as the harness named them.
    pub failed: BTreeSet<String>,
    /// What became of its reach.
    pub reach: Reach,
}

/// Every perturbed control of one engine recording, and how many it could not read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Perturbations {
    /// Every record that carried what a re-derivation needs.
    pub controls: Vec<Perturbed>,
    /// How many records lacked it, which a re-derivation cannot count as agreement.
    pub unreadable: usize,
}

/// Every perturbed control of one engine recording.
///
/// # Errors
/// A non-empty line that is not JSON rejects the whole recording.
pub fn read(recorded: &str) -> Result<Perturbations, crate::route::ReadError> {
    let mut read = Perturbations::default();
    for event in crate::route::events(recorded)? {
        if event.get("type").and_then(Value::as_str) != Some("perturbed-control") {
            continue;
        }
        match event.get("perturbed").and_then(perturbed) {
            Some(one) => read.controls.push(one),
            None => read.unreadable = read.unreadable.saturating_add(1),
        }
    }
    Ok(read)
}

fn perturbed(record: &Value) -> Option<Perturbed> {
    let target = record.get("target")?.as_str()?.to_owned();
    let reach = record.get("reach")?;
    let reach = match reach.get("state")?.as_str()? {
        "not-asked" => Reach::NotAsked,
        "not-read" => Reach::NotRead,
        "unrecorded" => Reach::Unrecorded,
        "unreadable" => Reach::Unreadable,
        "recorded" => {
            let touch = crate::drift::touch(reach.get("touch")?)?;
            (touch.target == target && touch.measured == Measured::Control)
                .then_some(Reach::Recorded(touch))?
        }
        _ => return None,
    };
    Some(Perturbed {
        started: started(record.get("perturbation")?)?,
        ended: Ended::parse(record.get("outcome")?.as_str()?)?,
        failed: strings(record.get("failed_tests")?)?.into_iter().collect(),
        reach,
        target,
    })
}

fn started(record: &Value) -> Option<Started> {
    Some(Started {
        environment: record
            .get("environment")?
            .as_array()?
            .iter()
            .map(|set| {
                let name = set.get("name")?.as_str()?.to_owned();
                let value = match set.get("value")? {
                    Value::Null => None,
                    text => Some(text.as_str()?.to_owned()),
                };
                Some((name, value))
            })
            .collect::<Option<Vec<_>>>()?,
        launcher: match record.get("launcher")? {
            Value::Null => None,
            text => Some(text.as_str()?.to_owned()),
        },
        arguments: strings(record.get("arguments")?)?,
        delayed: match record.get("delay")? {
            Value::Null => None,
            delay => Some(delay.get("site")?.as_u64()?),
        },
        confirms: match record.get("confirms")? {
            Value::Null => None,
            site => Some(site.as_u64()?),
        },
    })
}

fn strings(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|one| one.as_str().map(ToOwned::to_owned))
        .collect()
}

/// Why a perturbed control that passed established nothing to compare, by the name a report spells it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Because {
    /// Its process could not write what its guards reached.
    Unrecorded,
    /// What it wrote did not read back.
    Unreadable,
    /// It did not pass.
    ControlFailed,
    /// It passed other tests than its baseline did.
    OtherTests,
    /// The baseline recorded nothing to compare against.
    NoBaseline,
    /// The baseline passed only when run again.
    BaselineRetried,
    /// The tests one of the two runs was read as passing do not come to its own summary's count.
    Unparsed,
}

impl Because {
    /// The name a report spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unrecorded => "unrecorded",
            Self::Unreadable => "unreadable",
            Self::ControlFailed => "control-failed",
            Self::OtherTests => "other-tests",
            Self::NoBaseline => "no-baseline",
            Self::BaselineRetried => "baseline-retried",
            Self::Unparsed => "unparsed",
        }
    }
}

/// Why a perturbed control established nothing about its tests, by the name a report spells it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsettled {
    /// It did not run, stopped at a step limit, could not be read, or errored.
    Errored,
    /// It ran past its bound.
    Waited,
}

impl Unsettled {
    /// The name a report spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Errored => "errored",
            Self::Waited => "waited",
        }
    }
}

/// What one union gained and lost between a baseline and a control, by catalog index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// What only the control reported.
    pub gained: BTreeSet<u64>,
    /// What only the baseline reported.
    pub lost: BTreeSet<u64>,
}

impl Moved {
    fn between(baseline: &BTreeSet<u64>, control: &BTreeSet<u64>) -> Self {
        Self {
            gained: control.difference(baseline).copied().collect(),
            lost: baseline.difference(control).copied().collect(),
        }
    }
}

/// What moved in each of the three unions drift compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moves {
    /// The mutant sites.
    pub reached: Moved,
    /// The branch bodies entered.
    pub bodies: Moved,
    /// The mutations a guard saw its two branches differ over.
    pub infected: Moved,
}

/// What one perturbed control established against its target's baseline, re-derived from the two records alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Derived {
    /// It passed the tests its baseline passed and reached what its baseline did.
    Stable,
    /// It passed, and was not asked to record what it reached.
    Passed,
    /// A test failed, and these are the ones its harness named.
    Broke {
        /// The tests.
        failed: BTreeSet<String>,
    },
    /// It passed the tests its baseline passed and reached something else.
    Moved {
        /// What moved.
        reach: Box<Moves>,
    },
    /// It passed, and nothing of it could be compared, for every reason that holds.
    Uncompared {
        /// The reasons.
        because: BTreeSet<Because>,
    },
    /// It established nothing about its tests.
    Unsettled {
        /// Why.
        why: Unsettled,
    },
}

impl Derived {
    /// The state a report records it with.
    #[must_use]
    pub const fn state(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Passed => "passed",
            Self::Broke { .. } => "broke",
            Self::Moved { .. } => "moved",
            Self::Uncompared { .. } => "uncompared",
            Self::Unsettled { .. } => "unsettled",
        }
    }

    /// What the re-derivation found, as a clause a reader is told.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Self::Stable => "passed and reached what its baseline did".to_owned(),
            Self::Passed => "passed, and was not asked to record what it reached".to_owned(),
            Self::Broke { failed } => format!(
                "failed {}",
                if failed.is_empty() {
                    "with no test named".to_owned()
                } else {
                    failed.iter().cloned().collect::<Vec<_>>().join(", ")
                }
            ),
            Self::Moved { .. } => {
                "passed the tests its baseline passed and reached something else".to_owned()
            }
            Self::Uncompared { because } => format!(
                "passed and compared nothing, because of {}",
                because
                    .iter()
                    .map(|one| one.name())
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
            Self::Unsettled { why } => format!("settled nothing ({})", why.name()),
        }
    }
}

/// What `control` established against the baseline `touched` holds for its target.
#[must_use]
pub fn derived(control: &Perturbed, touched: &Touched) -> Derived {
    let uncompared = |because: Because| Derived::Uncompared {
        because: BTreeSet::from([because]),
    };
    match control.ended {
        Ended::Failed => Derived::Broke {
            failed: control.failed.clone(),
        },
        Ended::Waited => Derived::Unsettled {
            why: Unsettled::Waited,
        },
        Ended::Unsettled => Derived::Unsettled {
            why: Unsettled::Errored,
        },
        Ended::Passed => match &control.reach {
            Reach::NotAsked => Derived::Passed,
            Reach::NotRead => uncompared(Because::ControlFailed),
            Reach::Unrecorded => uncompared(Because::Unrecorded),
            Reach::Unreadable => uncompared(Because::Unreadable),
            Reach::Recorded(touch) => compared(touch, touched),
        },
    }
}

fn compared(control: &Touch, touched: &Touched) -> Derived {
    let baseline = touched
        .touches
        .iter()
        .rev()
        .find(|touch| touch.measured == Measured::Baseline && touch.target == control.target);
    let mut because = BTreeSet::new();
    if touched.retried.contains(&control.target) {
        because.insert(Because::BaselineRetried);
    }
    if !control.whole || baseline.is_some_and(|baseline| !baseline.whole) {
        because.insert(Because::Unparsed);
    }
    match baseline {
        None => {
            because.insert(Because::NoBaseline);
        }
        Some(baseline) if baseline.passed != control.passed => {
            because.insert(Because::OtherTests);
        }
        Some(_) => {}
    }
    let Some(baseline) = baseline.filter(|_| because.is_empty()) else {
        return Derived::Uncompared { because };
    };
    let reach = Moves {
        reached: Moved::between(&baseline.reached, &control.reached),
        bodies: Moved::between(&baseline.bodies, &control.bodies),
        infected: Moved::between(&baseline.infected, &control.infected),
    };
    if [&reach.reached, &reach.bodies, &reach.infected]
        .iter()
        .all(|moved| moved.gained.is_empty() && moved.lost.is_empty())
    {
        Derived::Stable
    } else {
        Derived::Moved {
            reach: Box::new(reach),
        }
    }
}

/// The test names a report's `failed` holds, or nothing where it is not a list of names.
#[must_use]
pub fn names(value: Option<&Value>) -> Option<BTreeSet<String>> {
    Some(strings(value?)?.into_iter().collect())
}

/// What a report's `reach` says moved in each union, or nothing where it does not say it in the shape a movement has.
#[must_use]
pub fn moves(value: Option<&Value>) -> Option<Moves> {
    let value = value?;
    let union = |name: &str| {
        let moved = value.get(name)?;
        let indices = |key: &str| -> Option<BTreeSet<u64>> {
            moved
                .get(key)?
                .as_array()?
                .iter()
                .map(Value::as_u64)
                .collect()
        };
        Some(Moved {
            gained: indices("gained")?,
            lost: indices("lost")?,
        })
    };
    Some(Moves {
        reached: union("reached")?,
        bodies: union("bodies")?,
        infected: union("infected")?,
    })
}
