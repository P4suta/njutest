// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report model and the invariants a durable report must satisfy.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_cli::report::audit::{Violation, validate_for_persistence};
use njutest_cli::report::{
    Finding, FindingKind, Git, Limitation, MutantAccounting, Position, Report, RunKind, SCHEMA,
    TargetAccounting, TargetRecord, TargetStatus, UNAVAILABLE, Verdict,
};

/// A report that satisfies every invariant, for a test to break one thing in. One field of a report, and how to leave it saying nothing.
type Blank = (&'static str, fn(&mut Report));

/// One fact about git, and how to make a report claim it.
type Claim = (&'static str, fn(&mut Git));

fn sound() -> Report {
    let mut report = Report::new(
        "20260905T081500Z-abcdef",
        RunKind::Full,
        njutest_cli::config::Contract::StandardV1,
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
    report.accounting.mutants = MutantAccounting {
        cataloged: 1,
        executed: 1,
        killed: 1,
        ..MutantAccounting::default()
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

#[test]
fn a_new_report_names_the_schema_the_run_and_what_it_ran_on() {
    let report = sound();
    assert_eq!(SCHEMA, "njutest-assurance-report-v1");
    assert_eq!(report.schema, SCHEMA);
    assert_eq!(report.run_id, "20260905T081500Z-abcdef");
    assert_eq!(report.run_kind, RunKind::Full);
    assert_eq!(report.tool.njutest, njutest_cli::VERSION);
    assert_eq!(report.tool.rust_mutants, rust_mutants::VERSION);
    assert!(report.limitations.is_empty());
    assert!(report.mutants.is_empty());
    assert_eq!(
        RunKind::default(),
        RunKind::Full,
        "a run that was not narrowed looked at everything, and the widest assurance is \
         the one a report of it may claim"
    );

    let fresh = Report::new(
        "r",
        RunKind::Full,
        njutest_cli::config::Contract::StandardV1,
    );
    assert_eq!(
        fresh.repository.workspace_digest, UNAVAILABLE,
        "a report says the tree it is about is unknown until somebody reads the tree: \
         every phase that learns it overwrites this, and none of them has to remember to \
         say so when it cannot"
    );
    assert_eq!(fresh.provenance.identity, UNAVAILABLE);
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
    report.verdict = Verdict::Insufficient;
    report.limitations.push(Limitation::new(
        "no-targets",
        "the workspace builds no test target",
    ));
    assert_eq!(validate_for_persistence(&report), Vec::<Violation>::new());
}

#[test]
fn a_report_that_put_no_mutation_to_a_test_cannot_say_it_is_assured() {
    let mut report = sound();
    report.accounting.mutants = MutantAccounting::default();
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::VerdictUnsupported { .. })),
        "an assurance is the claim that every mutation was noticed; with none put to a \
         test it is a claim about nothing, and the emptiness reads exactly like a suite \
         that noticed everything: {violations:?}"
    );

    report.verdict = Verdict::Insufficient;
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

#[test]
fn an_assurance_that_names_a_finding_is_two_claims_at_once() {
    let mut report = sound();
    report.findings = vec![Finding::new(
        FindingKind::SurvivingMutant,
        "cccccccc",
        "nothing noticed it",
    )];
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::FindingsDisagree { .. })),
        "{violations:?}"
    );
    assert!(
        violations[0].to_string().contains("nothing was found"),
        "{}",
        violations[0]
    );
}

#[test]
fn a_defect_a_reader_cannot_see_named_is_not_one_they_can_act_on() {
    let mut report = sound();
    report.verdict = Verdict::Defect;
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::FindingsDisagree { .. })),
        "{violations:?}"
    );
}

#[test]
fn a_defect_that_names_what_it_found_is_sound() {
    let mut report = sound();
    report.verdict = Verdict::Defect;
    report.findings = vec![Finding::new(
        FindingKind::FailingTest,
        "a1",
        "assertion failed",
    )];
    assert_eq!(validate_for_persistence(&report), Vec::new());
}

fn mutant(id: &str) -> njutest_cli::report::MutantRecord {
    njutest_cli::report::MutantRecord {
        id: id.to_owned(),
        display_id: id.get(..20).unwrap_or(id).to_owned(),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 1,
            column: 1,
            character_column: 1,
        },
        rule: "gt-to-ge@1".to_owned(),
        outcome: "killed".to_owned(),
        killed_by: Some("core/lib/core one".to_owned()),
        reused: false,
        source_run_id: None,
    }
}

#[test]
fn a_whole_report_cannot_call_a_uniquely_resolved_acceptance_unmatched() {
    let mut report = sound();
    report.verdict = Verdict::Insufficient;
    report.mutants = vec![mutant(&"a".repeat(64))];
    report.findings = vec![Finding::new(
        FindingKind::UnmatchedAcceptance,
        "aaaaaaaa",
        "the acceptance did not resolve",
    )];

    let violations = validate_for_persistence(&report);
    assert!(
        violations.iter().any(|violation| matches!(
            violation,
            Violation::UnmatchedAcceptanceResolved { subject, .. } if subject == "aaaaaaaa"
        )),
        "the full catalog proves the finding false: {violations:?}"
    );
}

#[test]
fn an_invalid_absent_or_ambiguous_acceptance_is_unmatched() {
    for subject in ["A", "cccc", "aaaa"] {
        let mut report = sound();
        report.verdict = Verdict::Insufficient;
        report.mutants = vec![
            mutant(&"a".repeat(64)),
            mutant(&format!("aaaa{}", "b".repeat(60))),
        ];
        report.findings = vec![Finding::new(
            FindingKind::UnmatchedAcceptance,
            subject,
            "the acceptance did not resolve",
        )];

        assert!(
            validate_for_persistence(&report)
                .iter()
                .all(|violation| !matches!(
                    violation,
                    Violation::UnmatchedAcceptanceResolved { .. }
                )),
            "{subject:?} does not resolve to exactly one catalog entry"
        );
    }
}

#[test]
fn a_shard_leaves_acceptance_resolution_for_the_merged_catalog_to_audit() {
    let mut report = sound();
    report.verdict = Verdict::Insufficient;
    report.scope.shard = Some("1/2".to_owned());
    report.mutants = vec![mutant(&"a".repeat(64))];
    report.findings = vec![Finding::new(
        FindingKind::UnmatchedAcceptance,
        "aaaaaaaa",
        "the acceptance did not resolve",
    )];

    assert!(
        validate_for_persistence(&report)
            .iter()
            .all(|violation| !matches!(violation, Violation::UnmatchedAcceptanceResolved { .. })),
        "one shard does not carry the catalog needed to re-resolve the prefix"
    );
}

#[test]
fn a_run_that_looked_at_part_of_a_workspace_does_not_assure_all_of_it() {
    let mut report = sound();
    report.run_kind = RunKind::Scoped;
    let violations = validate_for_persistence(&report);
    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::VerdictUnsupported { .. })),
        "{violations:?}"
    );

    report.verdict = Verdict::ScopeAssured;
    assert_eq!(validate_for_persistence(&report), Vec::new());
}

#[test]
fn one_survivor_nobody_accepted_is_one_too_many() {
    let mut accepted = sound();
    accepted.accounting.mutants.cataloged = 1;
    accepted.accounting.mutants.executed = 1;
    accepted.accounting.mutants.survived = 1;
    accepted.accounting.mutants.accepted = 1;

    let mut unaccepted = accepted.clone();
    unaccepted.accounting.mutants.accepted = 0;

    assert!(
        !validate_for_persistence(&accepted)
            .iter()
            .any(|violation| matches!(violation, Violation::VerdictUnsupported { .. })),
        "a survivor a reviewer accepted with a reason is one the run may still assure \
         around: {:?}",
        validate_for_persistence(&accepted)
    );
    assert!(
        validate_for_persistence(&unaccepted)
            .iter()
            .any(|violation| matches!(violation, Violation::VerdictUnsupported { .. })),
        "and the one after it is not: this is the boundary the whole contract turns on, \
         so it is the count itself that has to be compared and not the count plus room \
         for one: {:?}",
        validate_for_persistence(&unaccepted)
    );
}

#[test]
fn a_target_record_that_says_it_failed_is_not_answered_by_an_accounting_that_says_none_did() {
    let mut report = sound();
    report.targets[2].status = TargetStatus::Failed;

    let violations = validate_for_persistence(&report);

    assert!(
        violations
            .iter()
            .any(|violation| matches!(violation, Violation::VerdictUnsupported { .. })),
        "the accounting adds up and the records are all there, and one of them still \
         says it failed while the accounting says none did. A reader who trusts the \
         counts and a reader who reads the rows would come to two different answers \
         about the same run: {violations:?}"
    );
}

#[test]
fn a_field_a_report_leaves_empty_is_named_by_the_violation_that_refuses_it() {
    let blank: [Blank; 9] = [
        ("schema", |report: &mut Report| report.schema.clear()),
        ("run_id", |report: &mut Report| report.run_id.clear()),
        ("repository.root_name", |report: &mut Report| {
            report.repository.root_name.clear();
        }),
        ("repository.workspace_digest", |report: &mut Report| {
            report.repository.workspace_digest.clear();
        }),
        ("repository.configuration_digest", |report: &mut Report| {
            report.repository.configuration_digest.clear();
        }),
        ("repository.git.commit", |report: &mut Report| {
            report.repository.git.commit.clear();
        }),
        ("repository.git.branch", |report: &mut Report| {
            report.repository.git.branch.clear();
        }),
        ("toolchain.rustc", |report: &mut Report| {
            report.toolchain.rustc.clear();
        }),
        ("provenance.identity", |report: &mut Report| {
            report.provenance.identity.clear();
        }),
    ];

    for (field, empty) in blank {
        let mut report = sound();
        empty(&mut report);
        let violations = validate_for_persistence(&report);
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                Violation::EmptyRequiredValue { field: named } if named == field
            )),
            "a report that says nothing where {field} goes is one a reader cannot check, \
             and a violation that does not name the field leaves them to find it: \
             {violations:?}"
        );
    }
}

#[test]
fn a_run_that_could_not_ask_git_says_so_and_claims_none_of_its_facts() {
    let mut report = sound();
    report.repository.git = Git::unavailable();
    let stated = validate_for_persistence(&report);
    assert!(
        stated.iter().any(|violation| matches!(
            violation,
            Violation::MissingLimitation { name, .. } if name == "git-metadata-unavailable"
        )),
        "a run that could not name the commit it verified has to state that, or a reader \
         comparing two reports has no way to know which tree either was about: {stated:?}"
    );

    let claiming: [Claim; 5] = [
        ("repository.git.commit", |git: &mut Git| {
            git.commit = "0123456789abcdef0123456789abcdef01234567".to_owned();
        }),
        ("repository.git.branch", |git: &mut Git| {
            git.branch = "main".to_owned();
        }),
        ("repository.git.dirty", |git: &mut Git| {
            git.dirty = true;
        }),
        ("repository.git.merge_base", |git: &mut Git| {
            git.merge_base = Some("fedcba98".to_owned());
        }),
        ("repository.git.changed_files", |git: &mut Git| {
            git.changed_files = vec!["src/lib.rs".to_owned()];
        }),
    ];

    for (field, claim) in claiming {
        let mut report = sound();
        report.repository.git = Git::unavailable();
        report.limitations = vec![Limitation::new(
            "git-metadata-unavailable",
            "git said nothing",
        )];
        claim(&mut report.repository.git);
        let violations = validate_for_persistence(&report);
        assert!(
            violations.iter().any(|violation| matches!(
                violation,
                Violation::UnavailableWithFacts { field: named } if named == field
            )),
            "git was not available and {field} says otherwise; one of the two is untrue \
             and a report may not carry both: {violations:?}"
        );
    }
}

#[test]
fn targets_that_took_the_same_time_are_ordered_by_name_and_the_place_is_named() {
    let mut report = sound();
    report.targets = vec![
        target("a1", "core/lib/core one", TargetStatus::Passed, 30),
        target("c3", "core/test/it three", TargetStatus::Passed, 20),
        target("b2", "core/lib/core two", TargetStatus::Passed, 20),
    ];

    let violations = validate_for_persistence(&report);

    assert!(
        violations.iter().any(|violation| matches!(
            violation,
            Violation::TargetsOutOfOrder { at } if *at == 2
        )),
        "two targets that took the same time are ordered by identity, and the pair that \
         breaks the order is the second one here: a report that named the first would \
         send a reader to a pair that is fine: {violations:?}"
    );
    report.sort_targets();
    assert_eq!(
        validate_for_persistence(&report),
        Vec::<Violation>::new(),
        "and sorting is what the audit asks for, so the two agree by construction"
    );

    let mut backwards = sound();
    backwards.targets = vec![
        target("a1", "core/lib/core one", TargetStatus::Passed, 10),
        target("b2", "core/lib/core two", TargetStatus::Passed, 20),
        target("c3", "core/test/it three", TargetStatus::Passed, 30),
    ];
    let counted = validate_for_persistence(&backwards)
        .into_iter()
        .filter(|violation| matches!(violation, Violation::TargetsOutOfOrder { .. }))
        .count();
    assert_eq!(
        counted, 1,
        "every pair of a list in the wrong order is in the wrong order, and saying so \
         once for each would bury the one thing a reader has to do under a count of how \
         long the list is"
    );
}

#[test]
fn a_report_that_says_where_its_facts_came_from_says_one_thing_about_it() {
    let mut established = sound();
    established.provenance.cached = false;
    established.provenance.source_run_id = Some("20260905T081500Z-000000".to_owned());

    let mut anonymous = sound();
    anonymous.provenance.cached = true;
    anonymous.provenance.source_run_id = None;

    let mut itself = sound();
    itself.provenance.cached = true;
    itself.provenance.source_run_id = Some(itself.run_id.clone());

    let mut nameless = sound();
    nameless.provenance.cached = true;
    nameless.provenance.source_run_id = Some(String::new());

    for (what, report) in [
        (
            "a run that established its own facts and also names another",
            &established,
        ),
        (
            "a report read back from an earlier run that names no run",
            &anonymous,
        ),
        ("a run that read its own answer back", &itself),
        ("a source run with no name", &nameless),
    ] {
        let violations = validate_for_persistence(report);
        assert!(
            violations
                .iter()
                .any(|violation| matches!(violation, Violation::ProvenanceIncoherent { .. })),
            "{what} is a report that cannot be checked against the run it points at, \
             which is the whole of what provenance is for: {violations:?}"
        );
    }
}

/// Every report this audit refuses, one for each way it refuses one.
fn refused() -> Vec<Report> {
    let mut counted = sound();
    counted.accounting.targets.passed = 99;

    let mut blank = sound();
    blank.schema.clear();

    let mut failing = sound();
    failing.accounting.targets.failed = 1;
    failing.accounting.targets.passed = 1;
    failing.targets[0].status = TargetStatus::Failed;

    let mut disagreeing = sound();
    disagreeing.targets[2].status = TargetStatus::Failed;

    let mut found = sound();
    found.findings = vec![Finding::new(
        FindingKind::SurvivingMutant,
        "aaaaaaaaaaaa",
        "nothing noticed it",
    )];

    let mut silent = sound();
    silent.verdict = Verdict::Defect;

    let mut anonymous = sound();
    anonymous.provenance.cached = true;

    let mut itself = sound();
    itself.provenance.cached = true;
    itself.provenance.source_run_id = Some(itself.run_id.clone());

    let mut nameless = sound();
    nameless.provenance.cached = true;
    nameless.provenance.source_run_id = Some(String::new());

    let mut established = sound();
    established.provenance.source_run_id = Some("20260905T081500Z-000000".to_owned());

    let mut unasked = sound();
    unasked.repository.git = Git::unavailable();

    let mut claiming = sound();
    claiming.repository.git = Git::unavailable();
    claiming.repository.git.dirty = true;
    claiming.limitations = vec![Limitation::new(
        "git-metadata-unavailable",
        "git said nothing",
    )];

    let mut backwards = sound();
    backwards.targets.reverse();

    let mut empty = sound();
    empty.accounting.targets = TargetAccounting::default();
    empty.targets.clear();

    vec![
        counted,
        blank,
        failing,
        disagreeing,
        found,
        silent,
        anonymous,
        itself,
        nameless,
        established,
        unasked,
        claiming,
        backwards,
        empty,
    ]
}

#[test]
fn a_report_that_says_it_was_read_back_from_a_run_that_is_not_itself_is_coherent() {
    let mut cached = sound();
    cached.provenance.cached = true;
    cached.provenance.source_run_id = Some("20260905T081500Z-000000".to_owned());

    assert_eq!(
        validate_for_persistence(&cached),
        Vec::<Violation>::new(),
        "reusing what an earlier run of the same inputs established is the whole of what \
         evidence is for, and an audit that refused it would make every second run write \
         a report nobody may keep"
    );
}

#[test]
fn two_records_that_share_an_identity_are_not_blamed_for_being_out_of_order() {
    let mut twice = sound();
    twice.targets = vec![
        target("a1", "core/lib/core one", TargetStatus::Passed, 30),
        target("b2", "core/lib/core two", TargetStatus::Passed, 20),
        target("b2", "core/lib/core two", TargetStatus::Passed, 20),
    ];

    let violations = validate_for_persistence(&twice);

    assert!(
        !violations
            .iter()
            .any(|violation| matches!(violation, Violation::TargetsOutOfOrder { .. })),
        "the same identity twice is a report with a problem, and the problem is not the \
         order: a diagnostic that blamed the order would send a reader to sort a list \
         that is already sorted: {violations:?}"
    );
}

#[test]
fn every_refusal_is_a_finished_sentence() {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut kinds = 0u32;
    for report in refused() {
        let violations = validate_for_persistence(&report);
        assert!(!violations.is_empty(), "a report this audit refuses");
        for violation in violations {
            kinds = kinds.saturating_add(1);
            let said = violation.to_string();
            assert!(
                !said.contains("  "),
                "two spaces where a word was: a sentence with a hole in it is one \
                 somebody wrote and nobody read: {said:?}"
            );
            assert!(
                !said.trim_end().ends_with([';', ':', ',']),
                "a sentence that stops at its own semicolon promised a reason and gave \
                 none: {said:?}"
            );
            assert!(
                !said.ends_with(char::is_whitespace),
                "and one that stops at the space before the reason is the same hole with \
                 nothing to see: whatever the words around it were, they were written to \
                 be followed by something: {said:?}"
            );
            assert!(
                said.len() > 20 && !said.contains("Violation"),
                "what is wrong, in words a reader can act on, and not the name of a \
                 variant: {said:?}"
            );
            let _known = seen.insert(said);
        }
    }
    assert!(
        kinds >= 14,
        "one report for each way this refuses one, and every way says something \
         different: {kinds} refusals, {} of them distinct",
        seen.len()
    );
}
