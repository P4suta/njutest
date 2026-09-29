// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What became of the failures a fault made in one execution, read from the record its runtime kept (ADR 0032).

use crate::instrument::FAULT_FATE_SCHEMA;

/// How many failures one execution's fault made, how often something formatted one of them, and how many were dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fate {
    /// How many failures the fault made.
    pub made: u32,
    /// How many times something formatted one of them.
    pub read: u32,
    /// How many of them were dropped.
    pub dropped: u32,
}

impl Fate {
    /// Whether every failure made went nowhere a person or a test could read it: at least one made, every one dropped, none formatted.
    #[must_use]
    pub const fn absorbed(self) -> bool {
        self.made > 0 && self.read == 0 && self.dropped == self.made
    }
}

/// What an execution's record says became of the failures its fault made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fated {
    /// Every line is one the runtime writes about this catalog, counted.
    Recorded(Fate),
    /// There is no record: the fault made no failure, or made one of a type that carries no record.
    Absent,
    /// A line is not one the runtime writes about this catalog, or there are more than a count holds, so the record says nothing.
    Unreadable,
}

impl Fated {
    /// What the record `text` says, where every line must be one the runtime writes about `catalog`; an empty record is [`Fated::Absent`].
    #[must_use]
    pub fn read(text: &str, catalog: &str) -> Self {
        if text.is_empty() {
            return Self::Absent;
        }
        let mut fate = Fate {
            made: 0,
            read: 0,
            dropped: 0,
        };
        for line in text.split_terminator('\n') {
            let mut fields = line.split('\t');
            let (Some(schema), Some(about), Some(event), None) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                return Self::Unreadable;
            };
            if schema != FAULT_FATE_SCHEMA || about != catalog {
                return Self::Unreadable;
            }
            let count = match event {
                "made" => &mut fate.made,
                "read" => &mut fate.read,
                "dropped" => &mut fate.dropped,
                _ => return Self::Unreadable,
            };
            match count.checked_add(1) {
                Some(next) => *count = next,
                None => return Self::Unreadable,
            }
        }
        if text.ends_with('\n') {
            Self::Recorded(fate)
        } else {
            Self::Unreadable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Fate, Fated};

    const CATALOG: &str = "0123456789abcdef";

    fn line(event: &str) -> String {
        format!("rust-mutants-fate-v1\t{CATALOG}\t{event}\n")
    }

    #[test]
    fn a_record_counts_what_became_of_each_failure_and_says_nothing_it_cannot_stand_behind() {
        let absorbed = [line("made"), line("dropped")].concat();
        assert_eq!(
            Fated::read(&absorbed, CATALOG),
            Fated::Recorded(Fate {
                made: 1,
                read: 0,
                dropped: 1
            })
        );
        let shown = [line("made"), line("read"), line("dropped")].concat();
        assert_eq!(
            Fated::read(&shown, CATALOG),
            Fated::Recorded(Fate {
                made: 1,
                read: 1,
                dropped: 1
            })
        );
        assert_eq!(Fated::read("", CATALOG), Fated::Absent);
        for unreadable in [
            line("made").replace(CATALOG, "another"),
            line("made").replace("fate-v1", "fate-v2"),
            line("kept"),
            format!("{}extra\n", line("made").trim_end()),
            line("made").trim_end().to_owned(),
        ] {
            assert_eq!(
                Fated::read(&unreadable, CATALOG),
                Fated::Unreadable,
                "a line the runtime does not write says nothing: {unreadable:?}"
            );
        }
    }

    #[test]
    fn a_failure_is_absorbed_only_where_every_one_made_was_dropped_unread() {
        let fate = |made, read, dropped| Fate {
            made,
            read,
            dropped,
        };
        assert!(fate(1, 0, 1).absorbed());
        assert!(fate(3, 0, 3).absorbed());
        assert!(
            !fate(0, 0, 0).absorbed(),
            "a run that made no failure says nothing about one"
        );
        assert!(!fate(1, 1, 1).absorbed(), "a failure formatted was read");
        assert!(
            !fate(2, 0, 1).absorbed(),
            "a failure still held when the process ended may have gone anywhere"
        );
    }
}
