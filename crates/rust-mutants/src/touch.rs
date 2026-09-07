// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The touch log: which of a test process's threads reached which guard.
//!
//! Every guard an instrumented tree carries already asks the runtime whether it
//! is the live one, and libtest gives each test a thread of its own named after
//! it (`library/test/src/lib.rs`, `thread::Builder::new().name(…)`, on every
//! platform that has threads and at every concurrency level). So the guards can
//! say which test reached them, and the run that asks them costs nothing but the
//! baseline execution it was already going to do.
//!
//! A thread the process cannot name after a test — the main thread, a benchmark,
//! one a test spawned for itself — is recorded as `-`. Nothing was attributed to
//! it, so everything it reached has to reach every test of its target: the
//! fallback is toward running more, never less.
//!
//! Why the guards rather than a coverage build:
//! [ADR 0014](../../../docs/adr/0014-the-guards-are-the-measurement.md).
//!
//! The reader is fail-closed, for the reason [`crate::probe::log`] is: a
//! truncated line or a header naming another catalog yields no facts at all,
//! because a smaller wrong answer is what a partially-written log looks like and
//! acting on one would skip a test that could have killed something.

use std::collections::{BTreeMap, BTreeSet};

/// The first token of a touch log's header line, which carries the recipe version.
pub const SCHEMA: &str = "rust-mutants-touch-v1";

/// The name a record takes when nothing about the thread that wrote it names a test.
pub const UNATTRIBUTED: &str = "-";

/// The first field of a record naming the mutant sites a thread reached.
pub(crate) const SITES: &str = "t";

/// The first field of a record naming the branch bodies a thread entered.
pub(crate) const BODIES: &str = "b";

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

/// What one test process's guards said about which of its threads reached them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Touches {
    /// The mutant sites each named thread reached, which is each test that ran.
    pub tests: BTreeMap<String, BTreeSet<u32>>,
    /// The branch bodies each named thread entered, by the index of the marker at the body's first statement.
    pub bodies: BTreeMap<String, BTreeSet<u32>>,
    /// The mutant sites reached on a thread no test answers for.
    pub loose: BTreeSet<u32>,
    /// The branch bodies entered on a thread no test answers for.
    pub loose_bodies: BTreeSet<u32>,
}

/// Every touch the log records, gathered by the thread that made it.
///
/// `catalog` is the digest this run is about and `count` how many mutants it
/// holds; both are what makes a stale log say nothing rather than something
/// wrong. A thread that filled more than one line is one entry, because a long
/// list is written in pieces that each repeat the name.
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
    if kind != SITES && kind != BODIES {
        return Err(TouchError::Malformed {
            line: number,
            what: format!("{kind:?} is not a kind of touch this reader knows"),
        });
    }
    let reached = sites(indices, count, number)?;
    let into = match (kind, name == UNATTRIBUTED) {
        (SITES, true) => &mut touches.loose,
        (SITES, false) => touches.tests.entry(name.to_owned()).or_default(),
        (_, true) => &mut touches.loose_bodies,
        (_, false) => touches.bodies.entry(name.to_owned()).or_default(),
    };
    into.extend(reached);
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
}

/// What one target's guards recorded, and which of its tests ran to record it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct TargetTouches {
    /// The mutant sites each test of this target reached.
    pub tests: BTreeMap<String, BTreeSet<u32>>,
    /// The branch bodies each test of this target entered, by the index of the marker at the body's first statement.
    #[serde(default)]
    pub bodies: BTreeMap<String, BTreeSet<u32>>,
    /// The sites reached where nothing named a test, which therefore reach every test of the target.
    pub loose: BTreeSet<u32>,
    /// The bodies entered where nothing named a test, which therefore were entered by every test of the target.
    #[serde(default)]
    pub loose_bodies: BTreeSet<u32>,
    /// Every test the baseline ran, which is what a loose site reaches and what "all of them" counts against.
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
    /// Whether anything of this target entered the body `marker` names.
    ///
    /// A body entered where nothing named a test was entered as far as this
    /// target is concerned: the record could not say by which of its tests, and
    /// the answer to not knowing is that it might have been any of them.
    #[must_use]
    pub fn entered(&self, marker: u32) -> bool {
        self.loose_bodies.contains(&marker)
            || self
                .bodies
                .values()
                .any(|entered| entered.contains(&marker))
    }

    /// Whether the named test of this target entered the body `marker` names.
    #[must_use]
    pub fn entered_by(&self, test: &str, marker: u32) -> bool {
        self.loose_bodies.contains(&marker)
            || self
                .bodies
                .get(test)
                .is_some_and(|entered| entered.contains(&marker))
    }

    /// Which of this target's tests reached `index`.
    ///
    /// A site recorded on a thread nothing names a test after is reached by
    /// every test of the target: the measurement could not say which one, and
    /// the answer to that is to run more rather than fewer. A target that ran
    /// tests this reader never saw named — a harness of its own, an output
    /// this engine does not parse — has an empty `ran`, and then a site
    /// nothing was attributed to is the whole target as well.
    #[must_use]
    pub fn reaching(&self, index: u32) -> Reaching {
        if self.loose.contains(&index) {
            return Reaching::Whole;
        }
        let named: Vec<String> = self
            .tests
            .iter()
            .filter(|(_, sites)| sites.contains(&index))
            .map(|(name, _)| name.clone())
            .collect();
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
