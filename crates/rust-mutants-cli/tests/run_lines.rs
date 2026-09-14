// SPDX-FileCopyrightText: 2026 njutest contributors
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
        identical: None,
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
        targets: Vec::new(),
        established_tests: 0,
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
            discharged: 0,
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
            locator: None,
            reason: "equivalent under the invariant the type carries".to_owned(),
            outcome: "survived".to_owned(),
            mutant: Some(format!("{:064x}", 1)),
            covered: None,
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
    njutest_devkit::golden::golden(
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

#[test]
fn the_lines_say_how_much_of_a_whole_run_this_one_did_not_do() {
    let mut document = document();
    document.targets = vec![
        rust_mutants_cli::report::run::TargetDocument {
            id: "demo/lib/demo".to_owned(),
            kind: "lib".to_owned(),
            harness: true,
            tests: 4,
            limitations: Vec::new(),
        },
        rust_mutants_cli::report::run::TargetDocument {
            id: "demo/test/parity".to_owned(),
            kind: "test".to_owned(),
            harness: true,
            tests: 2,
            limitations: Vec::new(),
        },
    ];
    for mutant in &mut document.mutants {
        mutant.route = Some(rust_mutants_cli::report::run::RouteDocument {
            granularity: "block".to_owned(),
            fallback: None,
            reaching: vec!["demo/lib/demo".to_owned()],
            discharged: Vec::new(),
            executed: vec!["demo/lib/demo".to_owned()],
            tests: std::collections::BTreeMap::new(),
        });
    }
    let rows = document.mutants.len();
    let text = rust_mutants_cli::report::lines(&document);
    assert!(
        text.contains("WORK"),
        "a reader who cannot see the work cannot see it fall: {text}"
    );
    assert!(
        text.contains(&format!("started={rows} of {} pairs", rows * 2)),
        "every row against both targets is what a whole run would have started, and one target \
         each is what this one did: {text}"
    );
    assert!(
        text.contains(&format!("unreached={rows}")),
        "and coverage routing is what removed the other half: {text}"
    );
    assert!(text.contains("50.0% removed"), "{text}");
    assert!(
        !text.contains("asked for less than the whole"),
        "nothing here was a filter: {text}"
    );
}

#[test]
fn a_run_that_built_no_targets_says_nothing_about_work_rather_than_dividing_by_it() {
    let document = document();
    let text = rust_mutants_cli::report::lines(&document);
    assert!(
        !text.contains("WORK"),
        "a report from before this release has no target list, and a share of nothing is not zero \
         per cent: {text}"
    );
}

#[test]
fn the_findings_of_a_whole_run_are_in_one_order_and_said_once() {
    let finding = |kind: &str, detail: &str| FindingDocument {
        kind: kind.to_owned(),
        mutant: None,
        detail: detail.to_owned(),
    };
    let part = |findings: Vec<FindingDocument>| {
        let mut document = document();
        document.mutants = Vec::new();
        document.findings = findings;
        document
    };
    let shared = finding("unmatched-expectation", "the claim verifies nothing");
    let merged = rust_mutants_cli::report::run::merge(&[
        part(vec![
            finding("surviving-mutant", "b noticed nothing"),
            shared.clone(),
            finding("surviving-mutant", "a noticed nothing"),
        ]),
        part(vec![
            shared,
            finding("discharged-mutant", "a proof removed it"),
        ]),
    ])
    .expect("one whole");

    let read: Vec<(&str, &str)> = merged
        .findings
        .iter()
        .map(|one| (one.kind.as_str(), one.detail.as_str()))
        .collect();
    assert_eq!(
        read,
        [
            ("discharged-mutant", "a proof removed it"),
            ("surviving-mutant", "a noticed nothing"),
            ("surviving-mutant", "b noticed nothing"),
            ("unmatched-expectation", "the claim verifies nothing"),
        ],
        "the whole says what the parts said in one order, whatever order the parts came back \
         in, and says a finding both parts made once"
    );
}

/// One claim a reviewer of a part wrote, which the whole has to keep.
fn claimed(why: &str) -> ExpectationDocument {
    ExpectationDocument {
        id: why.to_owned(),
        locator: None,
        reason: why.to_owned(),
        outcome: "survived".to_owned(),
        mutant: None,
        covered: None,
        standing: "unmatched".to_owned(),
        actual: None,
        why: None,
    }
}

#[test]
fn a_whole_run_is_what_its_parts_come_to_and_not_what_the_first_of_them_said() {
    let part = |rows: Vec<RunMutantDocument>, milliseconds: u64, code: u8| {
        let mut document = document();
        document.run.duration_ms = milliseconds;
        document.run.exit_code = code;
        document.mutants = rows;
        document
    };
    let killed = mutant(0, "killed", false);
    let survived = mutant(1, "survived", false);
    let unreached = {
        let mut one = mutant(2, "not_run", false);
        one.unreached = true;
        one.not_run_reason = Some("unreached".to_owned());
        one
    };
    let mut earlier = part(vec![killed], 1_000, 0);
    earlier.expectations = vec![claimed("the first part's reviewer")];
    let discharged = {
        let mut one = mutant(3, "not_run", false);
        one.not_run_reason = Some("discharged".to_owned());
        one
    };
    let mut later = part(vec![survived, unreached, discharged], 250, 1);
    later.expectations = vec![claimed("the second part's reviewer")];
    let whole = rust_mutants_cli::report::run::merge(&[earlier, later]).expect("one whole");

    assert_eq!(
        whole.mutants.len(),
        4,
        "the whole holds every row its parts judged"
    );
    assert_eq!(
        whole.accounting.cataloged, 4,
        "and counts them: {:?}",
        whole.accounting
    );
    assert_eq!(
        (whole.accounting.unreached, whole.accounting.discharged),
        (1, 1),
        "including the column each reason is counted in: {:?}",
        whole.accounting
    );
    assert_eq!(
        (whole.accounting.not_run, whole.accounting.executed),
        (2, 2),
        "and what it did not run is not what it ran: {:?}",
        whole.accounting
    );
    assert_eq!(
        whole.expectations.len(),
        2,
        "and holds what every reviewer of every part claimed"
    );
    assert_eq!(whole.accounting.killed, 1);
    assert_eq!(whole.accounting.survived, 1);
    assert_eq!(
        whole.run.duration_ms, 1_250,
        "a whole took as long as its parts together"
    );
    assert_eq!(
        whole.run.exit_code, 1,
        "and earns what the whole earns, not what the part that ran first did"
    );
    assert!(
        whole
            .score
            .as_ref()
            .is_some_and(|score| score.decided == 2 && score.detected == 1),
        "and scores the whole: {:?}",
        whole.score
    );
}
