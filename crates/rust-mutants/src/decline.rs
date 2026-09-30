// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a test process wrote, on the file the engine named for it, about the tests of it that could not measure where they ran (ADR 0043).

use std::collections::BTreeSet;

pub use rust_mutants_adapt::decline::{Decline, Declines, Unbelieved};

use crate::execute::{MutantConclusion, Reading};

/// The variable that names, to every test process the engine starts, the file a test that cannot measure there appends a line to.
pub const DECLINE_NOTICE_ENV: &str = "RUST_MUTANTS_DECLINE_NOTICE";

/// The name of that file, in the engine's own directory for the process.
pub const DECLINE_NOTICE_FILE: &str = "decline-notice";

/// What the notice at `path` says of the process that could write it, or that no test declined where the engine named no notice or the process wrote none.
#[must_use]
pub fn of(path: Option<&std::path::Path>, reading: Reading, passed: &[String]) -> Declines {
    Declines::answered(path.map(crate::runner::read_side_channel), reading, passed)
}

/// Whether an answer resting on `executions` may be kept for another run: not where any test declined or a notice was not believed, since an answer established where a test could not measure is the machine's, and read back where it could, it would pass the machine off as the tree (ADR 0043).
#[must_use]
pub fn storable(executions: &[crate::execute::MutantResult]) -> bool {
    executions.iter().all(|one| one.declines.is_silent())
}

/// Takes away whatever notice an earlier process left at `path`, so what is read there after the next one is only that one's.
///
/// # Errors
/// What removing it said, where it was there and could not be removed.
pub fn cleared(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// A decline conclusion that disagrees with the process and baseline evidence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("decline conclusion {decided:?} disagrees with the process notice or baseline")]
pub(crate) struct DecisionMismatchError {
    decided: MutantConclusion,
}

/// Checks the execution's decline conclusion against the notice, the whole-process reading, and the baseline independently of the classifier.
pub(crate) fn checked_decision(
    notice: &Declines,
    process: (Reading, &[String]),
    baseline: &[Decline],
    decided: &MutantConclusion,
) -> Result<(), DecisionMismatchError> {
    let (reading, passed) = process;
    let valid = match notice {
        Declines::Unbelieved { .. } => *decided == MutantConclusion::Errored,
        Declines::Read { declined, .. } => {
            let mut named = BTreeSet::new();
            let whole = declined.is_empty()
                || (reading == Reading::Whole
                    && declined
                        .iter()
                        .all(|one| passed.contains(&one.test) && named.insert(one.test.as_str())));
            if !whole {
                false
            } else if let Some(changed) = declined.iter().find(|one| !baseline.contains(one)) {
                matches!(decided, MutantConclusion::DeclinedUnderTheMutant { by } if by == changed)
            } else if !declined.is_empty() && declined.len() == passed.len() {
                matches!(decided, MutantConclusion::Declined { tests } if tests == declined)
            } else {
                *decided == MutantConclusion::Survived
            }
        }
    };
    if valid {
        Ok(())
    } else {
        Err(DecisionMismatchError {
            decided: decided.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Decline, Declines, checked_decision};
    use crate::execute::{MutantConclusion, Reading};

    #[test]
    fn a_planted_wrong_execution_decline_decision_is_refused() {
        let first = Decline {
            test: "tests::a".to_owned(),
            why: "no network".to_owned(),
        };
        let changed = Decline {
            test: "tests::a".to_owned(),
            why: "different words".to_owned(),
        };
        let baseline = [first.clone()];
        let notice = |one| Declines::Read {
            declined: vec![one],
            quoted: Vec::new(),
        };
        let matching = notice(first.clone());
        let new_words = notice(changed.clone());
        let candidates = [
            MutantConclusion::Survived,
            MutantConclusion::Errored,
            MutantConclusion::Declined {
                tests: vec![first.clone()],
            },
            MutantConclusion::DeclinedUnderTheMutant {
                by: changed.clone(),
            },
        ];
        for (notice, reading, passed, expected) in [
            (
                matching.clone(),
                Reading::Whole,
                vec!["tests::a".to_owned()],
                MutantConclusion::Declined { tests: vec![first] },
            ),
            (
                matching.clone(),
                Reading::Whole,
                vec!["tests::a".to_owned(), "tests::b".to_owned()],
                MutantConclusion::Survived,
            ),
            (
                new_words,
                Reading::Whole,
                vec!["tests::a".to_owned()],
                MutantConclusion::DeclinedUnderTheMutant { by: changed },
            ),
            (
                Declines::Unbelieved {
                    because: super::Unbelieved::ReadingNotWhole,
                },
                Reading::Short,
                vec!["tests::a".to_owned()],
                MutantConclusion::Errored,
            ),
        ] {
            assert!(
                checked_decision(&notice, (reading, &passed), &baseline, &expected).is_ok(),
                "the actual {expected:?} was refused for {notice:?} under {reading:?}"
            );
            for planted in &candidates {
                if *planted == expected {
                    continue;
                }
                assert!(
                    checked_decision(&notice, (reading, &passed), &baseline, planted).is_err(),
                    "a planted {planted:?} passed for {notice:?} under {reading:?}"
                );
            }
        }
        for planted in &candidates {
            assert!(
                checked_decision(
                    &matching,
                    (Reading::Short, &["tests::a".to_owned()]),
                    &baseline,
                    planted,
                )
                .is_err(),
                "a planted {planted:?} passed for a notice from an incomplete process"
            );
        }
    }
}
