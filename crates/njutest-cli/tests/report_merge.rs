// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Combining the parts of one catalog into the report the whole would have written.

use njutest_cli::config::Contract;
use njutest_cli::report::Outcome;
use njutest_cli::report::merge::{MergeError, merge};
use njutest_cli::report::{
    CandidateRecord, Finding, FindingKind, Limitation, MutantRecord, Position, Report, RunKind,
    TargetRecord, TargetStatus, Timing, Verdict,
};

/// What a run established under `outcome`, against `by` where the outcome has a target.
///
/// The target is supplied where the outcome needs one, because the pairing is
/// the thing under test everywhere else and a fixture that could not build a
/// valid one would be testing the fixture.
fn decided(outcome: &str, by: Option<&str>) -> njutest_cli::report::Decided {
    let held = Outcome::parse(outcome).unwrap_or(Outcome::Errored);
    let named = by.unwrap_or("pkg/lib/pkg").to_owned();
    njutest_cli::report::Decided::of(held, Some(named))
        .or_else(|| njutest_cli::report::Decided::of(held, None))
        .unwrap_or(njutest_cli::report::Decided::Survived)
}

fn part(shard: &str, mutants: &[(&str, &str)]) -> Report {
    let mut report = Report::new(
        "20260908T000000Z-000001",
        RunKind::Full,
        Contract::StandardV1,
    );
    report.repository.workspace_digest = "a".repeat(64);
    report.repository.configuration_digest = "b".repeat(64);
    report.scope.shard = Some(shard.to_owned());
    report.targets.push(TargetRecord {
        id: "one".to_owned(),
        name: "pkg/lib/pkg".to_owned(),
        package: "pkg".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 5,
        message: None,
    });
    for (id, outcome) in mutants {
        report.mutants.push(MutantRecord {
            id: (*id).to_owned(),
            display_id: id.get(..8).unwrap_or(id).to_owned(),
            path: "src/lib.rs".to_owned(),
            rule: "gt-to-ge@1".to_owned(),
            item: "demo".to_owned(),
            original: ">".to_owned(),
            replacement: String::new(),
            position: Position {
                line: 1,
                column: 1,
                character_column: 1,
            },
            outcome: decided(outcome, Some("pkg/lib/pkg")),
            reuse: njutest_cli::report::Reuse(njutest_cli::report::Established::Here),
            blind_in: Vec::new(),
            routing: None,
        });
    }
    report
}

#[test]
fn the_whole_holds_every_mutant_its_parts_judged() {
    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);

    let whole = merge(&[one, two]).expect("two parts of one catalog");

    assert_eq!(whole.mutants.len(), 2, "{:?}", whole.mutants);
    assert_eq!(
        whole.accounting.mutants.cataloged, 2,
        "the accounting is derived again from what the whole holds, never averaged \
         from the parts: a part's numbers are over a different denominator"
    );
    assert_eq!(whole.accounting.mutants.killed, 2);
    assert_eq!(
        whole.scope.shard, None,
        "a whole is not a part, so it names none: {:?}",
        whole.scope
    );
    assert_eq!(
        whole.verdict,
        Verdict::Assured,
        "and the verdict is the one the whole supports, which is the thing a part \
         could not say"
    );
}

#[test]
fn nothing_is_not_a_catalog() {
    assert_eq!(merge(&[]).expect_err("no parts"), MergeError::Nothing);
}

fn shard_set_error(parts: &[Report]) -> String {
    match merge(parts) {
        Err(MergeError::ShardSet { because }) => because,
        Ok(_) | Err(_) => String::new(),
    }
}

#[test]
fn a_whole_requires_every_shard_not_only_disjoint_rows() {
    let one_of_two = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    assert!(
        shard_set_error(&[one_of_two]).contains("shard 2/2 is missing"),
        "a missing shard has no row with which to overlap, so disjoint rows do not prove a whole"
    );

    let one = part("1/3", &[("a".repeat(64).as_str(), "killed")]);
    let three = part("3/3", &[("c".repeat(64).as_str(), "killed")]);
    assert!(
        shard_set_error(&[one, three]).contains("shard 2/3 is missing"),
        "a gap in the middle is no more complete than a missing last part"
    );
}

#[test]
fn every_part_names_one_shared_denominator_and_one_distinct_index() {
    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let two_of_three = part("2/3", &[("b".repeat(64).as_str(), "killed")]);
    assert!(
        shard_set_error(&[one, two_of_three]).contains("while the first report is one of 2"),
        "disjoint rows cut with different denominators leave an unknown part"
    );

    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let another_one = part("1/2", &[("b".repeat(64).as_str(), "killed")]);
    assert!(
        shard_set_error(&[one, another_one]).contains("shard 1/2 was offered more than once"),
        "two disjoint documents with the same label still leave shard 2/2 absent"
    );
}

#[test]
fn one_whole_report_is_an_explicit_passthrough_but_cannot_be_mixed_with_parts() {
    let mut whole = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    whole.scope.shard = None;
    whole.verdict = Verdict::Assured;
    let whole = merge(std::slice::from_ref(&whole)).expect("one whole report");
    assert_eq!(whole.scope.shard, None);
    assert_eq!(whole.mutants.len(), 1);
    let partial = part("1/2", &[("b".repeat(64).as_str(), "killed")]);
    assert!(
        shard_set_error(&[whole, partial]).contains("does not name a shard"),
        "an unsharded report mixed with a part does not prove what the other part omitted"
    );

    let malformed = part("part one", &[("a".repeat(64).as_str(), "killed")]);
    assert!(
        shard_set_error(&[malformed]).contains("is not a shard"),
        "a label the engine would refuse cannot prove which part this report judged"
    );
}

#[test]
fn parts_of_two_different_trees_are_not_parts_of_one_catalog() {
    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.repository.workspace_digest = "c".repeat(64);

    let refused = merge(&[one, two]).expect_err("two trees");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the tree"),
        "adding up answers about two different trees produces an answer about neither: \
         {refused}"
    );
}

#[test]
fn parts_that_answered_to_different_contracts_are_not_added_into_one_that_answered_to_either() {
    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.contract = Contract::DeepV1;

    let refused = merge(&[one, two]).expect_err("two contracts");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the contract"),
        "a report is one claim that a contract was met, and a claim assembled from a \
         part that met it and a part that did not is true of neither: {refused}"
    );

    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.repository.configuration_digest = "d".repeat(64);
    let refused = merge(&[one, two]).expect_err("two configurations");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the configuration"),
        "{refused}"
    );
}

#[test]
fn a_mutant_two_parts_both_judged_says_the_parts_were_cut_differently() {
    let both = "a".repeat(64);
    let one = part("1/2", &[(both.as_str(), "killed")]);
    let two = part("2/2", &[(both.as_str(), "survived")]);

    let refused = merge(&[one, two]).expect_err("an overlap");
    assert!(
        matches!(&refused, MergeError::Overlapping { mutant } if mutant == &both),
        "every mutant belongs to exactly one part, so two parts holding one of them is \
         two runs cut with different N: {refused}"
    );
}

#[test]
fn parts_with_different_effective_scopes_or_tool_versions_are_not_one_catalog() {
    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.scope.requested_packages.push("only-this".to_owned());
    let refused = merge(&[one, two]).expect_err("two selected package sets");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the selected packages and exclusions"),
        "the same tree can be cataloged over different package subsets: {refused}"
    );

    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.run_kind = RunKind::Changed;
    let refused = merge(&[one, two]).expect_err("two run scopes");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the run scope"),
        "a full and changed catalog do not add up to either: {refused}"
    );

    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.tool.rust_mutants = "another engine".to_owned();
    let refused = merge(&[one, two]).expect_err("two engine versions");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the runner and engine versions"),
        "two engine versions need not enumerate the same catalog: {refused}"
    );
}

/// One mutant of a given disposition, so a part can hold more than the one that was killed.
fn disposed(id: &str, outcome: &str, reused: bool) -> MutantRecord {
    MutantRecord {
        id: id.to_owned(),
        display_id: id.get(..8).unwrap_or(id).to_owned(),
        path: "src/lib.rs".to_owned(),
        rule: "gt-to-ge@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: String::new(),
        position: Position {
            line: 1,
            column: 1,
            character_column: 1,
        },
        outcome: decided(outcome, None),
        reuse: njutest_cli::report::Reuse(if reused {
            njutest_cli::report::Established::ReadBackFrom("an earlier run".to_owned())
        } else {
            njutest_cli::report::Established::Here
        }),
        blind_in: Vec::new(),
        routing: None,
    }
}

#[test]
fn the_whole_counts_every_disposition_its_parts_held() {
    let mut one = part("1/2", &[]);
    one.mutants = vec![
        disposed(&"a".repeat(64), "killed", true),
        disposed(&"b".repeat(64), "survived", true),
        disposed(&"c".repeat(64), "compile-rejected", false),
    ];
    let mut two = part("2/2", &[]);
    two.mutants = vec![
        disposed(&"d".repeat(64), "timed_out", false),
        disposed(&"e".repeat(64), "unreached", false),
        disposed(&"f".repeat(64), "equivalent", false),
        disposed(&"g".repeat(64), "unconfirmed", false),
    ];

    let counts = merge(&[one, two]).expect("two parts").accounting.mutants;

    assert_eq!(counts.cataloged, 7);
    assert_eq!(counts.killed, 1);
    assert_eq!(counts.survived, 1);
    assert_eq!(counts.rejected, 1);
    assert_eq!(counts.runaway, 1);
    assert_eq!(counts.unreached, 1);
    assert_eq!(counts.equivalent, 1);
    assert_eq!(counts.reused_killed, 1);
    assert_eq!(counts.reused_survived, 1);
    assert_eq!(
        counts.executed, 4,
        "killed, survived, timed out, and the one this release has no column for were \
         each put to a test; unreached and equivalent were not, and a disposition a \
         later release adds is executed until something says otherwise"
    );
}

#[test]
fn what_both_parts_state_the_whole_states_once() {
    let stated = Limitation::new("touch-not-recorded", "the guards recorded nothing");
    let raised = Finding::new(
        FindingKind::SurvivingMutant,
        "aaaaaaaa",
        "nothing noticed it",
    );
    let mut one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    one.limitations = vec![stated.clone()];
    one.findings = vec![raised.clone()];
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.limitations = vec![stated.clone()];
    two.findings = vec![raised.clone()];

    let whole = merge(&[one, two]).expect("two parts");

    assert_eq!(
        whole.limitations,
        vec![stated],
        "both parts measured the same baseline, so both state the same limitation about \
         it; a whole that said it once per part would read as one limitation per shard: \
         {:?}",
        whole.limitations
    );
    assert_eq!(whole.findings, vec![raised]);
}

#[test]
fn an_unmatched_acceptance_every_shard_reports_is_one_finding_in_the_whole() {
    let raised = Finding::new(
        FindingKind::UnmatchedAcceptance,
        "ffff",
        "no mutant matches this acceptance",
    );
    let mut one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    one.findings = vec![raised.clone()];
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.findings = vec![raised.clone()];

    let whole = merge(&[one, two]).expect("two parts");

    assert_eq!(whole.findings, vec![raised]);
    assert_eq!(whole.verdict, Verdict::Insufficient);
}

#[test]
fn a_finding_only_one_part_raised_is_carried_by_the_whole() {
    let mut one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    one.findings = vec![Finding::new(
        FindingKind::SurvivingMutant,
        "aaaaaaaa",
        "nothing noticed it",
    )];
    let two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);

    let whole = merge(&[one, two]).expect("two parts");

    assert_eq!(whole.findings.len(), 1, "{:?}", whole.findings);
    assert_eq!(
        whole.verdict,
        Verdict::Insufficient,
        "a finding in one part is a finding of the whole, and the whole is what carries \
         the verdict it makes"
    );
}

#[test]
fn the_whole_ran_from_the_first_start_to_the_last_finish_and_cost_what_the_parts_cost() {
    let mut one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    one.timing = Timing {
        started: "2026-09-08T10:00:00Z".to_owned(),
        finished: "2026-09-08T10:05:00Z".to_owned(),
        duration_ms: 300_000,
    };
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);
    two.timing = Timing {
        started: "2026-09-08T09:00:00Z".to_owned(),
        finished: "2026-09-08T09:10:00Z".to_owned(),
        duration_ms: 600_000,
    };

    let whole = merge(&[one, two]).expect("two parts").timing;

    assert_eq!(whole.started, "2026-09-08T09:00:00Z", "the earliest start");
    assert_eq!(whole.finished, "2026-09-08T10:05:00Z", "the latest finish");
    assert_eq!(
        whole.duration_ms, 900_000,
        "and the sum of what they cost, not the span between them: parts run on \
         different machines at once, and the wall clock of the whole is not the work"
    );
}

#[test]
fn the_targets_of_the_whole_are_ordered_the_way_a_report_orders_them() {
    let mut one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    if let Some(first) = one.targets.first_mut() {
        first.duration_ms = 1;
    }
    one.targets.push(TargetRecord {
        id: "two".to_owned(),
        name: "pkg/test/slow".to_owned(),
        package: "pkg".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 900,
        message: None,
    });
    let two = part("2/2", &[("b".repeat(64).as_str(), "killed")]);

    let whole = merge(&[one, two]).expect("two parts");

    assert_eq!(
        whole
            .targets
            .iter()
            .map(|target| target.duration_ms)
            .collect::<Vec<u64>>(),
        vec![900, 1],
        "slowest first, which is the order every report is read in: {:?}",
        whole.targets
    );
}

fn offered(mutant: &str) -> CandidateRecord {
    CandidateRecord {
        finding: "surviving-mutant".to_owned(),
        mutant: mutant.to_owned(),
        kind: "patch".to_owned(),
        path: "src/lib.rs".to_owned(),
        digest: "c".repeat(64),
        preimage: None,
        stability_runs: 3,
        kill_runs: 2,
        accepted: true,
        why: None,
    }
}

#[test]
fn what_only_the_last_part_found_is_still_what_the_whole_found() {
    let one = part("1/2", &[("a".repeat(64).as_str(), "killed")]);
    let mut two = part("2/2", &[("b".repeat(64).as_str(), "survived")]);
    two.findings = vec![Finding::new(
        FindingKind::SurvivingMutant,
        "bbbbbbbb",
        "nothing noticed it",
    )];
    two.limitations = vec![Limitation::new(
        "custom-harness",
        "the target brings its own harness",
    )];
    two.candidates = vec![offered("bbbbbbbb")];

    let whole = merge(&[one, two]).expect("two parts");

    assert_eq!(
        whole.findings.len(),
        1,
        "a whole that carried only what its first part found would report a clean run \
         for every gap the other parts are the ones holding: {:?}",
        whole.findings
    );
    assert_eq!(whole.limitations.len(), 1, "{:?}", whole.limitations);
    assert_eq!(whole.candidates.len(), 1, "{:?}", whole.candidates);
    assert_eq!(
        whole.verdict,
        Verdict::Insufficient,
        "and the finding one part raised is what the whole concludes from"
    );
}

#[test]
fn the_acceptances_of_the_whole_are_the_ones_its_parts_recorded_and_no_others() {
    let mut one = part("1/3", &[("a".repeat(64).as_str(), "survived")]);
    one.accounting.mutants.accepted = 1;
    let mut two = part("2/3", &[("b".repeat(64).as_str(), "survived")]);
    two.accounting.mutants.accepted = 2;
    let three = part("3/3", &[("c".repeat(64).as_str(), "killed")]);

    let whole = merge(&[one, two, three]).expect("three parts");

    assert_eq!(
        whole.accounting.mutants.accepted, 3,
        "an acceptance is a fact about a reviewer, and every mutant belongs to exactly \
         one part, so the parts add up to what a reviewer recorded and to nothing more: \
         a whole that counted one acceptance nobody made would excuse a survivor nobody \
         looked at"
    );
}

/// `report` with `target` recorded as having answered `outcome` about its one mutation.
fn answered_by(mut report: Report, target: &str, outcome: &str) -> Report {
    for record in &mut report.mutants {
        record.routing = Some(njutest_cli::report::Routing {
            granularity: rust_mutants::session::Granularity::Block,
            reaching: vec![target.to_owned()],
            discharged: Vec::new(),
            fallback: None,
            answered: vec![njutest_cli::report::Answered {
                target: target.to_owned(),
                outcome: Outcome::parse(outcome).unwrap_or(Outcome::Errored),
            }],
        });
    }
    report
}

#[test]
fn a_target_silent_in_one_part_and_noticing_in_another_is_accused_by_neither_nor_by_the_whole() {
    let one = answered_by(
        part("1/2", &[("a".repeat(64).as_str(), "survived")]),
        "pkg/lib/pkg",
        "survived",
    );
    let two = answered_by(
        part("2/2", &[("b".repeat(64).as_str(), "killed")]),
        "pkg/lib/pkg",
        "killed",
    );

    for named in [&one, &two] {
        assert!(
            !named
                .findings
                .iter()
                .any(|one| one.kind == FindingKind::HollowTarget),
            "a part has seen a slice of the catalog, and whether a target notices \
             anything is a statement about the whole of it: {:?}",
            named.findings
        );
    }

    let whole = merge(&[one, two]).expect("two parts of one catalog");
    assert!(
        !whole
            .findings
            .iter()
            .any(|one| one.kind == FindingKind::HollowTarget),
        "and the whole sees it notice something, so nobody is accused. A conclusion \
         that came out differently because of where the catalog was cut is an \
         artefact of the measurement: {:?}",
        whole.findings
    );
}

#[test]
fn a_target_that_noticed_nothing_in_the_whole_catalog_is_accused_by_the_merge() {
    let one = answered_by(
        part("1/2", &[("a".repeat(64).as_str(), "survived")]),
        "pkg/lib/pkg",
        "survived",
    );
    let two = answered_by(
        part("2/2", &[("b".repeat(64).as_str(), "survived")]),
        "pkg/lib/pkg",
        "survived",
    );

    let whole = merge(&[one, two]).expect("two parts of one catalog");
    let accused: Vec<&Finding> = whole
        .findings
        .iter()
        .filter(|one| one.kind == FindingKind::HollowTarget)
        .collect();
    assert_eq!(
        accused.len(),
        1,
        "the whole is what may say it, and it says it once over both parts: {:?}",
        whole.findings
    );
    let said = accused
        .first()
        .map(|one| one.detail.clone())
        .unwrap_or_default();
    assert!(
        said.contains("2 mutations"),
        "over the whole catalog rather than over either part of it: {said}"
    );
}
