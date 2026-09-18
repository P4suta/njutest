// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a person is told, drawn: the value in front of them rather than the record stream behind it.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reaches into the fixture it just built, and an expectation made of a \
              literal and the layout the configuration names is made where it is read"
)]

use njutest_cli::presentation::{
    Action, Diagnostic, Excerpt, Headline, Missing, Severity, Site, Terminal, Told, human,
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
            survived: 2,
            unreached: 1,
            timed_out: 0,
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
        site.excerpt = Excerpt::Instead(Missing::Moved);
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
fn a_run_with_nothing_to_say_says_that_and_stops() {
    let told = Told {
        headline: Headline {
            verdict: Verdict::Assured,
            project: "fixture-assured".to_owned(),
            cataloged: 4,
            killed: 4,
            survived: 0,
            unreached: 0,
            timed_out: 0,
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
            "  ASSURED   4 killed  0 survived  0 unreached  1.4s\n  {}\n",
            kept("20260101T000000Z-bbbbbb")
        ),
        "a run that found nothing is four lines of nothing in most tools and one line \
         here, because the answer is the whole of what it has to say"
    );
}

#[test]
fn what_to_draw_for_is_decided_from_what_was_found_out_and_nothing_else() {
    use njutest_cli::presentation::{Asked, Glyphs, ROOM, Reader, Wanted};

    let piped = Terminal::of(&Asked::default());
    assert!(
        !piped.colour && !piped.unicode && !piped.drawing && piped.width == ROOM,
        "a stream nobody said anything about gets the record stream, no colour, and a \
         width that is not eighty, because eighty is the width of a punched card: {piped:?}"
    );

    let asked = Asked {
        reader: Reader::Person,
        columns: Some(132),
        glyphs: Glyphs::Drawn,
        ..Asked::default()
    };
    assert_eq!(
        Terminal::of(&asked),
        Terminal {
            width: 132,
            colour: true,
            unicode: true,
            drawing: true
        },
        "a person at a terminal that said how wide it is, and whose locale says the font \
         has more than ASCII, gets all of it"
    );

    assert!(
        !Terminal::of(&Asked {
            colour: Wanted::Refused,
            ..asked.clone()
        })
        .colour,
        "NO_COLOR is honoured, because a person who set it meant it"
    );
    assert!(
        Terminal::of(&Asked {
            colour: Wanted::Forced,
            ..Asked::default()
        })
        .drawing,
        "and CLICOLOR_FORCE draws even into a pipe, which is how somebody captures it on \
         purpose"
    );

    let dumb = Terminal::of(&Asked {
        term: Some("dumb".to_owned()),
        ..asked
    });
    assert!(
        !dumb.colour && !dumb.unicode,
        "a terminal that says it is dumb is taken at its word: {dumb:?}"
    );

    assert_eq!(
        Terminal::of(&Asked {
            columns: Some(3),
            reader: Reader::Person,
            ..Asked::default()
        })
        .width,
        ROOM,
        "and a width nothing can be drawn in is one nobody meant, so it is not believed"
    );
}

#[test]
fn the_label_under_a_mark_says_what_the_run_established_and_never_something_else() {
    use njutest_cli::report::Decided;

    let said = |decided: Decided| njutest_cli::presentation::label("gt-to-ge", &decided);
    for (decided, wrong) in [
        (
            Decided::TimedOut {
                on: "fixture/test/smoke".to_owned(),
            },
            "noticed",
        ),
        (
            Decided::Unconfirmed {
                on: "fixture/test/smoke".to_owned(),
            },
            "noticed",
        ),
        (
            Decided::Errored {
                on: "fixture/test/smoke".to_owned(),
            },
            "noticed",
        ),
    ] {
        let label = said(decided.clone());
        assert!(
            !label.contains(wrong),
            "`decided_by` answers with a target for four outcomes and only one of them is \
             a detection. A label that reads the target out of it and says the target \
             noticed tells somebody a test caught this, about a measurement that ran out \
             of time or never happened: {label}"
        );
    }
    assert!(
        said(Decided::Equivalent).contains("proof"),
        "and a mutation a proof settled is not one nothing noticed: {}",
        said(Decided::Equivalent)
    );
    assert!(
        said(Decided::CompileRejected).contains("compiler"),
        "nor is one the compiler refused: {}",
        said(Decided::CompileRejected)
    );
}
