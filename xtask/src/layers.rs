// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How far each layer of an independent re-decision got, which every audit states for every layer it has.

use std::fmt;

/// How far one layer's re-decision got with one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Everything the run owes the layer was re-decided.
    Rederived,
    /// Something the run owes the layer could not be re-decided, and an unaudited line says what.
    Partly,
    /// The run holds nothing this layer re-decides, and why.
    Absent(&'static str),
}

impl fmt::Display for Coverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rederived => f.write_str("re-decided"),
            Self::Partly => f.write_str("partly re-decided; the unaudited lines say what was not"),
            Self::Absent(why) => write!(f, "nothing to re-decide: {why}"),
        }
    }
}

/// A subject's complete absence, created only by a checked inventory or explicit configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Absence {
    reason: &'static str,
}

impl Absence {
    /// Why the independently closed subject is not owed.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        self.reason
    }
}

/// The declared subject count, independently enumerated subjects and retained observations.
#[derive(Debug, Clone, Copy)]
pub struct Inventory {
    /// The count explicitly present in the complete input.
    pub declared: Option<u64>,
    /// The subjects independently enumerated from the input's rows.
    pub subjects: Option<u64>,
    /// The subjects independently observed in the other retained input.
    pub observed: ObservedSubjects,
}

/// A retained count, an explicitly unrequested observation, or evidence still missing.
#[derive(Debug, Clone, Copy)]
pub enum ObservedSubjects {
    /// The complete retained input independently enumerated these subjects.
    Counted(u64),
    /// The complete request declares that this observation was not required.
    Unrequested,
    /// A required observation or representable count is missing.
    Missing,
}

/// Whether a complete configuration requests the subject.
#[derive(Debug, Clone, Copy)]
pub enum Request {
    /// This configuration requests evidence about the subject.
    Requested,
    /// This configuration explicitly leaves the subject unrequested.
    Unrequested,
}

/// The configured targets and every target the producer accounted for.
#[derive(Debug)]
pub struct Configuration<'a> {
    /// The complete configured target inventory.
    pub requested: &'a std::collections::BTreeSet<&'a str>,
    /// The targets the actual producer recorded.
    pub recorded: &'a std::collections::BTreeSet<&'a str>,
    /// The configured targets the actual producer explicitly declined to record.
    pub excepted: &'a std::collections::BTreeSet<&'a str>,
}

/// An independently derived closed subject or evidence still owed, matched exhaustively by readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// A checked complete inventory or explicit configuration proves this absence.
    NothingOwed(Absence),
    /// The subject cannot be closed from the retained inputs.
    Missing,
}

impl Closed {
    /// Closes an empty subject only when its declaration, rows and retained observations all agree.
    #[must_use]
    pub const fn empty(inventory: Inventory, reason: &'static str) -> Self {
        if !matches!(inventory.declared, Some(0)) || !matches!(inventory.subjects, Some(0)) {
            return Self::Missing;
        }
        match inventory.observed {
            ObservedSubjects::Counted(0) | ObservedSubjects::Unrequested => {
                Self::NothingOwed(Absence { reason })
            }
            ObservedSubjects::Counted(_) | ObservedSubjects::Missing => Self::Missing,
        }
    }

    /// Closes an explicitly unrequested subject independently of unrelated missing evidence.
    #[must_use]
    pub const fn requested(request: Request, reason: &'static str) -> Self {
        match request {
            Request::Requested => Self::Missing,
            Request::Unrequested => Self::NothingOwed(Absence { reason }),
        }
    }

    /// Closes a producer observation only when it accounts for every configured target and no foreign one.
    #[must_use]
    pub fn configured(configuration: &Configuration<'_>, reason: &'static str) -> Self {
        let accounted: std::collections::BTreeSet<_> = configuration
            .recorded
            .union(configuration.excepted)
            .copied()
            .collect();
        if !configuration.recorded.is_empty()
            && configuration.recorded.is_disjoint(configuration.excepted)
            && accounted == *configuration.requested
        {
            Self::NothingOwed(Absence { reason })
        } else {
            Self::Missing
        }
    }
}
