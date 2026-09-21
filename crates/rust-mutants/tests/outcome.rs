// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Outcomes: the wire names are stable, and what caught a mutant is said in one place.

use rust_mutants::outcome::Outcome;

#[test]
fn wire_names_are_stable_snake_case_and_round_trip() {
    let names: Vec<&str> = Outcome::ALL.iter().map(|o| o.name()).collect();
    assert_eq!(
        names,
        [
            "not_run",
            "killed",
            "survived",
            "step_limit_reached",
            "waited",
            "inconclusive",
            "errored"
        ]
    );
    for outcome in Outcome::ALL {
        assert_eq!(Outcome::parse(outcome.name()), Some(outcome));
        assert_eq!(outcome.to_string(), outcome.name());
    }
    assert_eq!(
        Outcome::parse("timed_out"),
        None,
        "a clock and a count stopped a process for different reasons and the older name said \
         neither, so a record written by an older run is one a reader is told about rather \
         than one silently read as a detection"
    );
    assert_eq!(Outcome::parse("KILLED"), None);
}

#[test]
fn neither_a_clock_nor_a_step_limit_proves_detection() {
    let detected: Vec<Outcome> = Outcome::ALL.into_iter().filter(|o| o.detected()).collect();
    assert_eq!(
        detected,
        [Outcome::Killed],
        "a bound expiring is a fact about the machine that watched, and reaching a finite \
         guard-take allowance establishes only where this execution stopped. Neither proves \
         that the mutation cannot terminate, so neither is a detection"
    );
}

#[test]
fn one_place_says_which_outcomes_are_detections() {
    for outcome in Outcome::ALL {
        assert_eq!(
            outcome.detected(),
            outcome.noticed().is_some(),
            "two layers each answering which outcomes are caught is two answers, and this \
             repository had them: the engine counted a timeout as caught while the report \
             said an expired bound establishes nothing, about the same mutation in the same \
             run"
        );
    }
}
