// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Outcomes: the default is "not run", the wire names are stable, and only a kill or a confirmed timeout counts as detection.

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
            "timed_out",
            "inconclusive",
            "errored"
        ]
    );
    for outcome in Outcome::ALL {
        assert_eq!(Outcome::parse(outcome.name()), Some(outcome));
        assert_eq!(outcome.to_string(), outcome.name());
    }
    assert_eq!(
        Outcome::parse("timed-out"),
        None,
        "the report spelling is not the wire spelling"
    );
    assert_eq!(Outcome::parse("KILLED"), None);
}

#[test]
fn only_a_kill_or_a_confirmed_timeout_is_a_detection() {
    let detected: Vec<Outcome> = Outcome::ALL.into_iter().filter(|o| o.detected()).collect();
    assert_eq!(detected, [Outcome::Killed, Outcome::TimedOut]);
}
