// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run report looks like to a person. The document is the contract a program reads; these lines are the contract a reader reads, and both are fixed.

use std::path::Path;

use rust_mutants_cli::report::run::{
    Accounting, ExpectationDocument, FindingDocument, RunDocument, RunMeta, RunMutantDocument,
    ScoreDocument,
};
use rust_mutants_cli::report::{PlatformDocument, SelectionDocument, WorkspaceDocument};

fn mutant(index: u32, outcome: &str, expected: bool) -> RunMutantDocument {
    RunMutantDocument {
        index,
        id: format!("{index:064x}"),
        display_id: format!("{index:020x}"),
        path: "src/lib.rs".to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: "gt-to-ge".to_owned(),
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
        retried: false,
        expected,
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
            id: "20260905T120000000Z".to_owned(),
            started_at: "2026-09-05T12:00:00Z".to_owned(),
            finished_at: "2026-09-05T12:00:01Z".to_owned(),
            duration_ms: 1000,
            interrupted: false,
            exit_code: 1,
            shard: None,
        },
        workspace: WorkspaceDocument {
            root_name: "demo".to_owned(),
            toolchain: "rustc 1.98.0".to_owned(),
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
            cataloged: 5,
            refused: 2,
            skipped: 7,
            executed: 5,
            killed: 3,
            survived: 2,
            timed_out: 0,
            inconclusive: 0,
            errored: 0,
            unreached: 0,
            not_run: 0,
            expected: 1,
        },
        score: Some(ScoreDocument {
            detected: 3,
            decided: 5,
            value: 0.6,
        }),
        mutants: vec![
            mutant(0, "killed", false),
            mutant(1, "survived", true),
            mutant(2, "survived", false),
        ],
        rejections: Vec::new(),
        skips: Vec::new(),
        expectations: vec![ExpectationDocument {
            id: "0000000000000001".to_owned(),
            reason: "equivalent under the invariant the type carries".to_owned(),
            outcome: "survived".to_owned(),
            mutant: Some(format!("{:064x}", 1)),
            standing: "met".to_owned(),
            actual: None,
            why: None,
        }],
        findings: vec![FindingDocument {
            kind: "surviving-mutant".to_owned(),
            mutant: Some(format!("{:064x}", 2)),
            detail: "no test noticed 00000000000000000002; 1 tests ran and passed".to_owned(),
        }],
    }
}

#[test]
fn a_run_report_reads_as_the_recorded_lines() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/run-lines.golden");
    mjutest_devkit::golden::golden(
        &golden,
        rust_mutants_cli::report::lines(&document()).as_bytes(),
    )
    .expect("the lines are the recorded ones");
}

#[test]
fn a_run_that_decided_nothing_says_so_rather_than_scoring_zero() {
    let mut document = document();
    document.score = None;
    document.accounting = Accounting {
        cataloged: 0,
        ..document.accounting
    };
    let text = rust_mutants_cli::report::lines(&document);
    assert!(
        text.contains("SCORE     none; the run decided nothing"),
        "{text}"
    );
    assert!(!text.contains("0.0%"), "{text}");
}

#[test]
fn an_interrupted_run_says_it_stopped_early() {
    let mut document = document();
    document.run.interrupted = true;
    document.run.exit_code = 130;
    let text = rust_mutants_cli::report::lines(&document);
    assert!(text.contains("INTERRUPTED"), "{text}");
}

#[test]
fn merging_the_parts_of_a_run_earns_the_code_the_whole_would_have_earned() {
    let unreached = || {
        let mut one = mutant(0, "not_run", false);
        one.unreached = true;
        one
    };
    let part = |index: u32| {
        let mut document = document();
        let mut row = unreached();
        row.index = index;
        row.id = format!("{index:064x}");
        row.display_id = format!("{index:020x}");
        document.mutants = vec![row];
        document.findings = vec![FindingDocument {
            kind: "unreached-mutant".to_owned(),
            mutant: Some(format!("{index:064x}")),
            detail: "no measured target reaches it".to_owned(),
        }];
        document.run.exit_code = 1;
        document
    };
    let merged = rust_mutants_cli::report::run::merge(&[part(0), part(1)]).expect("one whole");
    assert_eq!(
        merged.run.exit_code, 1,
        "a mutation nothing reaches is a gap in the tests, not a run that broke; the whole \
         earns what each part earned"
    );
}
