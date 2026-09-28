// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec::Vec;

use super::{Account, Ending, Harness, Observed, judged};
use crate::evidence::{Detection, Doubt, Sealed};

fn every_harness() -> Vec<Harness> {
    let mut every: Vec<Harness> = Account::ALL.into_iter().map(Harness::Libtest).collect();
    every.push(Harness::Doctest);
    every.push(Harness::ShouldPanic);
    every
}

fn every_observation() -> Vec<Observed> {
    let mut every = Vec::new();
    for ending in Ending::ALL {
        for harness in every_harness() {
            for beyond_control in [false, true] {
                every.push(Observed {
                    ending,
                    harness,
                    beyond_control,
                });
            }
        }
    }
    every
}

fn by_the_table(observed: Observed) -> Sealed {
    if observed.beyond_control {
        return Sealed::Doubted(Doubt::Refused);
    }
    match (observed.harness, observed.ending) {
        (Harness::Libtest(Account::Passed) | Harness::Doctest, Ending::Returned)
        | (
            Harness::ShouldPanic,
            Ending::ExitedFailure | Ending::Panicked | Ending::Aborted | Ending::Trapped,
        ) => Sealed::Passed,
        (Harness::Libtest(_) | Harness::Doctest, Ending::Panicked) => {
            Sealed::Detected(Detection::Panicked)
        }
        (Harness::Libtest(Account::Failed) | Harness::Doctest, Ending::ExitedFailure)
        | (Harness::ShouldPanic, Ending::Returned) => Sealed::Detected(Detection::Failed),
        (Harness::Libtest(_) | Harness::Doctest, Ending::Aborted | Ending::Trapped) => {
            Sealed::Detected(Detection::Trapped)
        }
        (_, Ending::FuelExhausted) => Sealed::Detected(Detection::FuelExceeded),
        (Harness::Libtest(_) | Harness::Doctest, Ending::MemoryExhausted) => {
            Sealed::Detected(Detection::MemoryExceeded)
        }
        (Harness::ShouldPanic, Ending::MemoryExhausted) => Sealed::Doubted(Doubt::Refused),
        (_, Ending::ExitedZero) => Sealed::Doubted(Doubt::ExitedEarly),
        (_, Ending::StackOverflow) => Sealed::Doubted(Doubt::StackOverflow),
        (Harness::Libtest(Account::Failed | Account::Other), Ending::Returned)
        | (Harness::Libtest(Account::Passed | Account::Other), Ending::ExitedFailure)
        | (_, Ending::ExitedOther) => Sealed::Doubted(Doubt::Unaccounted),
    }
}

#[test]
fn every_observation_is_judged_as_the_table_in_sealed_md_says() {
    let disagreeing: Vec<(Observed, Sealed, Sealed)> = every_observation()
        .into_iter()
        .map(|observed| (observed, judged(observed), by_the_table(observed)))
        .filter(|(_, said, ruled)| said != ruled)
        .collect();
    assert!(
        disagreeing.is_empty(),
        "{} of {} observations are judged against the table, the first {:?}",
        disagreeing.len(),
        every_observation().len(),
        disagreeing.first()
    );
}

#[test]
fn only_the_ending_its_harness_passes_by_passes() {
    for observed in every_observation() {
        if judged(observed) == Sealed::Passed {
            let passing = match observed.harness {
                Harness::Libtest(account) => {
                    observed.ending == Ending::Returned && account == Account::Passed
                }
                Harness::Doctest => observed.ending == Ending::Returned,
                Harness::ShouldPanic => matches!(
                    observed.ending,
                    Ending::ExitedFailure | Ending::Panicked | Ending::Aborted | Ending::Trapped
                ),
            };
            assert!(passing && !observed.beyond_control, "{observed:?} passed");
        }
    }
}

#[test]
fn a_judgement_that_reads_an_early_exit_as_a_pass_is_caught_by_the_table() {
    let planted = |observed: Observed| match observed.ending {
        Ending::ExitedZero => Sealed::Passed,
        Ending::Returned
        | Ending::ExitedFailure
        | Ending::ExitedOther
        | Ending::Panicked
        | Ending::Aborted
        | Ending::Trapped
        | Ending::StackOverflow
        | Ending::FuelExhausted
        | Ending::MemoryExhausted => judged(observed),
    };
    assert!(
        every_observation()
            .into_iter()
            .any(|observed| planted(observed) != by_the_table(observed)),
        "the table did not catch an early exit read as a pass"
    );
}

#[test]
fn a_judgement_that_reads_a_should_panic_doctest_as_any_other_is_caught_by_the_table() {
    let planted = |observed: Observed| match observed.harness {
        Harness::ShouldPanic => judged(Observed {
            harness: Harness::Doctest,
            ..observed
        }),
        Harness::Libtest(_) | Harness::Doctest => judged(observed),
    };
    assert!(
        every_observation()
            .into_iter()
            .any(|observed| planted(observed) != by_the_table(observed)),
        "the table did not catch a doctest that should panic read as one that should return"
    );
}
