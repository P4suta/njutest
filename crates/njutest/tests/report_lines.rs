// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The record stream a pipe reads and a person skims.

#![expect(
    clippy::assigning_clones,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;

use njutest::report::{
    BuildReport, Finding, FindingKind, Limitation, MutantAccounting, MutantRecord,
    ObserverAccounting, Position, Report, RunKind, TargetRecord, TargetStatus, lines,
};

fn report_varying(vary: &dyn Fn(&mut BuildReport)) -> Report {
    let mut source = BuildReport::new(
        "source-run",
        RunKind::Full,
        njutest::config::Contract::StandardV1,
    );
    source.repository.root_name = "fixture-workspace".to_owned();
    source.repository.packages = vec!["app".to_owned(), "core".to_owned()];
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    source.timing.started = "2026-09-05T08:15:00Z".to_owned();
    source.timing.finished = "2026-09-05T08:16:12Z".to_owned();
    source.timing.duration_ms = 72_000;
    source.targets = vec![
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
    source.count_targets().expect("one exact target accounting");
    source.mutants = vec![MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "c".repeat(64),
        display_id: "cccccccccccccccccccc".to_owned(),
        path: "crates/core/src/lib.rs".to_owned(),
        position: Position {
            line: 12,
            column: 9,
            character_column: 9,
        },
        rule: "lt-to-le@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: njutest::report::Decided::Survived,
        accepted: false,
        reuse: njutest::report::Reuse(njutest::report::Established::Here),
        blind_in: Vec::new(),
        routing: None,
    }];
    source.accounting.mutants = MutantAccounting {
        cataloged: 1,
        executed: 1,
        survived: 1,
        observers: ObserverAccounting {
            unnoticed: 1,
            ..ObserverAccounting::default()
        },
        ..MutantAccounting::default()
    };
    source.findings = vec![Finding::new(
        FindingKind::SurvivingMutant,
        "cccccccccccccccccccc",
        "no test noticed the edit at crates/core/src/lib.rs:12",
    )];
    source.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "the stream fixture is not a git repository",
    ));
    njutest::testkit::read_every_named_file(&mut source);
    source.verdict = source.concluded();
    vary(&mut source);
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let run =
        rust_mutants::id::RunId::try_from("20260905t081500z-abcdef").expect("a canonical run id");
    let latticed = njutest::report::across::configured(&run, &measurements)
        .expect("one checked complete lattice");
    let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog stream fixture cannot be a shard");
    };
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

fn report() -> Report {
    report_varying(&|_| {})
}

fn records(text: &str, kind: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.split('\t').next() == Some(kind))
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn every_line_is_a_record_that_names_its_kind_first() {
    let text = lines::stream(&report()).expect("the checked report streams");
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
    let text = lines::stream(&report()).expect("the checked report streams");
    let last = text.lines().next_back().expect("at least one record");
    assert_eq!(last, "VERDICT\tINSUFFICIENT");
}

#[test]
fn a_run_says_where_it_wrote_so_nobody_reading_it_has_to_know_the_layout() {
    let document = "elsewhere/runs/20260101T000000Z-aaaaaa/njutest-assurance-report-v1.json";
    let text = lines::kept(&report(), document).expect("the checked report keeps its document");
    let said = records(&text, "REPORT");
    assert_eq!(
        said,
        vec![
            "REPORT\telsewhere/runs/20260101T000000Z-aaaaaa/njutest-assurance-report-v1.json"
                .to_owned()
        ],
        "a script that read the verdict reads the rest beside it, and a project that moved \
         its report directory did not thereby break every reader: {text}"
    );
    assert_eq!(
        text.lines().next_back(),
        Some("VERDICT\tINSUFFICIENT"),
        "and the verdict is still the last record, because that is what a reader takes \
         the tail for: {text}"
    );
    assert!(
        records(
            &lines::stream(&report()).expect("the checked report streams"),
            "REPORT"
        )
        .is_empty(),
        "while a report printed with nowhere to point at says nothing rather than \
         guessing where one would be"
    );
}

#[test]
fn there_is_one_record_for_each_thing_the_report_holds() {
    let report = report();
    let text = lines::stream(&report).expect("the checked report streams");
    let conclusion = report
        .conclusion()
        .expect("the checked report has a representable conclusion");
    assert_eq!(records(&text, "TARGET").len(), conclusion.targets.len());
    assert_eq!(records(&text, "MUTANT").len(), conclusion.mutants.len());
    assert_eq!(
        records(&text, "LIMITATION").len(),
        conclusion.limitations.len()
    );
    for kind in [
        "RUN",
        "BUILD",
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
    let text = lines::stream(&report()).expect("the checked report streams");
    let names: Vec<String> = records(&text, "TARGET")
        .iter()
        .filter_map(|line| line.split('\t').nth(4).map(ToOwned::to_owned))
        .collect();
    assert_eq!(names, ["core/lib/adds::works", "core/lib/adds::rounds"]);
}

#[test]
fn a_mutant_record_carries_the_place_a_person_would_open() {
    let text = lines::stream(&report()).expect("the checked report streams");
    let record = records(&text, "MUTANT").pop().expect("one mutant");
    assert!(
        record.contains("crates/core/src/lib.rs:12:9"),
        "path:line:column, the form an editor understands: {record}"
    );
    assert!(record.contains("unnoticed"), "{record}");
    assert!(record.contains("lt-to-le@1"), "{record}");
}

#[test]
fn a_message_holding_a_newline_cannot_forge_a_record() {
    let report = report_varying(&|source| {
        source.targets[0].message = Some("boom\nLIMITATION\tforged\tnot a real one".to_owned());
    });
    let text = lines::stream(&report).expect("the checked report streams");
    assert_eq!(
        records(&text, "LIMITATION").len(),
        1,
        "the one the report states, and no more: {text}"
    );
    assert!(text.contains("boom\\nLIMITATION"), "{text}");
}

#[test]
fn a_message_holding_a_tab_cannot_forge_a_field() {
    let report = report_varying(&|source| {
        source.targets[0].message = Some("left\tright".to_owned());
    });
    let text = lines::stream(&report).expect("the checked report streams");
    let record = records(&text, "TARGET")
        .into_iter()
        .next()
        .expect("the target");
    assert_eq!(record.split('\t').count(), 6, "the fields it declares");
    assert!(record.contains("left\\tright"), "{record}");
}

#[test]
fn a_carriage_return_cannot_overwrite_what_was_already_printed() {
    let report = report_varying(&|source| {
        source.targets[0].message = Some("all good\rDEFECT".to_owned());
    });
    let text = lines::stream(&report).expect("the checked report streams");
    assert!(!text.contains('\r'), "{text:?}");
    assert!(text.contains("all good\\rDEFECT"), "{text}");
}

#[test]
fn an_escape_sequence_cannot_colour_or_move_the_terminal() {
    let report = report_varying(&|source| {
        source.targets[0].message = Some("\u{1b}[31mred\u{1b}[0m\u{7f}".to_owned());
    });
    let text = lines::stream(&report).expect("the checked report streams");
    assert!(!text.contains('\u{1b}'), "no escape survives: {text:?}");
    assert!(text.contains("\\u{1b}[31mred"), "{text}");
    assert!(text.contains("\\u{7f}"), "delete is a control too: {text}");
}

#[test]
fn a_backslash_is_written_so_the_escaping_reads_back() {
    let report = report_varying(&|source| {
        source.targets[0].message = Some(r"C:\n".to_owned());
    });
    let text = lines::stream(&report).expect("the checked report streams");
    assert!(
        text.contains(r"C:\\n"),
        "otherwise a literal backslash-n and a newline are the same text: {text}"
    );
}

#[test]
fn the_stream_matches_the_recorded_one() {
    let text = lines::stream(&report()).expect("the checked report streams");
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/report.golden.lines");
    njutest_devkit::golden::golden(&golden, text.as_bytes()).expect("the recorded stream");
}

#[test]
fn a_finding_is_a_record_of_its_own_that_names_its_kind() {
    let text = lines::stream(&report()).expect("the checked report streams");
    let record = records(&text, "FINDING")
        .into_iter()
        .next()
        .expect("the finding");
    let fields: Vec<&str> = record.split('\t').collect();
    assert_eq!(fields[0], "FINDING");
    assert_eq!(fields[1], "surviving-mutant", "the model's own wire name");
    assert_eq!(fields[2], "cccccccccccccccccccc");
    assert!(fields[3].contains("no test noticed"), "{record}");
}

#[test]
fn an_unmatched_acceptance_is_a_named_record_in_the_line_projection() {
    let report = report_varying(&|source| {
        source.findings.push(Finding::new(
            FindingKind::UnmatchedAcceptance,
            "ffff",
            "no mutant matches this acceptance",
        ));
    });

    let finding = records(
        &lines::stream(&report).expect("the checked report streams"),
        "FINDING",
    )
    .into_iter()
    .next()
    .expect("one finding");
    assert!(
        finding.starts_with("FINDING\tunmatched-acceptance\tffff\t"),
        "{finding}"
    );
}

#[test]
fn a_finding_detail_cannot_forge_a_verdict() {
    let report = report_varying(&|source| {
        source.findings[0].detail = "all clear\nVERDICT\tASSURED".to_owned();
    });
    let text = lines::stream(&report).expect("the checked report streams");
    assert_eq!(
        records(&text, "VERDICT").len(),
        1,
        "the one the report reached, and no more: {text}"
    );
    assert!(text.ends_with("VERDICT\tINSUFFICIENT\n"), "{text}");
}
