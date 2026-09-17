// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a person is told, drawn: the value in front of them rather than the record stream behind it.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reaches into the fixture it just built, and an expectation made of a \
              literal and the layout the configuration names is made where it is read"
)]

use njutest_cli::presentation::{
    Action, Diagnostic, Excerpt, Headline, Severity, Site, Terminal, Told, human,
};
use njutest_cli::report::Verdict;

/// Where a run of a project that has said nothing about where it writes was kept.
fn kept(run: &str) -> String {
    format!(
        "{}/runs/{run}",
        njutest_cli::config::Config::default()
            .reports
            .directory
            .display()
    )
}

/// One survivor, in a file this test wrote.
fn survivor() -> Diagnostic {
    Diagnostic {
        severity: Severity::Gap,
        code: "NJ-SURVIVOR",
        title: "the suite passed with this change in place".to_owned(),
        at: Some(Site {
            path: "src/lib.rs".to_owned(),
            line: 8,
            column: 10,
            excerpt: Excerpt::Read("    if a > b { a } else { b }".to_owned()),
            label: "changed to `>=`, and 1 test ran without noticing".to_owned(),
            width: 1,
        }),
        notes: vec!["both branches were taken, so nothing here checks the boundary".to_owned()],
        actions: vec![
            Action {
                said: "explain".to_owned(),
                command: "njutest explain src/lib.rs:sign:gt-to-ge@8".to_owned(),
            },
            Action {
                said: "accept".to_owned(),
                command: "njutest accept src/lib.rs:sign:gt-to-ge@8 --reason \"...\"".to_owned(),
            },
        ],
    }
}

fn told() -> Told {
    Told {
        headline: Headline {
            verdict: Verdict::Insufficient,
            project: "fixture-baseline".to_owned(),
            cataloged: 10,
            killed: 7,
            refused_by_types: 0,
            survived: 2,
            unreached: 1,
            duration_ms: 1911,
            kept: kept("20260101T000000Z-aaaaaa"),
        },
        places: Vec::new(),
        diagnostics: vec![survivor()],
        limitations: Vec::new(),
    }
}

#[test]
fn a_survivor_is_drawn_where_it_is_with_the_change_under_a_caret() {
    let drawn = human::draw(&told(), Terminal::plain(80));
    let lines: Vec<&str> = drawn.lines().collect();
    let code = lines
        .iter()
        .position(|line| line.contains("if a > b"))
        .expect("the line the run changed is shown");
    let caret = lines
        .get(code.saturating_add(1))
        .copied()
        .unwrap_or_default();
    assert_eq!(
        caret.find('^'),
        lines[code].find('>'),
        "the caret lands under the character the run changed, because going to open the \
         file is the part a compiler stopped making anybody do twenty years ago: {drawn}"
    );
    assert!(
        caret.contains("changed to `>=`"),
        "and says what it became: {drawn}"
    );
    assert!(
        drawn.contains("njutest explain src/lib.rs:sign:gt-to-ge@8"),
        "with the command that answers it, spelled as somebody would type it: {drawn}"
    );
}

#[test]
fn a_caret_lands_under_the_code_however_wide_the_characters_before_it_are() {
    let line = "    let \u{898b}\u{51fa}\u{3057} = count > 0;";
    let mut told = told();
    let site = told.diagnostics[0]
        .at
        .as_mut()
        .expect("the survivor is somewhere");
    site.excerpt = Excerpt::Read(line.to_owned());
    site.column = u32::try_from(line.chars().take_while(|one| *one != '>').count())
        .expect("a column")
        .saturating_add(1);

    let drawn = human::draw(&told, Terminal::plain(80));
    let lines: Vec<&str> = drawn.lines().collect();
    let code = lines
        .iter()
        .position(|one| one.contains("count"))
        .expect("the line is shown");
    let caret = lines
        .get(code.saturating_add(1))
        .copied()
        .unwrap_or_default();

    let under = width(&before(caret, '^'));
    let pointed = width(&before(lines[code], '>'));
    assert_eq!(
        under, pointed,
        "a caret is placed by how wide the line is on a terminal, which is neither how \
         many bytes it is nor how many characters: three characters here are two columns \
         each, and counting either of the first two lands the caret three columns short \
         of the code it is about:\n{drawn}"
    );
}

/// The line of a drawing that carries the counts, which is the last thing a reader is left with.
fn headline(drawn: &str) -> &str {
    drawn
        .lines()
        .find(|line| line.contains("killed,"))
        .unwrap_or(drawn)
}

/// How wide `text` is on a terminal, which is what a caret is placed by.
fn width(text: &str) -> usize {
    njutest_cli::presentation::wide(text)
}

/// Everything in `line` up to the first `mark`, taken by characters so a multi-byte one is never cut in half.
fn before(line: &str, mark: char) -> String {
    line.chars().take_while(|one| *one != mark).collect()
}

#[test]
fn a_file_that_moved_under_the_run_is_said_rather_than_drawn() {
    let mut told = told();
    if let Some(site) = told.diagnostics[0].at.as_mut() {
        site.excerpt = Excerpt::Moved;
    }
    let drawn = human::draw(&told, Terminal::plain(80));
    assert!(
        drawn.contains("the file has changed since the run, so the line is not shown"),
        "drawing a line the run never measured is worse than drawing none: {drawn}"
    );
    assert!(
        !drawn.contains("if a > b"),
        "and the line that is there now is not the line that was: {drawn}"
    );
}

#[test]
fn a_gap_that_is_only_in_one_build_says_which_build_it_is_in() {
    let mut record = njutest_cli::report::MutantRecord {
        id: "a".repeat(64),
        display_id: "a".repeat(20),
        path: "src/lib.rs".to_owned(),
        position: njutest_cli::report::Position {
            line: 8,
            column: 10,
            character_column: 10,
        },
        rule: "gt-to-ge".to_owned(),
        item: "sign".to_owned(),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: "survived".to_owned(),
        blind_in: vec!["release".to_owned()],
        killed_by: None,
        reused: false,
        source_run_id: None,
    };

    let said = njutest_cli::presentation::blindness_of(&record, false);
    assert!(
        said.contains("release"),
        "a gap only the release build has is closed by a different change than one \
         every build has, and a reader told neither goes looking in the wrong \
         program: {said}"
    );

    record.blind_in = Vec::new();
    let said = njutest_cli::presentation::blindness_of(&record, false);
    assert!(
        !said.contains("in "),
        "and a run of one build names none, because which build is not a question \
         it has: {said}"
    );
}

#[test]
fn what_the_type_system_refused_is_said_where_the_kills_are_said() {
    let mut refused = told();
    refused.headline.refused_by_types = 3;
    let drawn = human::draw(&refused, Terminal::plain(80));
    assert!(
        headline(&drawn).contains("7 killed, 3 refused by types, 2 survived"),
        "a mutation the compiler refuses is one the type system caught, and a reader \
         who is never told cannot see how much of what could go wrong is answered \
         before a test runs: {drawn}"
    );

    let none = human::draw(&told(), Terminal::plain(80));
    assert!(
        !headline(&none).contains("refused"),
        "and a run where the type system caught nothing says nothing about it, \
         because a zero in a headline is a column a reader learns to skip: {none}"
    );
}

#[test]
fn a_run_with_nothing_to_say_says_that_and_stops() {
    let told = Told {
        headline: Headline {
            verdict: Verdict::Assured,
            project: "fixture-assured".to_owned(),
            cataloged: 4,
            killed: 4,
            refused_by_types: 0,
            survived: 0,
            unreached: 0,
            duration_ms: 1388,
            kept: kept("20260101T000000Z-bbbbbb"),
        },
        places: Vec::new(),
        diagnostics: Vec::new(),
        limitations: Vec::new(),
    };
    assert_eq!(
        human::draw(&told, Terminal::plain(80)),
        format!(
            "  ASSURED   4 killed, 0 survived, 0 unreached   1.4s\n  {}\n",
            kept("20260101T000000Z-bbbbbb")
        ),
        "a run that found nothing is four lines of nothing in most tools and one line \
         here, because the answer is the whole of what it has to say"
    );
}
