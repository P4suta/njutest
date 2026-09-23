// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The touch log: which of a test process's threads reached which guard.

use std::collections::{BTreeMap, BTreeSet};

/// The first token of a touch log's header line, which carries the recipe version.
pub const SCHEMA: &str = "rust-mutants-touch-v1";

/// The name a record takes when nothing about the thread that wrote it names a test.
pub const UNATTRIBUTED: &str = "-";

/// The first field of a record naming the mutant sites a thread reached.
pub(crate) const SITES: &str = "t";

/// The first field of a record naming the branch bodies a thread entered.
pub(crate) const BODIES: &str = "b";

/// The first field of a record naming the mutations a thread saw its guard's two branches differ over.
pub(crate) const INFECTED: &str = "i";

/// Why a touch log said nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TouchError {
    /// A line is neither a header nor a record.
    #[error("line {line}: {what}")]
    Malformed {
        /// The 1-based line.
        line: usize,
        /// What is wrong with it.
        what: String,
    },
    /// A header names a catalog this run is not about.
    #[error("line {line}: the log is about catalog {found}, and this run is about {expected}")]
    OtherCatalog {
        /// The 1-based line.
        line: usize,
        /// What the log says.
        found: String,
        /// What was asked for.
        expected: String,
    },
    /// A record names a site no mutant of the catalog answers to.
    #[error("line {line}: site {index} is beyond the {count} the catalog holds")]
    BeyondCatalog {
        /// The 1-based line.
        line: usize,
        /// The index the log named.
        index: u32,
        /// How many mutants there are.
        count: u32,
    },
    /// A record appeared before any header did.
    #[error("line {line}: a record before any header, so nothing says which catalog it is about")]
    Headless {
        /// The 1-based line.
        line: usize,
    },
}

/// One kind of thing the guards report, by the thread that reported it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct Seen {
    /// What each named thread reported.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tests: BTreeMap<String, BTreeSet<u32>>,
    /// What was reported on a thread no test answers for.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub loose: BTreeSet<u32>,
}

impl Seen {
    /// Whether anything of this target reported `index`.
    #[must_use]
    pub fn any(&self, index: u32) -> bool {
        self.loose.contains(&index) || self.tests.values().any(|held| held.contains(&index))
    }

    /// Whether the named test reported `index`, where a report nothing could attribute is one every test made.
    #[must_use]
    pub fn by(&self, test: &str, index: u32) -> bool {
        self.loose.contains(&index)
            || self
                .tests
                .get(test)
                .is_some_and(|held| held.contains(&index))
    }

    /// Every index anything of this target reported, whichever thread reported it.
    #[must_use]
    pub fn union(&self) -> BTreeSet<u32> {
        self.tests
            .values()
            .flat_map(|held| held.iter().copied())
            .chain(self.loose.iter().copied())
            .collect()
    }

    /// The named tests that reported `index`, in name order.
    #[must_use]
    pub fn who(&self, index: u32) -> Vec<String> {
        self.tests
            .iter()
            .filter(|(_, held)| held.contains(&index))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

/// What one test process's guards said about which of its threads reached them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Touches {
    /// The mutant sites each thread reached.
    pub reached: Seen,
    /// The branch bodies each thread entered, by the index of the marker at the body's first statement.
    pub bodies: Seen,
    /// The mutations each thread saw its guard's two branches differ over.
    pub infected: Seen,
}

/// What each test that passed reported, with everything a thread no passing test names reported folded into `loose`.
#[must_use]
pub fn attributed(recorded: Seen, ran: &[String]) -> Seen {
    let mut held = Seen {
        loose: recorded.loose,
        ..Seen::default()
    };
    for (thread, reported) in recorded.tests {
        if ran.iter().any(|test| test == &thread) {
            held.tests.extend([(thread, reported)]);
        } else {
            held.loose.extend(reported);
        }
    }
    held
}

/// Every touch the log records, gathered by the thread that made it.
///
/// # Errors
/// See [`TouchError`].
/// Every failure yields no facts at all, never the prefix that parsed.
pub fn read(text: &str, catalog: &str, count: u32) -> Result<Touches, TouchError> {
    let mut touches = Touches::default();
    let mut seen_header = false;
    for (position, line) in text.lines().enumerate() {
        let number = position.saturating_add(1);
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(SCHEMA) {
            seen_header = true;
            header(rest, catalog, number)?;
            continue;
        }
        if !seen_header {
            return Err(TouchError::Headless { line: number });
        }
        record(line, count, number, &mut touches)?;
    }
    Ok(touches)
}

/// The rest of a header line, which is the catalog digest the log is about.
fn header(rest: &str, catalog: &str, line: usize) -> Result<(), TouchError> {
    let mut fields = rest.split_whitespace();
    let (Some(found), None) = (fields.next(), fields.next()) else {
        return Err(TouchError::Malformed {
            line,
            what: "a header names one catalog".to_owned(),
        });
    };
    if found == catalog {
        return Ok(());
    }
    Err(TouchError::OtherCatalog {
        line,
        found: found.to_owned(),
        expected: catalog.to_owned(),
    })
}

/// One record: the kind of thing reached, the thread that reached it, and the indices.
fn record(line: &str, count: u32, number: usize, touches: &mut Touches) -> Result<(), TouchError> {
    let mut fields = line.split('\t');
    let (Some(kind), Some(name), Some(indices), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err(TouchError::Malformed {
            line: number,
            what: format!("{line:?} is not a kind, a thread, and a list of sites"),
        });
    };
    let seen = match kind {
        SITES => &mut touches.reached,
        BODIES => &mut touches.bodies,
        INFECTED => &mut touches.infected,
        _ => {
            return Err(TouchError::Malformed {
                line: number,
                what: format!("{kind:?} is not a kind of touch this reader knows"),
            });
        }
    };
    let reported = sites(indices, count, number)?;
    let into = if name == UNATTRIBUTED {
        &mut seen.loose
    } else {
        seen.tests.entry(name.to_owned()).or_default()
    };
    into.extend(reported);
    Ok(())
}

/// The comma-separated indices of one record, each within the catalog.
fn sites(indices: &str, count: u32, line: usize) -> Result<BTreeSet<u32>, TouchError> {
    let mut reached = BTreeSet::new();
    for field in indices.split(',') {
        let index = field
            .parse::<u32>()
            .map_err(|_error| TouchError::Malformed {
                line,
                what: format!("{field:?} is not a site"),
            })?;
        if index >= count {
            return Err(TouchError::BeyondCatalog { line, index, count });
        }
        reached.insert(index);
    }
    Ok(reached)
}

/// The header one process writes before its first record.
#[must_use]
pub fn header_line(catalog: &str) -> String {
    format!("{SCHEMA} {catalog}\n")
}

/// A target whose guards said nothing this run can route by, so every test of it reaches every mutation in it.
pub use crate::limitation::TOUCH_NOT_RECORDED as UNRECORDED;

/// A target whose record did not read back, so nothing of it is believed.
pub use crate::limitation::TOUCH_LOG_UNREADABLE as UNREADABLE;

/// What the guards of a whole run said, target by target.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct Touched {
    /// What each target's guards recorded, by target identity.
    pub targets: BTreeMap<String, TargetTouches>,
    /// Why a target the run built is not in `targets`, as `<limitation>:<target>`.
    pub limitations: Vec<String>,
    /// Which of the records above are facts about which mutant, without which the rest is only silence.
    #[serde(default)]
    pub narrowing: Narrowing,
}

/// What a reader has to know before this record narrows anything: which mutants the tree that made it could say something about.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct Narrowing {
    /// Every mutant whose guard in the tree that ran compares its two branches, so `infected` is a fact about it.
    pub compared: BTreeSet<u32>,
    /// The marker each mutant's branch proof rests on, where the instrumenter wrote that marker, so `bodies` is a fact about it.
    pub bodies: BTreeMap<u32, u32>,
}

/// What one target's guards recorded, and which of its tests ran to record it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct TargetTouches {
    /// The mutant sites each test of this target reached.
    #[serde(default)]
    pub reached: Seen,
    /// The branch bodies each test of this target entered, by the index of the marker at the body's first statement.
    #[serde(default)]
    pub bodies: Seen,
    /// The mutations each test of this target saw its guard's two branches differ over.
    #[serde(default)]
    pub infected: Seen,
    /// Every test the baseline ran, which is what a report nothing could attribute is about and what "all of them" counts against.
    pub ran: Vec<String>,
}

impl TargetTouches {
    /// What one whole-target process's guards recorded, attributed to the tests it passed.
    #[must_use]
    pub fn of(recorded: Touches, ran: &[String]) -> Self {
        Self {
            reached: attributed(recorded.reached, ran),
            bodies: attributed(recorded.bodies, ran),
            infected: attributed(recorded.infected, ran),
            ran: ran.to_vec(),
        }
    }
}

/// What one kind of report a control made that its baseline did not, and what the baseline made that the control did not.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Moved {
    /// What only the control reported.
    pub gained: BTreeSet<u32>,
    /// What only the baseline reported.
    pub lost: BTreeSet<u32>,
}

impl Moved {
    /// What moved between the two unions.
    fn between(baseline: &Seen, control: &Seen) -> Self {
        let (before, after) = (baseline.union(), control.union());
        Self {
            gained: after.difference(&before).copied().collect(),
            lost: before.difference(&after).copied().collect(),
        }
    }

    /// Whether the two unions were the same.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.gained.is_empty() && self.lost.is_empty()
    }
}

/// How a whole target's reach on a control differed from its reach on the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReachMoved {
    /// The mutant sites.
    pub reached: Moved,
    /// The branch bodies entered.
    pub bodies: Moved,
    /// The mutations a guard saw its two branches differ over.
    pub infected: Moved,
}

/// How a control's per-target unions differ from the baseline's, or nothing where all three agree; that both passed the same tests is the caller's premise (ADR 0025).
#[must_use]
pub fn unions_differ(baseline: &TargetTouches, control: &TargetTouches) -> Option<ReachMoved> {
    let moved = ReachMoved {
        reached: Moved::between(&baseline.reached, &control.reached),
        bodies: Moved::between(&baseline.bodies, &control.bodies),
        infected: Moved::between(&baseline.infected, &control.infected),
    };
    if moved.reached.is_empty() && moved.bodies.is_empty() && moved.infected.is_empty() {
        return None;
    }
    Some(moved)
}

/// What an original-code control of one whole target established about whether its baseline reach holds.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    variant_size_differences,
    reason = "a moved reach carries what moved and the other two carry nothing or one closed \
              reason; boxing it moves the difference behind a pointer rather than removing it"
)]
pub enum Steadiness {
    /// A control that passed the tests the baseline passed reached exactly what the baseline did.
    Held,
    /// A control that passed the tests the baseline passed reached something else.
    Moved(ReachMoved),
    /// Nothing about it could be compared, and why.
    NotMeasured(Unmeasured),
}

/// Why a control established nothing about whether a target's baseline reach holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Unmeasured {
    /// The control's process could not record what its guards reached.
    Unrecorded,
    /// The control's record did not read back.
    Unreadable,
    /// The control did not pass, so its reach is the reach of a failing run.
    ControlFailed,
    /// The control passed other tests than the baseline did, so its reach is the reach of other tests.
    OtherTests,
    /// The baseline recorded nothing for the target to be compared against.
    NoBaseline,
    /// The baseline passed only when run again in the directory its first attempt left, so it did not run under the conditions a control does.
    BaselineRetried,
    /// The tests one of the two runs was read as passing do not come to its own summary's count, so which tests passed is the parser's answer and not the harness's.
    Unparsed,
}

impl Unmeasured {
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

/// Which of a target's tests could have noticed one mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reaching {
    /// Nothing of this target reached the site, so running it would establish nothing.
    Nothing,
    /// Exactly these tests reached it.
    Tests(Vec<String>),
    /// The site was reached where nothing could be attributed, so every test of the target reaches it.
    Whole,
}

impl TargetTouches {
    /// Which of this target's tests reached `index`.
    #[must_use]
    pub fn reaching(&self, index: u32) -> Reaching {
        if self.reached.loose.contains(&index) {
            return Reaching::Whole;
        }
        let named = self.reached.who(index);
        if named.is_empty() {
            return Reaching::Nothing;
        }
        if named.len() >= self.ran.len() {
            return Reaching::Whole;
        }
        Reaching::Tests(named)
    }
}

impl Touched {
    /// Whether anything at all was recorded, which is what makes routing by the guards possible.
    #[must_use]
    pub fn measured(&self) -> bool {
        !self.targets.is_empty()
    }

    /// Which of `target`'s tests could have noticed the mutation at `index`, or nothing when this run cannot say.
    #[must_use]
    pub fn reaching(&self, target: &str, index: u32) -> Option<Reaching> {
        Some(self.targets.get(target)?.reaching(index))
    }

    /// Records that `target` said nothing this run can route by, and why.
    pub fn limited(&mut self, limitation: &str, target: &str) {
        self.limitations.push(format!("{limitation}:{target}"));
    }
}
