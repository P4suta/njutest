// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report model and the invariants a durable report must satisfy.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use mjutest_cli::report::audit::{Violation, validate_for_persistence};
use mjutest_cli::report::{
    Git, Limitation, Position, Report, RunKind, SCHEMA, TargetAccounting, TargetRecord,
    TargetStatus, Verdict,
};

/// A report that satisfies every invariant, for a test to break one thing in.
fn sound() -> Report {
    let mut report = Report::new(
        "20260905T081500Z-abcdef",
        RunKind::Full,
        mjutest_cli::config::Contract::StandardV1,
    );
    report.verdict = Verdict::Assured;
    report.repository.git = Git {
        available: true,
        commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        branch: "main".to_owned(),
        dirty: false,
        merge_base: None,
        changed_files: Vec::new(),
    };
    report.accounting.targets = TargetAccounting {
        selected: 3,
        passed: 2,
        failed: 0,
        skipped: 1,
        missing: 0,
    };
    report.targets = vec![
        target("a1", "core/lib/core one", TargetStatus::Passed, 30),
        target("b2", "core/lib/core two", TargetStatus::Passed, 20),
        target("c3", "core/test/it three", TargetStatus::Skipped, 10),
    ];
    report
}

fn target(id: &str, name: &str, status: TargetStatus, duration_ms: u64) -> TargetRecord {
    TargetRecord {
        id: id.to_owned(),
        name: name.to_owned(),
        package: "core".to_owned(),
        status,
        duration_ms,
        message: None,
    }
}

// --- the shape ---------------------------------------------------------------------

#[test]
fn a_new_report_names_the_schema_the_run_and_what_it_ran_on() {
    let report = sound();
    assert_eq!(SCHEMA, "mjutest-assurance-report-v1");
    assert_eq!(report.schema, SCHEMA);
    assert_eq!(report.run_id, "20260905T081500Z-abcdef");
    assert_eq!(report.run_kind, RunKind::Full);
    assert_eq!(report.tool.mjutest, mjutest_cli::VERSION);
    assert_eq!(report.tool.rust_mutants, rust_mutants::VERSION);
    assert!(report.limitations.is_empty());
    assert!(report.mutants.is_empty());
}

#[test]
fn a_position_carries_both_columns_because_one_toolchain_uses_both() {
    let line = "    let γ = 1;";
    let at = line.find('γ').expect("the identifier");
    let position = Position::of(line, 12, at);
    assert_eq!(position.line, 12);
    assert_eq!(
        position.column, 9,
        "bytes, which is what a coverage region uses"
    );
    assert_eq!(
        position.character_column, 9,
        "characters, which is what a rustc diagnostic uses"
    );

    let wide = "    let 日本語 = 1; let γ = 2;";
    let at = wide.rfind('γ').expect("the second identifier");
    let position = Position::of(wide, 3, at);
    assert_ne!(
        position.column, position.character_column,
        "the two differ wherever it matters"
    );
    assert_eq!(position.column, 28, "27 bytes precede it");
    assert_eq!(position.character_column, 22, "21 characters precede it");
}

#[test]
fn git_is_either_available_with_its_facts_or_explicitly_not() {
    let unavailable = Git::unavailable();
    assert!(!unavailable.available);
    assert_eq!(unavailable.commit, "unavailable");
    assert_eq!(unavailable.branch, "unavailable");
    assert!(!unavailable.dirty);

    let available = Git {
        available: true,
        commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        branch: "main".to_owned(),
        dirty: true,
        merge_base: Some("fedcba98".to_owned()),
        changed_files: vec!["src/lib.rs".to_owned()],
    };
    assert!(available.available);
}

// --- the invariants ------------------------------------------------------------------

#[test]
fn a_sound_report_has_nothing_to_report() {
    assert_eq!(validate_for_persistence(&sound()), Vec::<Violation>::new());
}

#[test]
fn the_target_accounting_must_add_up_and_match_the_records() {
    let mut report = sound();
    report.accounting.targets.passed = 3;
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::TargetsDoNotAddUp { .. })),
        "{violations:?}"
    );

    let mut report = sound();
    report.targets.pop();
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::TargetRecordsDisagree { .. })),
        "{violations:?}"
    );
}

#[test]
fn a_verdict_must_be_the_one_the_accounting_supports() {
    let mut report = sound();
    report.accounting.targets.failed = 1;
    report.accounting.targets.passed = 1;
    report.targets[0].status = TargetStatus::Failed;
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::VerdictUnsupported { .. })),
        "a failing target and an ASSURED verdict cannot both be true: {violations:?}"
    );

    let mut report = sound();
    report.accounting.targets.missing = 1;
    report.accounting.targets.skipped = 0;
    report.targets[2].status = TargetStatus::Missing;
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::VerdictUnsupported { .. })),
        "a target that could not be found is not an assurance: {violations:?}"
    );
}

#[test]
fn targets_are_ordered_slowest_first_and_then_by_name() {
    let mut report = sound();
    report.targets.reverse();
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::TargetsOutOfOrder { .. })),
        "{violations:?}"
    );
    report.sort_targets();
    assert_eq!(validate_for_persistence(&report), Vec::<Violation>::new());
    let order: Vec<&str> = report.targets.iter().map(|one| one.id.as_str()).collect();
    assert_eq!(order, ["a1", "b2", "c3"]);
}

#[test]
fn a_report_that_says_nothing_ran_cannot_say_it_is_assured() {
    let mut report = sound();
    report.accounting.targets = TargetAccounting::default();
    report.targets.clear();
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::NothingObserved)),
        "{violations:?}"
    );
    // The same report is fine once it says so.
    report.verdict = Verdict::Insufficient;
    report.limitations.push(Limitation::new(
        "no-targets",
        "the workspace builds no test target",
    ));
    assert_eq!(validate_for_persistence(&report), Vec::<Violation>::new());
}

#[test]
fn an_unavailable_fact_is_a_sentinel_and_never_an_empty_string() {
    let mut report = sound();
    report.repository.git.commit = String::new();
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::EmptyRequiredValue { .. })),
        "{violations:?}"
    );

    let mut report = sound();
    report.repository.git.available = false;
    report.repository.git.commit = "0123456789abcdef".to_owned();
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::UnavailableWithFacts { .. })),
        "an unavailable git cannot also have a commit: {violations:?}"
    );

    let mut report = sound();
    report.repository.git = Git::unavailable();
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::MissingLimitation { .. })),
        "an unavailable git is a stated limitation: {violations:?}"
    );
    report.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "git is not available, so the run cannot name what it verified",
    ));
    assert_eq!(validate_for_persistence(&report), Vec::<Violation>::new());
}

#[test]
fn a_violation_says_what_is_wrong_in_words_a_reader_can_act_on() {
    let mut report = sound();
    report.accounting.targets.passed = 99;
    for violation in validate_for_persistence(&report) {
        let said = violation.to_string();
        assert!(said.len() > 20, "{said}");
        assert!(!said.contains("Violation"), "{said}");
    }
}
