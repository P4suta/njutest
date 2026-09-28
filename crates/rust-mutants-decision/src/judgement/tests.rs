// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec::Vec;

use super::{Account, Ending, Observed, judged};
use crate::evidence::{Detection, Doubt, Sealed};

fn every_observation() -> Vec<Observed> {
    let mut every = Vec::new();
    for ending in Ending::ALL {
        for account in Account::ALL {
            for beyond_control in [false, true] {
                every.push(Observed {
                    ending,
                    account,
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
    match (observed.ending, observed.account) {
        (Ending::Returned, Account::Passed) => Sealed::Passed,
        (Ending::Panicked, _) => Sealed::Detected(Detection::Panicked),
        (Ending::ExitedFailure, Account::Failed) => Sealed::Detected(Detection::Failed),
        (Ending::Aborted | Ending::Trapped, _) => Sealed::Detected(Detection::Trapped),
        (Ending::FuelExhausted, _) => Sealed::Detected(Detection::FuelExceeded),
        (Ending::MemoryExhausted, _) => Sealed::Detected(Detection::MemoryExceeded),
        (Ending::ExitedZero, _) => Sealed::Doubted(Doubt::ExitedEarly),
        (Ending::StackOverflow, _) => Sealed::Doubted(Doubt::StackOverflow),
        (Ending::Returned, Account::Failed | Account::Other)
        | (Ending::ExitedFailure, Account::Passed | Account::Other)
        | (Ending::ExitedOther, _) => Sealed::Doubted(Doubt::Unaccounted),
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
fn only_a_returned_instance_its_harness_accounted_for_passes() {
    for observed in every_observation() {
        if judged(observed) == Sealed::Passed {
            assert_eq!(
                (observed.ending, observed.account, observed.beyond_control),
                (Ending::Returned, Account::Passed, false),
                "{observed:?} passed"
            );
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
