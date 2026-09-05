// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run report looks like to a person. The document is the contract a program reads; these lines are the contract a reader reads, and both are fixed.

use std::path::Path;

use rust_mutants_cli::report::run::{
    Accounting, ExpectationDocument, FindingDocument, RunDocument, RunMeta, RunMutantDocument,
    ScoreDocument, lines,
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
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: outcome.to_owned(),
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration_ms: 41,
        tests_run: Some(1),
        retried: false,
        expected,
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
    mjutest_devkit::golden::golden(&golden, lines(&document()).as_bytes())
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
    let text = lines(&document);
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
    let text = lines(&document);
    assert!(text.contains("INTERRUPTED"), "{text}");
}
