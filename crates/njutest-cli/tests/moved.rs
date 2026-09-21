// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What changed between two rounds of a watch, which is the only thing a watcher is reading for.

use njutest_cli::presentation::moved::moved;
use njutest_cli::presentation::{
    Blindness, Headline, Place, Spot, Standing, Terminal, Told, Unsettled,
};
use njutest_cli::report::Verdict;

fn spot(line: u32, was: &str, standing: Standing) -> Spot {
    Spot {
        line,
        column: 5,
        was: was.to_owned(),
        now: ">=".to_owned(),
        said: standing.word().to_owned(),
        standing,
        blind_in: Vec::new(),
        locator: format!("src/lib.rs:sign:gt-to-ge@{line}"),
    }
}

fn told(verdict: Verdict, spots: Vec<Spot>) -> Told {
    Told {
        headline: Headline {
            verdict,
            cataloged: 4,
            killed: 3,
            survived: 1,
            unreached: 0,
            step_limit_reached: 0,
            waited: 0,
            duration_ms: 1000,
            kept: String::new(),
        },
        places: vec![Place {
            item: "sign".to_owned(),
            path: "src/lib.rs".to_owned(),
            excerpt: Vec::new(),
            instead: None,
            spots,
        }],
        diagnostics: Vec::new(),
        limitations: Vec::new(),
    }
}

#[test]
fn a_gap_that_only_moved_down_the_file_is_the_same_gap() {
    let before = told(
        Verdict::Insufficient,
        vec![spot(8, ">", Standing::Blind(Blindness::Ran))],
    );
    let after = told(
        Verdict::Insufficient,
        vec![spot(12, ">", Standing::Blind(Blindness::Ran))],
    );
    let said = moved(&before, &after, Terminal::plain(80));
    assert!(
        !said.contains("closed") && !said.contains("new"),
        "this surface exists for the moment somebody has just edited the file, so a key \
         with a line number in it reports every insertion above a gap as one gap closing \
         and another opening. The person reads that as having fixed something and broken \
         something else in one keystroke: {said:?}"
    );
}

#[test]
fn a_gap_that_is_gone_is_said_and_a_new_one_is_said() {
    let before = told(
        Verdict::Insufficient,
        vec![
            spot(8, ">", Standing::Blind(Blindness::Ran)),
            spot(10, "<", Standing::Blind(Blindness::Ran)),
        ],
    );
    let after = told(
        Verdict::Insufficient,
        vec![
            spot(10, "<", Standing::Blind(Blindness::Ran)),
            spot(14, "*", Standing::Blind(Blindness::Never)),
        ],
    );
    let said = moved(&before, &after, Terminal::plain(80));
    assert!(
        said.contains("closed") && said.contains('>'),
        "the gap that went away is the news somebody is watching for: {said:?}"
    );
    assert!(
        said.contains('*'),
        "and so is one that was not there before: {said:?}"
    );
    assert!(
        !said.contains('<'),
        "a gap that is still exactly where it was is not news, and a watcher who is \
         shown it every round stops reading the ones that are: {said:?}"
    );
}

#[test]
fn a_round_that_changed_nothing_says_so_in_one_line() {
    let same = told(
        Verdict::Insufficient,
        vec![spot(8, ">", Standing::Blind(Blindness::Ran))],
    );
    let said = moved(&same, &same, Terminal::plain(80));
    assert_eq!(
        said.lines().count(),
        1,
        "an edit that moved nothing is one line, because the whole value of watching is \
         that the rounds which changed something look different from the rounds which \
         did not: {said:?}"
    );
}

#[test]
fn a_verdict_that_turned_is_the_headline_of_the_round() {
    let before = told(
        Verdict::Insufficient,
        vec![spot(8, ">", Standing::Blind(Blindness::Ran))],
    );
    let after = told(Verdict::Assured, Vec::new());
    let said = moved(&before, &after, Terminal::plain(80));
    assert!(
        said.contains("ASSURED"),
        "somebody watching is waiting for exactly this and should not have to read a \
         list to find it: {said:?}"
    );
}

#[test]
fn what_the_run_could_not_settle_is_not_counted_as_a_gap_that_closed() {
    let before = told(
        Verdict::Insufficient,
        vec![spot(8, ">", Standing::Unsettled(Unsettled::Waited))],
    );
    let after = told(Verdict::Insufficient, Vec::new());
    let said = moved(&before, &after, Terminal::plain(80));
    assert!(
        !said.contains("closed"),
        "a measurement that timed out last round and is absent this round was not closed \
         by anything the person did — the run established nothing either time, and \
         telling them they fixed it is the same lie one layer along: {said:?}"
    );
}
