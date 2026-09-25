// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a run at the terminal: what is drawn, and what the keys do.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rust_mutants::outcome::Outcome;
use rust_mutants::report::catalog::{PlatformDocument, SelectionDocument, WorkspaceDocument};
use rust_mutants::run::FindingKind;
use rust_mutants_cli::report::run::{
    Accounting, RunDocument, RunMeta, RunMutantDocument, ScoreDocument,
};
use rust_mutants_cli::report::sources::Held;
use rust_mutants_cli::tui::{Browser, Flow, Key, Pane, draw, pressed};

const SOURCE: &str = "// SPDX-FileCopyrightText: 2026 njutest contributors\n\
                      // SPDX-License-Identifier: MIT OR Apache-2.0\n\
                      \n\
                      //! A demo.\n\
                      \n\
                      /// Whether a is over b.\n\
                      ///\n\
                      /// The comparison the tests are about.\n\
                      #[must_use]\n\
                      pub const fn over(a: u32, b: u32) -> bool {\n\
                      \x20   a > b\n\
                      }\n\
                      \n\
                      #[cfg(test)]\n\
                      mod tests {\n\
                      \x20   use super::over;\n\
                      \n\
                      \x20   #[test]\n\
                      \x20   fn two_is_over_one() {\n\
                      \x20       assert!(over(2, 1));\n\
                      \x20   }\n\
                      }\n";

fn sources() -> std::collections::BTreeMap<String, Held> {
    std::iter::once(("src/lib.rs".to_owned(), Held::Measured(SOURCE.to_owned()))).collect()
}

fn browser() -> Browser {
    Browser::of(document(), sources())
}

fn mutant(index: u32, outcome: Outcome, rule: &str) -> RunMutantDocument {
    RunMutantDocument {
        index,
        id: format!("{index:064x}"),
        display_id: format!("{index:020x}"),
        path: "src/lib.rs".to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: rule.to_owned(),
        item: "demo".to_owned(),
        rule_version: 1,
        line: 11,
        column: 8,
        start_byte: 100,
        end_byte: 101,
        source_digest: format!("{index:064x}"),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome,
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration_ms: 41,
        tests_run: Some(1),
        killed_by: Vec::new(),
        signal: None,
        not_run_reason: None,
        route: None,
        identical: rust_mutants::run::CodegenIdentity::NotMeasured,
        retried: false,
        lingered: false,
        expected: false,
        unreached: false,
        source_run_id: None,
        step_notice: None,
    }
}

fn document() -> RunDocument {
    RunDocument {
        document_type: "rust-mutants/run-report".to_owned(),
        schema_version: 3,
        tool_version: "0.1.0".to_owned(),
        run: RunMeta {
            id: "20260905T000000000Z".to_owned(),
            started_at: "2026-09-05T00:00:00Z".to_owned(),
            finished_at: "2026-09-05T00:00:01Z".to_owned(),
            duration_ms: 1000,
            interrupted: false,
            exit_code: 1,
            shard: None,
            jobs: rust_mutants_cli::report::run::JobsDocument {
                asked: "auto".to_owned(),
                used: 1,
            },
        },
        workspace: WorkspaceDocument {
            root_name: "demo".to_owned(),
            toolchain: "cargo 1.98.0 / rustc 1.98.0".to_owned(),
            workspace_digest: "a".repeat(64),
            catalog_digest: "b".repeat(64),
            platform: PlatformDocument {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
            },
        },
        selection: SelectionDocument {
            build: Vec::new(),
            tier: "balanced".to_owned(),
            operators: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
            mutant_steps: None,
        },
        targets: Vec::new(),
        established_tests: 0,
        accounting: Accounting {
            cataloged: 3,
            refused: 0_u32.into(),
            skipped: 0_u32.into(),
            executed: 3_u32.into(),
            killed: 2_u32.into(),
            survived: 1_u32.into(),
            step_limit_reached: 0_u32.into(),
            waited: 0_u32.into(),
            inconclusive: 0_u32.into(),
            errored: 0_u32.into(),
            not_run: 0_u32.into(),
            unreached: 0_u32.into(),
            discharged: 0_u32.into(),
            expected: 0_u32.into(),
        },
        score: Some(ScoreDocument {
            detected: 2,
            decided: 3,
            value: 2.0 / 3.0,
        }),
        mutants: vec![
            mutant(1, Outcome::Killed, "gt-to-ge"),
            mutant(2, Outcome::Survived, "eq-to-neq"),
            mutant(3, Outcome::Killed, "add-to-sub"),
        ],
        rejections: Vec::new(),
        skips: Vec::new(),
        expectations: Vec::new(),
        findings: Vec::new(),
    }
}

fn drawn(browser: &Browser) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("a terminal");
    terminal
        .draw(|frame| draw(frame, browser))
        .expect("the frame is drawn");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| {
                    buffer
                        .cell((column, row))
                        .map_or(" ", ratatui::buffer::Cell::symbol)
                        .to_owned()
                })
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn recorded(name: &str, browser: &Browser) {
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata")
        .join(name);
    njutest_devkit::golden::golden(&golden, format!("{}\n", drawn(browser)).as_bytes())
        .expect("the frame is the recorded one");
}

#[test]
fn a_run_is_drawn_as_the_recorded_frame() {
    recorded("tui.golden", &browser());
}

#[test]
fn the_source_pane_shows_the_file_the_run_measured_around_the_mutation() {
    let mut browser = browser();
    assert_eq!(pressed(&mut browser, Key::Char('p')), Flow::Continue);
    assert_eq!(browser.pane(), Pane::Source);
    recorded("tui-source.golden", &browser);
    assert!(drawn(&browser).contains("a > b"), "{}", drawn(&browser));
}

#[test]
fn a_file_the_tree_no_longer_holds_says_so_rather_than_showing_something_else() {
    let mut browser = Browser::of(
        document(),
        std::iter::once(("src/lib.rs".to_owned(), Held::Changed)).collect(),
    );
    assert_eq!(pressed(&mut browser, Key::Char('p')), Flow::Continue);
    assert!(
        drawn(&browser).contains("changed since the run"),
        "{}",
        drawn(&browser)
    );
}

#[test]
fn searching_narrows_to_what_matches_and_the_query_is_shown() {
    let mut browser = browser();
    assert_eq!(pressed(&mut browser, Key::Char('/')), Flow::Continue);
    for character in "add".chars() {
        assert_eq!(pressed(&mut browser, Key::Char(character)), Flow::Continue);
    }
    assert_eq!(browser.query(), "add");
    assert_eq!(browser.shown().len(), 1);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("add-to-sub".to_owned())
    );
    recorded("tui-search.golden", &browser);
    assert_eq!(pressed(&mut browser, Key::Backspace), Flow::Continue);
    assert_eq!(browser.query(), "ad");
    assert_eq!(pressed(&mut browser, Key::Escape), Flow::Continue);
    assert_eq!(
        browser.query(),
        "",
        "escape leaves the search with nothing in it"
    );
    assert_eq!(browser.shown().len(), 3);
}

#[test]
fn a_search_that_is_being_typed_does_not_move_or_filter_or_quit() {
    let mut browser = browser();
    assert_eq!(pressed(&mut browser, Key::Char('/')), Flow::Continue);
    for character in "qjf".chars() {
        assert_eq!(pressed(&mut browser, Key::Char(character)), Flow::Continue);
    }
    assert_eq!(browser.query(), "qjf");
    assert_eq!(browser.narrowing(), "all");
    assert_eq!(browser.shown().len(), 0);
    assert_eq!(pressed(&mut browser, Key::Enter), Flow::Continue);
    assert_eq!(pressed(&mut browser, Key::Char('q')), Flow::Quit);
}

#[test]
fn the_findings_pane_names_what_stops_the_run_from_being_clean() {
    let mut document = document();
    document.findings = vec![rust_mutants_cli::report::run::FindingDocument {
        kind: FindingKind::SurvivingMutant,
        mutant: Some(format!("{:064x}", 2)),
        detail: "no test noticed 00000000000000000002; 1 test ran and passed".to_owned(),
    }];
    let mut browser = Browser::of(document, sources());
    assert_eq!(pressed(&mut browser, Key::Char('F')), Flow::Continue);
    assert_eq!(browser.pane(), Pane::Findings);
    recorded("tui-findings.golden", &browser);
}

#[test]
fn the_help_pane_names_every_key_the_browser_answers_to() {
    let mut browser = browser();
    assert_eq!(pressed(&mut browser, Key::Char('?')), Flow::Continue);
    assert_eq!(browser.pane(), Pane::Help);
    let drawn = drawn(&browser);
    for key in ["j", "k", "/", "f", "r", "p", "F", "?", "y", "q"] {
        assert!(drawn.contains(key), "{key} is not in the help: {drawn}");
    }
    recorded("tui-help.golden", &browser);
    assert_eq!(pressed(&mut browser, Key::Char('?')), Flow::Continue);
    assert_eq!(browser.pane(), Pane::Mutants, "asking again puts it away");
}

#[test]
fn a_number_goes_straight_to_one_filter_and_a_page_moves_by_a_page() {
    let mut browser = browser();
    assert_eq!(pressed(&mut browser, Key::Char('2')), Flow::Continue);
    assert_eq!(browser.narrowing(), "survived");
    assert_eq!(pressed(&mut browser, Key::Char('1')), Flow::Continue);
    assert_eq!(browser.narrowing(), "all");
    assert_eq!(pressed(&mut browser, Key::End), Flow::Continue);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("add-to-sub".to_owned())
    );
    assert_eq!(pressed(&mut browser, Key::Home), Flow::Continue);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("gt-to-ge".to_owned())
    );
    assert_eq!(pressed(&mut browser, Key::PageDown), Flow::Continue);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("add-to-sub".to_owned()),
        "a page past the end is the end"
    );
    assert_eq!(pressed(&mut browser, Key::PageUp), Flow::Continue);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("gt-to-ge".to_owned())
    );
}

#[test]
fn yanking_names_the_mutant_the_reader_was_on_and_quitting_hands_it_back() {
    let mut browser = browser();
    assert_eq!(browser.yanked(), None);
    assert_eq!(pressed(&mut browser, Key::Char('j')), Flow::Continue);
    assert_eq!(pressed(&mut browser, Key::Char('y')), Flow::Continue);
    assert_eq!(browser.yanked(), Some(format!("{:064x}", 2).as_str()));
    assert_eq!(pressed(&mut browser, Key::Char('q')), Flow::Quit);
}

#[test]
fn moving_down_and_up_selects_what_is_next_and_stops_at_the_ends() {
    let mut browser = browser();
    assert_eq!(
        browser.current().map(|one| one.outcome),
        Some(Outcome::Killed)
    );
    assert_eq!(pressed(&mut browser, Key::Char('j')), Flow::Continue);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("eq-to-neq".to_owned())
    );
    for _step in 0..10 {
        assert_eq!(pressed(&mut browser, Key::Char('j')), Flow::Continue);
    }
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("add-to-sub".to_owned()),
        "the last mutant is the last place to be"
    );
    for _step in 0..10 {
        assert_eq!(pressed(&mut browser, Key::Char('k')), Flow::Continue);
    }
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("gt-to-ge".to_owned())
    );
}

#[test]
fn narrowing_shows_one_outcome_and_starts_again_at_its_first() {
    let mut browser = browser();
    assert_eq!(browser.narrowing(), "all");
    assert_eq!(browser.shown().len(), 3);
    assert_eq!(pressed(&mut browser, Key::Char('f')), Flow::Continue);
    assert_eq!(browser.narrowing(), "killed");
    assert_eq!(browser.shown().len(), 2);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("gt-to-ge".to_owned())
    );
    assert_eq!(pressed(&mut browser, Key::Char('f')), Flow::Continue);
    assert_eq!(browser.narrowing(), "survived");
    assert_eq!(browser.shown().len(), 1);
    for _step in 0..7 {
        assert_eq!(pressed(&mut browser, Key::Char('f')), Flow::Continue);
    }
    assert_eq!(browser.narrowing(), "killed", "the filter comes back round");
}

#[test]
fn a_filter_that_admits_nothing_draws_without_a_mutant() {
    let mut browser = browser();
    for _step in 0..3 {
        assert_eq!(pressed(&mut browser, Key::Char('f')), Flow::Continue);
    }
    assert_eq!(browser.narrowing(), "step_limit_reached");
    assert!(browser.shown().is_empty());
    assert!(browser.current().is_none());
    assert!(drawn(&browser).contains("nothing here"));
}

#[test]
fn quitting_says_so() {
    let mut browser = browser();
    assert_eq!(pressed(&mut browser, Key::Char('q')), Flow::Quit);
    assert_eq!(pressed(&mut browser, Key::Char('x')), Flow::Continue);
}

#[test]
fn escape_puts_a_pane_away_before_it_leaves() {
    let mut browser = browser();
    assert_eq!(pressed(&mut browser, Key::Char('p')), Flow::Continue);
    assert_eq!(pressed(&mut browser, Key::Escape), Flow::Continue);
    assert_eq!(browser.pane(), Pane::Mutants);
    assert_eq!(pressed(&mut browser, Key::Escape), Flow::Quit);
}

#[test]
fn every_outcome_a_run_can_record_is_one_the_browser_narrows_to() {
    let mut browser = browser();
    assert_eq!(browser.narrowing(), "all");
    let mut visited = Vec::new();
    for _step in 0..Outcome::ALL.len() {
        assert_eq!(pressed(&mut browser, Key::Char('f')), Flow::Continue);
        visited.push(browser.narrowing());
    }
    let mut wanted: Vec<&str> = Outcome::ALL.iter().map(|one| one.as_str()).collect();
    let mut reached = visited.clone();
    wanted.sort_unstable();
    reached.sort_unstable();
    assert_eq!(
        reached, wanted,
        "the key that cycles the filter is the reader's only way to a column, so an \
         outcome it never reaches is one a stored run can hold and nobody can look at"
    );
    assert_eq!(pressed(&mut browser, Key::Char('f')), Flow::Continue);
    assert_eq!(browser.narrowing(), "all", "and then it comes back round");
    for outcome in Outcome::ALL {
        browser.narrowed(Some(outcome));
        assert_eq!(
            browser.narrowing(),
            outcome.as_str(),
            "and narrowing straight to one names it"
        );
    }
}
