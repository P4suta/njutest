// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a run at the terminal: what is drawn, and what the keys do.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rust_mutants_cli::report::run::{
    Accounting, RunDocument, RunMeta, RunMutantDocument, ScoreDocument,
};
use rust_mutants_cli::report::{PlatformDocument, SelectionDocument, WorkspaceDocument};
use rust_mutants_cli::tui::{Browser, Flow, draw, pressed};

fn mutant(index: u32, outcome: &str, rule: &str) -> RunMutantDocument {
    RunMutantDocument {
        index,
        id: format!("{index:064x}"),
        display_id: format!("{index:020x}"),
        path: "src/lib.rs".to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: rule.to_owned(),
        rule_version: 1,
        line: 11,
        column: 8,
        start_byte: 100,
        end_byte: 101,
        source_digest: format!("{index:064x}"),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: outcome.to_owned(),
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration_ms: 41,
        tests_run: Some(1),
        killed_by: Vec::new(),
        signal: None,
        not_run_reason: None,
        route: None,
        identical: None,
        retried: false,
        expected: false,
        unreached: false,
        source_run_id: None,
    }
}

fn document() -> RunDocument {
    RunDocument {
        document_type: "rust-mutants/run-report".to_owned(),
        schema_version: 1,
        tool_version: "0.1.0".to_owned(),
        run: RunMeta {
            id: "20260905T000000000Z".to_owned(),
            started_at: "2026-09-05T00:00:00Z".to_owned(),
            finished_at: "2026-09-05T00:00:01Z".to_owned(),
            duration_ms: 1000,
            interrupted: false,
            exit_code: 1,
            shard: None,
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
        },
        accounting: Accounting {
            cataloged: 3,
            refused: 0,
            skipped: 0,
            executed: 3,
            killed: 2,
            survived: 1,
            timed_out: 0,
            inconclusive: 0,
            errored: 0,
            not_run: 0,
            unreached: 0,
            discharged: 0,
            expected: 0,
        },
        score: Some(ScoreDocument {
            detected: 2,
            decided: 3,
            value: 2.0 / 3.0,
        }),
        mutants: vec![
            mutant(1, "killed", "gt-to-ge"),
            mutant(2, "survived", "eq-to-neq"),
            mutant(3, "killed", "add-to-sub"),
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

#[test]
fn a_run_is_drawn_as_the_recorded_frame() {
    let browser = Browser::of(document());
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/tui.golden");
    mjutest_devkit::golden::golden(&golden, format!("{}\n", drawn(&browser)).as_bytes())
        .expect("the frame is the recorded one");
}

#[test]
fn moving_down_and_up_selects_what_is_next_and_stops_at_the_ends() {
    let mut browser = Browser::of(document());
    assert_eq!(
        browser.current().map(|one| one.outcome.clone()),
        Some("killed".to_owned())
    );
    assert_eq!(pressed(&mut browser, 'j'), Flow::Continue);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("eq-to-neq".to_owned())
    );
    for _step in 0..10 {
        let _flow = pressed(&mut browser, 'j');
    }
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("add-to-sub".to_owned()),
        "the last mutant is the last place to be"
    );
    for _step in 0..10 {
        let _flow = pressed(&mut browser, 'k');
    }
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("gt-to-ge".to_owned())
    );
}

#[test]
fn narrowing_shows_one_outcome_and_starts_again_at_its_first() {
    let mut browser = Browser::of(document());
    assert_eq!(browser.narrowing(), "all");
    assert_eq!(browser.shown().len(), 3);
    let _flow = pressed(&mut browser, 'f');
    assert_eq!(browser.narrowing(), "killed");
    assert_eq!(browser.shown().len(), 2);
    assert_eq!(
        browser.current().map(|one| one.rule.clone()),
        Some("gt-to-ge".to_owned())
    );
    let _flow = pressed(&mut browser, 'f');
    assert_eq!(browser.narrowing(), "survived");
    assert_eq!(browser.shown().len(), 1);
    for _step in 0..6 {
        let _flow = pressed(&mut browser, 'f');
    }
    assert_eq!(browser.narrowing(), "killed", "the filter comes back round");
}

#[test]
fn a_filter_that_admits_nothing_draws_without_a_mutant() {
    let mut browser = Browser::of(document());
    for _step in 0..3 {
        let _flow = pressed(&mut browser, 'f');
    }
    assert_eq!(browser.narrowing(), "timed_out");
    assert!(browser.shown().is_empty());
    assert!(browser.current().is_none());
    assert!(drawn(&browser).contains("nothing here"));
}

#[test]
fn quitting_says_so() {
    let mut browser = Browser::of(document());
    assert_eq!(pressed(&mut browser, 'q'), Flow::Quit);
    assert_eq!(pressed(&mut browser, 'x'), Flow::Continue);
}
