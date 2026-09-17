// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a dry run says a run would cost.

#![expect(
    clippy::panic,
    reason = "the helper that finds one line of a tally is not itself a test: a tally that \
              lost the line leaves nothing to read out of it"
)]

use std::time::Duration;

use rust_mutants_cli::app::estimate::Estimated;

/// The line of `text` that starts with `name`.
fn line<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .find(|line| line.starts_with(name))
        .unwrap_or_else(|| panic!("an estimate says {name}:\n{text}"))
}

#[test]
fn an_estimate_counts_the_pairs_a_run_would_start_against_the_pairs_there_are() {
    let counted = Estimated {
        cataloged: 10_u64.into(),
        selected: 4_u64.into(),
        pairs: 8_u64.into(),
        unreached: 24_u64.into(),
        discharged: 6_u64.into(),
        unselected: 2_u64.into(),
        nothing_to_ask: 4_u64.into(),
        tests: 30_u64.into(),
        tests_whole: 120_u64.into(),
        duration: Duration::from_secs(90),
    };
    let said = counted.said(rust_mutants::count::Count::new(4));

    assert!(
        line(&said, "WOULD START").contains("8 of 40 pairs (10 mutants against 4 targets)"),
        "the whole is every mutant against every target, which is the run nobody has to \
         do: {said}"
    );
    assert!(
        line(&said, "WOULD START").contains("80.0% removed"),
        "and the share is of that whole: {said}"
    );
    assert!(
        line(&said, "WHICH RUN").contains("30 of 120 tests; 75.0% removed"),
        "the tests are counted separately, because a pair is a target and a target is \
         however many tests its route names: {said}"
    );
    assert!(
        line(&said, "REMOVED BY")
            .contains("unreached=24 discharged=6 of the 32 pairs the route removed"),
        "every pair removed is removed by something a person can go and look at, and the \
         two numbers add up to the pairs the line above says were removed: {said}"
    );
    assert!(
        line(&said, "NEVER ASKED").contains("unselected=2 nothing-to-ask=4 of the 10 mutants"),
        "a mutant nothing asks about is not a pair that was removed, and a reader who \
         added it to the pairs was adding two different things: {said}"
    );
    assert!(
        line(&said, "AT MOST").contains('4'),
        "the mutants that execute are the ones with a target to ask: {said}"
    );
}

#[test]
fn a_catalog_with_nothing_in_it_removed_nothing_rather_than_everything() {
    let said = Estimated::default().said(rust_mutants::count::Count::new(0));
    assert!(
        line(&said, "WOULD START").contains("0 of 0 pairs")
            && line(&said, "WOULD START").contains("0.0% removed"),
        "a run with no mutants and no targets removed nothing: reading it as 100% is a \
         division nobody did, and it is the line a person reads as the proof layers \
         working: {said}"
    );
    assert!(
        line(&said, "WHICH RUN").contains("0.0% removed"),
        "and the same for the tests: {said}"
    );
    assert!(
        line(&said, "ROUGHLY").contains("0:00:00"),
        "and nothing takes no time: {said}"
    );
}

#[test]
fn a_run_that_removed_every_pair_says_so_and_a_run_that_removed_none_says_that() {
    let whole = Estimated {
        cataloged: 5_u64.into(),
        selected: 5_u64.into(),
        pairs: 10_u64.into(),
        tests: 40_u64.into(),
        tests_whole: 40_u64.into(),
        ..Estimated::default()
    };
    assert!(
        line(
            &whole.said(rust_mutants::count::Count::new(2)),
            "WOULD START"
        )
        .contains("0.0% removed"),
        "every pair a run would start is a run the proof layers did nothing for: {}",
        whole.said(rust_mutants::count::Count::new(2))
    );
    assert!(
        line(&whole.said(rust_mutants::count::Count::new(2)), "WHICH RUN").contains("0.0% removed"),
        "and the same of the tests: {}",
        whole.said(rust_mutants::count::Count::new(2))
    );

    let none = Estimated {
        cataloged: 5_u64.into(),
        nothing_to_ask: 5_u64.into(),
        unreached: 10_u64.into(),
        ..Estimated::default()
    };
    assert!(
        line(
            &none.said(rust_mutants::count::Count::new(2)),
            "WOULD START"
        )
        .contains("0 of 10 pairs")
            && line(
                &none.said(rust_mutants::count::Count::new(2)),
                "WOULD START"
            )
            .contains("100.0% removed"),
        "and a run that would start nothing removed all of it: {}",
        none.said(rust_mutants::count::Count::new(2))
    );
}

#[test]
fn the_time_is_a_guess_and_says_so_where_a_person_reads_it() {
    let hours = Estimated {
        duration: Duration::from_secs(3 * 3600 + 25 * 60 + 9),
        ..Estimated::default()
    };
    let said = hours.said(rust_mutants::count::Count::new(1));
    assert!(
        line(&said, "ROUGHLY").contains("3:25:09"),
        "a run of hours is read in hours, minutes and seconds rather than as a number of \
         seconds nobody converts: {said}"
    );
    assert!(
        line(&said, "ROUGHLY").contains("guess about the machine"),
        "and the line says it is a guess, because it is the only number here that is one: \
         {said}"
    );
}

#[test]
fn work_shorter_than_a_second_is_a_second_rather_than_none() {
    let barely = Estimated {
        duration: Duration::from_nanos(1),
        ..Estimated::default()
    };
    assert!(
        line(&barely.said(rust_mutants::count::Count::new(1)), "ROUGHLY").contains("0:00:01"),
        "work that is going to happen takes some time, and rounding it to nothing reads \
         as a run that would not start: {}",
        barely.said(rust_mutants::count::Count::new(1))
    );
    let nothing = Estimated {
        duration: Duration::ZERO,
        ..Estimated::default()
    };
    assert!(
        line(&nothing.said(rust_mutants::count::Count::new(1)), "ROUGHLY").contains("0:00:00"),
        "while no work is no time: {}",
        nothing.said(rust_mutants::count::Count::new(1))
    );
}

#[test]
fn the_tally_is_six_lines_a_reader_finds_by_its_shape() {
    let counted = Estimated {
        cataloged: 3_u64.into(),
        selected: 1_u64.into(),
        pairs: 1_u64.into(),
        tests: 1_u64.into(),
        tests_whole: 3_u64.into(),
        ..Estimated::default()
    };
    let said = counted.said(rust_mutants::count::Count::new(1));
    assert_eq!(
        said.lines().filter(|line| !line.is_empty()).count(),
        6,
        "the tally is six lines and no more, because it is read at the end of a wall of \
         mutants and a person finds it by its shape. It was five until the two the route \
         removed and the two nobody asked about shared a line, where they could not be \
         added up because they are not counted in the same thing: {said}"
    );
}
