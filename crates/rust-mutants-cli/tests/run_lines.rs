// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run report looks like to a person. The document is the contract a program reads; these lines are the contract a reader reads, and both are fixed.

#![expect(
    clippy::expect_used,
    reason = "a report-contract test reports impossible fixture and merge failures by panicking"
)]

use std::path::Path;

use njutest_devkit::result::{ResultState, result_state};
use rust_mutants::execute::StepLimitNotice;
use rust_mutants::id::{Identity, digest};
use rust_mutants::outcome::Outcome;
use rust_mutants::report::catalog::{
    PlatformDocument, RejectionDocument, SelectionDocument, WorkspaceDocument,
};
use rust_mutants::run::{FindingKind, NotRunReason};
use rust_mutants::span::Span;
use rust_mutants_cli::report::run::{
    Accounting, ExpectationDocument, FindingDocument, RunDocument, RunMeta, RunMutantDocument,
    ScoreDocument, StepEvidenceField,
};

const OTHER_MUTANT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const OTHER_CATALOG: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn returned<T: std::fmt::Debug, E: std::fmt::Debug>(result: Result<T, E>) -> Option<T> {
    assert_eq!(
        result_state(&result),
        ResultState::Returned,
        "the closed report fixture was refused: {result:?}"
    );
    match result {
        Ok(value) => Some(value),
        Err(_already_reported) => None,
    }
}

fn mutant(index: u32, outcome: Outcome, expected: bool) -> RunMutantDocument {
    let source_digest = format!("{index:064x}");
    let original = ">".to_owned();
    let replacement = ">=".to_owned();
    let id = Identity {
        path: "src/lib.rs".to_owned(),
        rule_name: "gt-to-ge".to_owned(),
        rule_version: 1,
        span: Span {
            start: 100,
            end: 101,
        },
        source_digest: source_digest.clone(),
        original_digest: digest(original.as_bytes()),
        replacement_digest: digest(replacement.as_bytes()),
    }
    .id()
    .expect("the fixture identity is complete");
    let display_id = id.display();
    assert_eq!(
        id.as_str().len(),
        64,
        "the fixture identity must be canonical"
    );
    assert_eq!(
        display_id.as_str().len(),
        20,
        "the fixture identity must have a display form"
    );
    RunMutantDocument {
        index,
        display_id: display_id.into_inner(),
        id: id.into_inner(),
        path: "src/lib.rs".to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: "gt-to-ge".to_owned(),
        item: "demo".to_owned(),
        rule_version: 1,
        line: 11,
        column: 8,
        start_byte: 100,
        end_byte: 101,
        source_digest,
        original,
        replacement,
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
        expected,
        unreached: false,
        source_run_id: None,
        step_notice: None,
    }
}

fn rejection(index: u32) -> RejectionDocument {
    let one = mutant(index, Outcome::Killed, false);
    RejectionDocument {
        index,
        id: one.id,
        display_id: one.display_id,
        path: one.path,
        rule: one.rule,
        code: Some("E0308".to_owned()),
        diagnostic: "the isolated candidate did not compile".to_owned(),
        isolated: true,
    }
}

fn document() -> RunDocument {
    let killed = mutant(0, Outcome::Killed, false);
    let expected = mutant(1, Outcome::Survived, true);
    let survivor = mutant(2, Outcome::Survived, false);
    let expected_id = expected.id.clone();
    let surviving_id = survivor.id.clone();
    let surviving_display_id = survivor.display_id.clone();
    let mutants = vec![killed, expected, survivor];
    RunDocument {
        document_type: "rust-mutants/run-report".to_owned(),
        schema_version: 2,
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
            mutant_steps: None,
        },
        targets: Vec::new(),
        established_tests: 0,
        accounting: fixture_accounting(),
        score: Some(ScoreDocument {
            detected: 1,
            decided: 3,
            value: 1.0 / 3.0,
        }),
        mutants,
        rejections: Vec::new(),
        skips: Vec::new(),
        expectations: vec![ExpectationDocument {
            id: "declared-survivor".to_owned(),
            locator: None,
            reason: "equivalent under the invariant the type carries".to_owned(),
            outcome: Outcome::Survived,
            mutant: Some(expected_id),
            covered: None,
            standing: "met".to_owned(),
            actual: None,
            why: None,
        }],
        findings: vec![FindingDocument {
            kind: FindingKind::SurvivingMutant,
            mutant: Some(surviving_id),
            detail: format!("no test noticed {surviving_display_id}; 1 tests ran and passed"),
        }],
    }
}

fn fixture_accounting() -> Accounting {
    Accounting {
        cataloged: 3,
        refused: 0_u32.into(),
        skipped: 0_u32.into(),
        executed: 3_u32.into(),
        killed: 1_u32.into(),
        survived: 2_u32.into(),
        step_limit_reached: 0_u32.into(),
        waited: 0_u32.into(),
        inconclusive: 0_u32.into(),
        errored: 0_u32.into(),
        unreached: 0_u32.into(),
        discharged: 0_u32.into(),
        not_run: 0_u32.into(),
        expected: 1_u32.into(),
    }
}

fn cohere(document: &mut RunDocument) {
    let mut accounting = Accounting {
        cataloged: u32::try_from(document.mutants.len())
            .expect("the test document fits the report schema"),
        refused: u32::try_from(document.rejections.len())
            .expect("the test document fits the report schema")
            .into(),
        skipped: document
            .skips
            .iter()
            .try_fold(0u32, |total, skip| total.checked_add(skip.count))
            .expect("the test document's skip count fits the report schema")
            .into(),
        ..Accounting::default()
    };
    for one in &document.mutants {
        match one.outcome {
            Outcome::Killed => accounting.killed.raise(),
            Outcome::Survived => accounting.survived.raise(),
            Outcome::StepLimitReached => accounting.step_limit_reached.raise(),
            Outcome::Waited => accounting.waited.raise(),
            Outcome::Inconclusive => accounting.inconclusive.raise(),
            Outcome::Errored => accounting.errored.raise(),
            Outcome::NotRun => accounting.not_run.raise(),
        }
        .expect("the finite fixture accounting fits u32");
        if one.not_run_reason == Some(NotRunReason::Unreached) {
            accounting
                .unreached
                .raise()
                .expect("the finite fixture accounting fits u32");
        }
        if one.not_run_reason == Some(NotRunReason::Discharged) {
            accounting
                .discharged
                .raise()
                .expect("the finite fixture accounting fits u32");
        }
        if one.expected {
            accounting
                .expected
                .raise()
                .expect("the finite fixture accounting fits u32");
        }
    }
    accounting.executed = accounting
        .cataloged
        .saturating_sub(accounting.not_run.count())
        .into();
    let detected = accounting.killed.count();
    let decided = detected.saturating_add(accounting.survived.count());
    document.score = (decided > 0).then(|| ScoreDocument {
        detected,
        decided,
        value: f64::from(detected) / f64::from(decided),
    });
    document.accounting = accounting;
    document.run.exit_code = if document.run.interrupted {
        rust_mutants::run::EXIT_INTERRUPTED
    } else if document
        .findings
        .iter()
        .any(|finding| finding.kind.is_infrastructure())
    {
        rust_mutants::run::EXIT_FAILED
    } else if document.findings.is_empty() {
        rust_mutants::run::EXIT_DETECTED
    } else {
        rust_mutants::run::EXIT_UNDETECTED
    };
}

fn step_notice(catalog: &str, mutant: &str, limit: u64) -> Option<StepLimitNotice> {
    match serde_json::from_value(serde_json::json!({
        "nonce": "0123456789abcdef0123456789abcdef",
        "catalog": catalog,
        "mutant": mutant,
        "limit": limit,
        "observed": limit.saturating_add(1)
    })) {
        Ok(notice) => Some(notice),
        Err(_) => None,
    }
}

fn step_document() -> RunDocument {
    let mut document = document();
    document.selection.mutant_steps = Some(10);
    if let Some(first) = document.mutants.first_mut() {
        first.outcome = Outcome::StepLimitReached;
        first.step_notice = step_notice(&document.workspace.catalog_digest, &first.id, 10);
        assert!(
            first.step_notice.is_some(),
            "the fixture step notice must obey the protocol"
        );
        document.findings.push(FindingDocument {
            kind: FindingKind::StepLimitReachedMutant,
            mutant: Some(first.id.clone()),
            detail: "the verified execution reached its configured step boundary".to_owned(),
        });
    }
    cohere(&mut document);
    document
}

#[test]
fn a_run_report_reads_as_the_recorded_lines() -> Result<(), njutest_devkit::golden::GoldenError> {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/run-lines.golden");
    let Some(lines) = returned(rust_mutants_cli::report::lines(&document())) else {
        return Ok(());
    };
    njutest_devkit::golden::golden(&golden, lines.as_bytes())?;
    Ok(())
}

#[test]
fn a_report_cannot_pair_one_verdict_with_another_verdicts_evidence_or_finding() {
    let whole = document();
    assert!(whole.validate().is_ok(), "the control document is coherent");

    let mut evidence = whole.clone();
    if let Some(first) = evidence.mutants.first_mut() {
        first.outcome = Outcome::StepLimitReached;
    }
    assert!(
        matches!(
            evidence.validate(),
            Err(rust_mutants_cli::report::run::DocumentError::StepEvidence { .. })
        ),
        "a step verdict without its exact runtime notice is not representable as a trusted report"
    );

    let mut finding = whole;
    if let Some(survivor) = finding
        .mutants
        .iter_mut()
        .find(|one| one.outcome == Outcome::Survived && !one.expected)
    {
        survivor.outcome = Outcome::Killed;
    }
    assert!(
        matches!(
            rust_mutants_cli::report::run::merge(&[finding]),
            Err(rust_mutants_cli::report::run::MergeError::InvalidPart { .. })
        ),
        "merge must not preserve a survivor finding beside a killed verdict"
    );
}

#[test]
fn a_step_notice_belongs_to_exactly_its_row_catalog_and_selected_limit() {
    let control = step_document();
    assert!(
        control.validate().is_ok(),
        "the control report is coherent: {:?}",
        control.validate()
    );
    let first_id = control
        .mutants
        .first()
        .map(|one| one.id.clone())
        .unwrap_or_default();
    assert_eq!(first_id.len(), 64, "the fixture must have a first mutant");

    for (field, mutant, catalog, selected) in [
        (
            StepEvidenceField::Mutant,
            OTHER_MUTANT,
            control.workspace.catalog_digest.as_str(),
            Some(10),
        ),
        (
            StepEvidenceField::Catalog,
            first_id.as_str(),
            OTHER_CATALOG,
            Some(10),
        ),
        (
            StepEvidenceField::Limit,
            first_id.as_str(),
            control.workspace.catalog_digest.as_str(),
            Some(11),
        ),
    ] {
        let mut tampered = control.clone();
        tampered.selection.mutant_steps = selected;
        if let Some(first) = tampered.mutants.first_mut() {
            first.step_notice = step_notice(catalog, mutant, 10);
        }
        assert!(
            matches!(
                tampered.validate(),
                Err(rust_mutants_cli::report::run::DocumentError::StepEvidenceMismatch {
                    field: actual,
                    ..
                }) if actual == field
            ),
            "a mismatch in {field:?} must fail closed"
        );
    }
}

#[test]
fn every_report_summary_is_rederived_before_it_is_trusted() {
    let control = document();
    assert!(control.validate().is_ok(), "the control report is coherent");

    let mut header = control.clone();
    header.schema_version = header.schema_version.saturating_add(1);
    assert!(matches!(
        header.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::Header { .. })
    ));

    let mut identity = control.clone();
    if let Some(first) = identity.mutants.first_mut() {
        first.replacement.push('!');
    }
    assert!(matches!(
        identity.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::CatalogRow { .. })
    ));

    let mut accounting = control.clone();
    accounting
        .accounting
        .killed
        .raise()
        .expect("the finite fixture accounting fits u32");
    assert!(matches!(
        accounting.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::Accounting { .. })
    ));

    let mut score = control.clone();
    score.score = None;
    assert!(matches!(
        score.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::Score)
    ));

    let mut exit = control;
    exit.run.exit_code = 0;
    assert!(matches!(
        exit.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::ExitCode { .. })
    ));
}

fn catalog_document() -> RunDocument {
    let mut control = document();
    control.rejections.push(rejection(3));
    cohere(&mut control);
    assert!(
        control.validate().is_ok(),
        "the accepted and refused rows form one coherent catalog: {:?}",
        control.validate()
    );
    control
}

#[test]
fn catalog_rows_have_one_dense_shard_checked_index_space() {
    let control = catalog_document();

    let mut shifted = control.clone();
    for row in &mut shifted.mutants {
        row.index = row.index.saturating_add(10);
    }
    for row in &mut shifted.rejections {
        row.index = row.index.saturating_add(10);
    }
    assert!(matches!(
        shifted.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::CatalogRow { .. })
    ));

    let mut fake_shard = control.clone();
    fake_shard.run.shard = Some("garbage".to_owned());
    assert!(matches!(
        fake_shard.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::Shard { .. })
    ));

    let mut wrong_shard = control.clone();
    wrong_shard.run.shard = Some("2/2".to_owned());
    assert!(matches!(
        wrong_shard.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::ShardRow { .. })
    ));

    let mut one_of_one = control;
    one_of_one.run.shard = Some("1/1".to_owned());
    for row in &mut one_of_one.mutants {
        row.index = row.index.saturating_add(10);
    }
    for row in &mut one_of_one.rejections {
        row.index = row.index.saturating_add(10);
    }
    assert!(matches!(
        one_of_one.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::CatalogRow { .. })
    ));
}

#[test]
fn catalog_rows_have_exact_display_ids_and_canonical_rules() {
    let control = catalog_document();

    let mut accepted_display = control.clone();
    if let Some(row) = accepted_display.mutants.first_mut() {
        assert!(
            row.display_id.pop().is_some(),
            "the fixture display id is not empty"
        );
    }
    assert!(matches!(
        accepted_display.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::CatalogRow { .. })
    ));

    let mut rejected_display = control.clone();
    if let Some(row) = rejected_display.rejections.first_mut() {
        assert!(
            row.display_id.pop().is_some(),
            "the fixture display id is not empty"
        );
    }
    assert!(matches!(
        rejected_display.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::CatalogRow { .. })
    ));

    let mut family = control;
    if let Some(row) = family.mutants.first_mut() {
        row.family = "arithmetic".to_owned();
    }
    assert!(matches!(
        family.validate(),
        Err(rust_mutants_cli::report::run::DocumentError::CatalogRow { .. })
    ));
}

#[test]
fn a_run_that_decided_nothing_says_so_rather_than_scoring_zero() {
    let mut document = document();
    document.score = None;
    document.accounting = Accounting {
        cataloged: 0,
        ..document.accounting
    };
    let text = rust_mutants_cli::report::lines(&document).expect("valid work ledger");
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
    let text = rust_mutants_cli::report::lines(&document).expect("valid work ledger");
    assert!(text.contains("INTERRUPTED"), "{text}");
}

#[test]
fn merging_the_parts_of_a_run_earns_the_code_the_whole_would_have_earned() {
    let unreached = |index| {
        let mut one = mutant(index, Outcome::NotRun, false);
        one.unreached = true;
        one.not_run_reason = Some(NotRunReason::Unreached);
        one
    };
    let part = |index: u32, shard: &str| {
        let mut document = document();
        document.run.shard = Some(shard.to_owned());
        let row = unreached(index);
        let id = row.id.clone();
        document.mutants = vec![row];
        document.findings = vec![FindingDocument {
            kind: FindingKind::UnreachedMutant,
            mutant: Some(id),
            detail: "no measured target reaches it".to_owned(),
        }];
        document.expectations.clear();
        cohere(&mut document);
        document
    };
    let merged = match rust_mutants_cli::report::run::merge(&[part(0, "1/2"), part(1, "2/2")]) {
        Ok(merged) => merged,
        Err(error) => panic!("coherent parts must merge: {error:?}"),
    };
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
    let text = rust_mutants_cli::report::lines(&document).expect("valid work ledger");
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
    let text = rust_mutants_cli::report::lines(&document).expect("valid work ledger");
    assert!(
        !text.contains("WORK"),
        "a report from before this release has no target list, and a share of nothing is not zero \
         per cent: {text}"
    );
}

#[test]
fn the_findings_of_a_whole_run_are_in_one_order_and_said_once() {
    let stale_mutant = mutant(0, Outcome::Killed, false);
    let stale_id = stale_mutant.id.clone();
    let finding = |kind: FindingKind, detail: &str| FindingDocument {
        kind,
        mutant: (kind == FindingKind::StaleExpectation).then(|| stale_id.clone()),
        detail: detail.to_owned(),
    };
    let shared = finding(
        FindingKind::UnmatchedExpectation,
        "the claim verifies nothing",
    );
    let merged = rust_mutants_cli::report::run::merge(&[
        findings_part(
            &stale_mutant,
            vec![
                finding(FindingKind::UnmatchedSkip, "b noticed nothing"),
                shared.clone(),
                finding(FindingKind::UnmatchedSkip, "a noticed nothing"),
            ],
        ),
        findings_part(
            &stale_mutant,
            vec![
                shared,
                finding(FindingKind::StaleExpectation, "a proof removed it"),
            ],
        ),
    ]);
    let merged = match merged {
        Ok(merged) => merged,
        Err(error) => panic!("coherent parts must merge: {error:?}"),
    };

    let read: Vec<(&str, &str)> = merged
        .findings
        .iter()
        .map(|one| (one.kind.as_str(), one.detail.as_str()))
        .collect();
    assert_eq!(
        read,
        [
            ("stale-expectation", "a proof removed it"),
            ("unmatched-expectation", "the claim verifies nothing"),
            ("unmatched-skip", "a noticed nothing"),
            ("unmatched-skip", "b noticed nothing"),
        ],
        "the whole says what the parts said in one order, whatever order the parts came back \
         in, and says a finding both parts made once"
    );
}

fn findings_part(stale_mutant: &RunMutantDocument, findings: Vec<FindingDocument>) -> RunDocument {
    let mut document = document();
    document.mutants = if findings
        .iter()
        .any(|finding| finding.kind == FindingKind::StaleExpectation)
    {
        vec![stale_mutant.clone()]
    } else {
        Vec::new()
    };
    document.expectations = findings.iter().filter_map(expectation_of).collect();
    document.findings = findings;
    cohere(&mut document);
    document
}

fn expectation_of(finding: &FindingDocument) -> Option<ExpectationDocument> {
    match finding.kind {
        FindingKind::StaleExpectation => Some(ExpectationDocument {
            id: finding.detail.clone(),
            locator: None,
            reason: finding.detail.clone(),
            outcome: Outcome::Survived,
            mutant: finding.mutant.clone(),
            covered: None,
            standing: "stale".to_owned(),
            actual: Some(Outcome::Killed),
            why: None,
        }),
        FindingKind::UnmatchedExpectation => Some(ExpectationDocument {
            id: finding.detail.clone(),
            locator: None,
            reason: finding.detail.clone(),
            outcome: Outcome::Survived,
            mutant: None,
            covered: None,
            standing: "unmatched".to_owned(),
            actual: None,
            why: Some("the claim names nothing".to_owned()),
        }),
        FindingKind::SurvivingMutant
        | FindingKind::InconclusiveMutant
        | FindingKind::StepLimitReachedMutant
        | FindingKind::WaitedMutant
        | FindingKind::ErroredMutant
        | FindingKind::NotRunMutant
        | FindingKind::UnreachedMutant
        | FindingKind::DischargedMutant
        | FindingKind::UnmatchedSkip => None,
    }
}

/// One claim a reviewer of a part wrote, which the whole has to keep.
fn claimed(why: &str, mutant: &RunMutantDocument) -> ExpectationDocument {
    ExpectationDocument {
        id: why.to_owned(),
        locator: None,
        reason: why.to_owned(),
        outcome: mutant.outcome,
        mutant: Some(mutant.id.clone()),
        covered: None,
        standing: "met".to_owned(),
        actual: None,
        why: None,
    }
}

fn part(rows: Vec<RunMutantDocument>, milliseconds: u64, shard: &str) -> RunDocument {
    let mut document = document();
    document.run.duration_ms = milliseconds;
    document.run.shard = Some(shard.to_owned());
    document.findings = rows
        .iter()
        .filter_map(|row| {
            let kind = match (row.outcome, row.not_run_reason) {
                (Outcome::Killed, _) => return None,
                (Outcome::Survived, _) if row.expected => return None,
                (Outcome::Survived, _) => FindingKind::SurvivingMutant,
                (Outcome::StepLimitReached, _) => FindingKind::StepLimitReachedMutant,
                (Outcome::Waited, _) => FindingKind::WaitedMutant,
                (Outcome::Inconclusive, _) => FindingKind::InconclusiveMutant,
                (Outcome::Errored, _) => FindingKind::ErroredMutant,
                (Outcome::NotRun, Some(NotRunReason::Unreached)) => FindingKind::UnreachedMutant,
                (Outcome::NotRun, Some(NotRunReason::Discharged)) => FindingKind::DischargedMutant,
                (Outcome::NotRun, _) => FindingKind::NotRunMutant,
            };
            Some(FindingDocument {
                kind,
                mutant: Some(row.id.clone()),
                detail: "the verdict's finding".to_owned(),
            })
        })
        .collect();
    document.mutants = rows;
    document.expectations.clear();
    cohere(&mut document);
    document
}

fn not_run(index: u32, reason: NotRunReason) -> RunMutantDocument {
    let mut one = mutant(index, Outcome::NotRun, false);
    one.unreached = reason == NotRunReason::Unreached;
    one.not_run_reason = Some(reason);
    one
}

#[test]
fn a_whole_run_is_what_its_parts_come_to_and_not_what_the_first_of_them_said() {
    let killed = mutant(0, Outcome::Killed, true);
    let survived = mutant(1, Outcome::Survived, true);
    let unreached = not_run(2, NotRunReason::Unreached);
    let mut earlier = part(vec![killed, unreached], 1_000, "1/2");
    earlier.expectations = earlier
        .mutants
        .first()
        .map(|one| claimed("the first part's reviewer", one))
        .into_iter()
        .collect();
    let discharged = not_run(3, NotRunReason::Discharged);
    let mut later = part(vec![survived, discharged], 250, "2/2");
    later.expectations = later
        .mutants
        .first()
        .map(|one| claimed("the second part's reviewer", one))
        .into_iter()
        .collect();
    let whole = rust_mutants_cli::report::run::merge(&[earlier, later]);
    let whole = match whole {
        Ok(whole) => whole,
        Err(error) => panic!("coherent parts must merge: {error:?}"),
    };

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
        (
            whole.accounting.unreached.count(),
            whole.accounting.discharged.count()
        ),
        (1, 1),
        "including the column each reason is counted in: {:?}",
        whole.accounting
    );
    assert_eq!(
        (
            whole.accounting.not_run.count(),
            whole.accounting.executed.count()
        ),
        (2, 2),
        "and what it did not run is not what it ran: {:?}",
        whole.accounting
    );
    assert_eq!(
        whole.expectations.len(),
        2,
        "and holds what every reviewer of every part claimed"
    );
    assert_eq!(whole.accounting.killed.count(), 1);
    assert_eq!(whole.accounting.survived.count(), 1);
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

/// One surviving mutation of `rule` in `path`, for the grouping the tally does.
fn survivor(path: &str, rule: &str, line: u32) -> RunMutantDocument {
    RunMutantDocument {
        path: path.to_owned(),
        rule: rule.to_owned(),
        item: "demo".to_owned(),
        line,
        ..mutant(line, Outcome::Survived, false)
    }
}

/// Survivors of a rule that names an unexecuted path are one gap said once, and everything else is its own.
#[test]
fn survivors_of_one_unexercised_path_are_counted_as_one_and_the_rest_are_not() {
    let mut document = document();
    document.mutants = vec![
        survivor("src/scan.rs", "question-to-unwrap", 531),
        survivor("src/scan.rs", "question-to-unwrap", 563),
        survivor("src/scan.rs", "question-to-unwrap", 574),
        survivor("src/scan.rs", "le-to-lt", 335),
        survivor("src/scan.rs", "le-to-lt", 436),
    ];
    let said = rust_mutants_cli::report::lines(&document).expect("valid work ledger");
    let line = said
        .lines()
        .find(|line| line.starts_with("SURVIVORS"))
        .unwrap_or("");
    assert!(
        line.contains("3 that are 1 unexercised paths"),
        "three question-to-unwrap survivors in one file are three instances of one \
         proposition, and a reader who works through them one at a time reads the same \
         sentence three times: {said}"
    );
    assert!(
        line.contains("2 each its own finding"),
        "two comparisons are two boundaries, and saying they are one would hide one of \
         them: {said}"
    );
}
