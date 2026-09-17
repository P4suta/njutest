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

/// Every touch the log records, gathered by the thread that made it.
///
/// # Errors
/// See [`TouchError`]. Every failure yields no facts at all, never the prefix
/// that parsed.
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
        let index: u32 = field.parse().map_err(|_error| TouchError::Malformed {
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
pub struct Narrowing {
    /// Every mutant whose guard in the tree that ran compares its two branches, so `infected` is a fact about it.
    pub compared: BTreeSet<u32>,
    /// The marker each mutant's branch proof rests on, where the instrumenter wrote that marker, so `bodies` is a fact about it.
    pub bodies: BTreeMap<u32, u32>,
}

/// What one target's guards recorded, and which of its tests ran to record it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
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
