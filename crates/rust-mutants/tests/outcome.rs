// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Outcomes: the default is "not run", the wire names are stable, and what caught a mutant is said in one place.

use rust_mutants::outcome::Outcome;

#[test]
fn the_default_outcome_is_not_run_so_a_forgotten_record_never_reads_as_a_kill() {
    assert_eq!(Outcome::default(), Outcome::NotRun);
    assert!(!Outcome::default().detected());
}

#[test]
fn wire_names_are_stable_snake_case_and_round_trip() {
    let names: Vec<&str> = Outcome::ALL.iter().map(|o| o.name()).collect();
    assert_eq!(
        names,
        [
            "not_run",
            "killed",
            "survived",
            "runaway",
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
fn a_clock_cannot_catch_a_mutant_and_a_count_can() {
    let detected: Vec<Outcome> = Outcome::ALL.into_iter().filter(|o| o.detected()).collect();
    assert_eq!(
        detected,
        [Outcome::Killed, Outcome::Runaway],
        "a bound expiring is a fact about the machine that watched, so two runs of one \
         catalogue on one commit would disagree about the score by how loaded each machine \
         was. A guard taken more times than the run allowed is a number every machine agrees \
         on, and it establishes that the mutation stopped the program terminating"
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
