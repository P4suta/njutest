// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use rust_mutants_decision::evidence::{Detection, Sealed};
use rust_mutants_decision::judgement::{Account, Ending, Harness, Observed, judged};

use super::judgement_keeps_the_pass_rule;

fn every_observation() -> Vec<Observed> {
    let mut harnesses: Vec<Harness> = Account::ALL.into_iter().map(Harness::Libtest).collect();
    harnesses.push(Harness::Doctest);
    harnesses.push(Harness::ShouldPanic);
    let mut every = Vec::new();
    for ending in Ending::ALL {
        for harness in &harnesses {
            for beyond_control in [false, true] {
                for matched in [true, false] {
                    every.push(Observed {
                        ending,
                        harness: *harness,
                        beyond_control,
                        matched,
                    });
                }
            }
        }
    }
    every
}

#[test]
fn every_judgement_the_engine_makes_keeps_the_rule_a_pass_keeps() {
    let broken: Vec<Observed> = every_observation()
        .into_iter()
        .filter(|observed| !judgement_keeps_the_pass_rule(*observed, judged(*observed)))
        .collect();
    assert!(
        broken.is_empty(),
        "the self-check stops a run only on a judgement that broke the rule: {broken:?}"
    );
}

#[test]
fn a_judgement_that_passed_what_does_not_pass_or_failed_what_does_stops_the_run() {
    for observed in every_observation() {
        let honest = judged(observed);
        let planted = if honest == Sealed::Passed {
            Sealed::Detected(Detection::Failed)
        } else {
            Sealed::Passed
        };
        assert!(
            !judgement_keeps_the_pass_rule(observed, planted),
            "a planted judgement {planted:?} of {observed:?}, where the rule gives {honest:?}, \
             is refused"
        );
    }
}
