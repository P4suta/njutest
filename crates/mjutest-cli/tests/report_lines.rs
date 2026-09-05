// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The record stream a pipe reads and a person skims.

#![expect(
    clippy::assigning_clones,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;

use mjutest_cli::report::{
    Finding, FindingKind, Limitation, MutantRecord, Position, Report, RunKind, TargetAccounting,
    TargetRecord, TargetStatus, Verdict, lines,
};

fn report() -> Report {
    let mut report = Report::new(
        "20260905T081500Z-abcdef",
        RunKind::Full,
        mjutest_cli::config::Contract::StandardV1,
    );
    report.verdict = Verdict::Insufficient;
    report.repository.root_name = "fixture-workspace".to_owned();
    report.repository.packages = vec!["app".to_owned(), "core".to_owned()];
    report.timing.started = "2026-09-05T08:15:00Z".to_owned();
    report.timing.finished = "2026-09-05T08:16:12Z".to_owned();
    report.timing.duration_ms = 72_000;
    report.accounting.targets = TargetAccounting {
        selected: 2,
        passed: 1,
        failed: 1,
        skipped: 0,
        missing: 0,
    };
    report.accounting.mutants.cataloged = 9;
    report.accounting.mutants.survived = 1;
    report.targets = vec![
        TargetRecord {
            id: "0123456789abcdef".to_owned(),
            name: "core/lib/adds::works".to_owned(),
            package: "core".to_owned(),
            status: TargetStatus::Failed,
            duration_ms: 40,
            message: Some("assertion failed: left == right".to_owned()),
        },
        TargetRecord {
            id: "fedcba9876543210".to_owned(),
            name: "core/lib/adds::rounds".to_owned(),
            package: "core".to_owned(),
            status: TargetStatus::Passed,
            duration_ms: 12,
            message: None,
        },
    ];
    report.mutants = vec![MutantRecord {
        id: "c".repeat(64),
        display_id: "cccccccc".to_owned(),
        path: "crates/core/src/lib.rs".to_owned(),
        position: Position {
            line: 12,
            column: 9,
            character_column: 9,
        },
        rule: "lt-to-le@1".to_owned(),
        outcome: "survived".to_owned(),
        killed_by: None,
        reused: false,
        source_run_id: None,
    }];
    report.findings = vec![Finding::new(
        FindingKind::SurvivingMutant,
        "cccccccc",
        "no test noticed the edit at crates/core/src/lib.rs:12",
    )];
    report.limitations = vec![Limitation::new(
        "mutation-phase-not-implemented",
        "no mutant was executed, so nothing is claimed about the tests' strength",
    )];
    report
}

fn records(text: &str, kind: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.split('\t').next() == Some(kind))
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn every_line_is_a_record_that_names_its_kind_first() {
    let text = lines::stream(&report());
    assert!(text.ends_with('\n'), "the last record is terminated");
    for line in text.lines() {
        let kind = line.split('\t').next().unwrap_or_default();
        assert!(!kind.is_empty(), "an empty kind in {line:?}");
        assert_eq!(kind, kind.to_uppercase(), "kinds are shouted: {line:?}");
        assert!(!line.ends_with('\t'), "no empty trailing field: {line:?}");
    }
}

#[test]
fn the_verdict_is_the_last_record_so_a_reader_can_take_the_tail() {
    let text = lines::stream(&report());
    let last = text.lines().next_back().expect("at least one record");
    assert_eq!(last, "VERDICT\tINSUFFICIENT");
}

#[test]
fn there_is_one_record_for_each_thing_the_report_holds() {
    let report = report();
    let text = lines::stream(&report);
    assert_eq!(records(&text, "TARGET").len(), report.targets.len());
    assert_eq!(records(&text, "MUTANT").len(), report.mutants.len());
    assert_eq!(records(&text, "LIMITATION").len(), report.limitations.len());
    for kind in [
        "RUN",
        "TOOLCHAIN",
        "REPOSITORY",
        "SCOPE",
        "TIMING",
        "VERDICT",
    ] {
        assert_eq!(records(&text, kind).len(), 1, "one {kind} record");
    }
}

#[test]
fn the_targets_appear_in_the_order_the_report_put_them_in() {
    let text = lines::stream(&report());
    let names: Vec<String> = records(&text, "TARGET")
        .iter()
        .filter_map(|line| line.split('\t').nth(4).map(ToOwned::to_owned))
        .collect();
    assert_eq!(names, ["core/lib/adds::works", "core/lib/adds::rounds"]);
}

#[test]
fn a_mutant_record_carries_the_place_a_person_would_open() {
    let text = lines::stream(&report());
    let record = records(&text, "MUTANT").pop().expect("one mutant");
    assert!(
        record.contains("crates/core/src/lib.rs:12:9"),
        "path:line:column, the form an editor understands: {record}"
    );
    assert!(record.contains("survived"), "{record}");
    assert!(record.contains("lt-to-le@1"), "{record}");
}

#[test]
fn a_message_holding_a_newline_cannot_forge_a_record() {
    let mut report = report();
    report.targets[0].message = Some("boom\nLIMITATION\tforged\tnot a real one".to_owned());
    let text = lines::stream(&report);
    assert_eq!(
        records(&text, "LIMITATION").len(),
        1,
        "the one the report states, and no more: {text}"
    );
    assert!(text.contains("boom\\nLIMITATION"), "{text}");
}

#[test]
fn a_message_holding_a_tab_cannot_forge_a_field() {
    let mut report = report();
    report.targets[0].message = Some("left\tright".to_owned());
    let text = lines::stream(&report);
    let record = records(&text, "TARGET")
        .into_iter()
        .next()
        .expect("the target");
    assert_eq!(record.split('\t').count(), 6, "the fields it declares");
    assert!(record.contains("left\\tright"), "{record}");
}

#[test]
fn a_carriage_return_cannot_overwrite_what_was_already_printed() {
    let mut report = report();
    report.targets[0].message = Some("all good\rDEFECT".to_owned());
    let text = lines::stream(&report);
    assert!(!text.contains('\r'), "{text:?}");
    assert!(text.contains("all good\\rDEFECT"), "{text}");
}

#[test]
fn an_escape_sequence_cannot_colour_or_move_the_terminal() {
    let mut report = report();
    report.targets[0].message = Some("\u{1b}[31mred\u{1b}[0m\u{7f}".to_owned());
    let text = lines::stream(&report);
    assert!(!text.contains('\u{1b}'), "no escape survives: {text:?}");
    assert!(text.contains("\\u{1b}[31mred"), "{text}");
    assert!(text.contains("\\u{7f}"), "delete is a control too: {text}");
}

#[test]
fn a_backslash_is_written_so_the_escaping_reads_back() {
    let mut report = report();
    report.targets[0].message = Some(r"C:\n".to_owned());
    let text = lines::stream(&report);
    assert!(
        text.contains(r"C:\\n"),
        "otherwise a literal backslash-n and a newline are the same text: {text}"
    );
}

#[test]
fn the_stream_matches_the_recorded_one() {
    let text = lines::stream(&report());
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/report.golden.lines");
    mjutest_devkit::golden::golden(&golden, text.as_bytes()).expect("the recorded stream");
}

#[test]
fn a_finding_is_a_record_of_its_own_that_names_its_kind() {
    let text = lines::stream(&report());
    let record = records(&text, "FINDING")
        .into_iter()
        .next()
        .expect("the finding");
    let fields: Vec<&str> = record.split('\t').collect();
    assert_eq!(fields[0], "FINDING");
    assert_eq!(fields[1], "surviving-mutant", "the model's own wire name");
    assert_eq!(fields[2], "cccccccc");
    assert!(fields[3].contains("no test noticed"), "{record}");
}

#[test]
fn a_finding_detail_cannot_forge_a_verdict() {
    let mut report = report();
    report.findings[0].detail = "all clear\nVERDICT\tASSURED".to_owned();
    let text = lines::stream(&report);
    assert_eq!(
        records(&text, "VERDICT").len(),
        1,
        "the one the report reached, and no more: {text}"
    );
    assert!(text.ends_with("VERDICT\tINSUFFICIENT\n"), "{text}");
}
