// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Combining the parts of one catalog into the report the whole would have written.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::assigning_clones,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::uninlined_format_args,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_cli::config::Contract;
use njutest_cli::report::StepBoundary;
use njutest_cli::report::merge::{MergeError, merge};
use njutest_cli::report::{
    BuildReport, CandidateRecord, Finding, FindingKind, LatticedDocument, Limitation,
    MutantAccounting, MutantRecord, Outcome, Position, Report, RunKind, ShardReport, TargetRecord,
    TargetStatus, Timing, Verdict,
};
use rust_mutants::id::RunId;

/// What a run established under `outcome`, against `by` where the outcome has a target.
///
/// The target is supplied where the outcome needs one, because the pairing is
/// the thing under test everywhere else and a fixture that could not build a
/// valid one would be testing the fixture.
fn decided(outcome: &str, by: Option<&str>) -> njutest_cli::report::Decided {
    let Some(held) = Outcome::parse(outcome) else {
        panic!("no outcome of a report is named {outcome}");
    };
    let named = by.unwrap_or("pkg/lib/pkg").to_owned();
    let boundary = (held == Outcome::StepLimitReached)
        .then(|| StepBoundary::new(10, 11).expect("a first count beyond the allowance"));
    njutest_cli::report::Decided::of(held, Some(named), boundary)
        .or_else(|| njutest_cli::report::Decided::of(held, None, boundary))
        .unwrap_or(njutest_cli::report::Decided::Survived)
}

/// Exactly what the report derives from rows, spelled here because the model's own counter is private to it.
fn counted(rows: &[MutantRecord]) -> MutantAccounting {
    let mut counts = MutantAccounting {
        cataloged: u32::try_from(rows.len()).expect("a fixture's rows are countable"),
        ..MutantAccounting::default()
    };
    for row in rows {
        let outcome = row.outcome.outcome();
        counts
            .observers
            .counted(outcome.decision())
            .expect("a fixture's rows are countable");
        if row.accepted {
            counts.accepted += 1;
        }
        match outcome {
            Outcome::CompileRejected => counts.rejected += 1,
            Outcome::Killed => {
                counts.executed += 1;
                counts.killed += 1;
                if row.reuse.0.read_back().is_some() {
                    counts.reused_killed += 1;
                }
            }
            Outcome::Survived => {
                counts.executed += 1;
                counts.survived += 1;
                if row.reuse.0.read_back().is_some() {
                    counts.reused_survived += 1;
                }
            }
            Outcome::StepLimitReached => {
                counts.executed += 1;
                counts.step_limit_reached += 1;
            }
            Outcome::Waited => {
                counts.executed += 1;
                counts.waited += 1;
            }
            Outcome::Unreached => counts.unreached += 1,
            Outcome::Equivalent => counts.equivalent += 1,
            Outcome::ModelNoticed => {
                counts.executed += 1;
                counts.model_noticed += 1;
            }
            Outcome::ModelProved => {
                counts.executed += 1;
                counts.model_proved += 1;
            }
            Outcome::Unconfirmed | Outcome::Errored => counts.executed += 1,
        }
    }
    counts
}

/// One judged row at its canonical catalog position.
fn row(index: u32, id: &str, outcome: &str, reused: bool) -> MutantRecord {
    let mut record = disposed(id, outcome, reused);
    record.catalog_index = njutest_cli::report::CatalogIndex::new(index);
    record
}

/// One mutant of a given disposition, so a part can hold more than the one that was killed.
fn disposed(id: &str, outcome: &str, reused: bool) -> MutantRecord {
    MutantRecord {
        catalog_index: njutest_cli::report::CatalogIndex::new(0),
        id: id.to_owned(),
        display_id: id.get(..20).unwrap_or(id).to_owned(),
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
        accepted: false,
        reuse: njutest_cli::report::Reuse(if reused {
            njutest_cli::report::Established::ReadBackFrom("an-earlier-run".to_owned())
        } else {
            njutest_cli::report::Established::Here
        }),
        blind_in: Vec::new(),
        routing: None,
    }
}

/// One typed shard document, closed through the same checked lattice as production.
fn part_stated(
    run: &str,
    shard: &str,
    rows: Vec<MutantRecord>,
    vary: &dyn Fn(&mut BuildReport),
    stated: &dyn Fn(&mut BuildReport),
) -> ShardReport {
    let mut source = BuildReport::new(run, RunKind::Full, Contract::StandardV1);
    source.repository.workspace_digest = "a".repeat(64);
    source.repository.configuration_digest = "b".repeat(64);
    source.toolchain.rustc = "rustc 1.98.0".to_owned();
    source.scope.configured_builds = vec![njutest_cli::config::DEFAULT_CONFIGURATION.to_owned()];
    source.scope.shard = Some(shard.to_owned());
    source.timing.started = "2026-09-08T09:00:00Z".to_owned();
    source.timing.finished = "2026-09-08T09:10:00Z".to_owned();
    source.timing.duration_ms = 600_000;
    source.targets.push(TargetRecord {
        id: "one".to_owned(),
        name: "pkg/lib/pkg".to_owned(),
        package: "pkg".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 5,
        message: None,
    });
    source.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "this synthetic fixture has no repository process",
    ));
    vary(&mut source);
    source.count_targets().expect("one exact target accounting");
    source.mutants = rows;
    source.accounting.mutants = counted(&source.mutants);
    source.findings = source
        .mutants
        .iter()
        .filter_map(|mutant| {
            mutant
                .outcome
                .outcome()
                .required_finding(mutant.accepted)
                .map(|kind| Finding::new(kind, &mutant.display_id, "synthetic mutation finding"))
        })
        .collect();
    stated(&mut source);
    source.verdict = source.concluded();
    let measurements = njutest_cli::report::across::BuildMeasurements::checked(vec![(
        njutest_cli::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let envelope =
        RunId::try_from(format!("{run}-report")).expect("a canonical envelope namespace");
    let latticed = njutest_cli::report::across::configured(&envelope, &measurements)
        .expect("one checked shard lattice");
    let LatticedDocument::Shard(part) = latticed else {
        panic!("the sharded fixture cannot be a whole report");
    };
    part
}

fn part_varying(
    run: &str,
    shard: &str,
    rows: Vec<MutantRecord>,
    vary: &dyn Fn(&mut BuildReport),
) -> ShardReport {
    part_stated(run, shard, rows, vary, &|_| {})
}

fn part(run: &str, shard: &str, rows: Vec<MutantRecord>) -> ShardReport {
    part_stated(run, shard, rows, &|_| {}, &|_| {})
}

fn run_id(value: &str) -> RunId {
    RunId::try_from(value).expect("a canonical run id")
}

fn whole(run: &str, parts: &[ShardReport]) -> Report {
    let latticed = merge(&run_id(run), parts).expect("the parts of one catalog");
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

fn refused_as(parts: &[ShardReport]) -> MergeError {
    merge(&run_id("the-whole"), parts).expect_err("an incomplete shard set")
}

#[test]
fn the_whole_holds_every_mutant_its_parts_judged() {
    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part("two", "2/2", vec![row(1, &"b".repeat(64), "killed", false)]);

    let whole = whole("the-whole", &[one, two]);
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(conclusion.mutants.len(), 2, "{:?}", conclusion.mutants);
    assert_eq!(
        conclusion.accounting.mutants.cataloged, 2,
        "the accounting is derived again from what the whole holds, never averaged \
         from the parts: a part's numbers are over a different denominator"
    );
    assert_eq!(conclusion.accounting.mutants.killed, 2);
    assert_eq!(
        whole.verdict(),
        Verdict::Assured,
        "and the verdict is the one the whole supports, which is the thing a part \
         could not say"
    );
}

#[test]
fn nothing_is_not_a_catalog() {
    assert_eq!(
        merge(&run_id("the-whole"), &[]).expect_err("no parts"),
        MergeError::Nothing
    );
}

#[test]
fn a_whole_requires_every_shard_not_only_disjoint_rows() {
    let one_of_two = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    assert!(
        matches!(
            refused_as(&[one_of_two]),
            MergeError::MissingShard { index: 2, of: 2 }
        ),
        "a missing shard has no row with which to overlap, so disjoint rows do not prove a whole"
    );

    let one = part("one", "1/3", vec![row(0, &"a".repeat(64), "killed", false)]);
    let three = part(
        "three",
        "3/3",
        vec![row(2, &"c".repeat(64), "killed", false)],
    );
    assert!(
        matches!(
            refused_as(&[one, three]),
            MergeError::MissingShard { index: 2, of: 3 }
        ),
        "a gap in the middle is no more complete than a missing last part"
    );
}

#[test]
fn every_part_names_one_shared_denominator_and_one_distinct_index() {
    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two_of_three = part("two", "2/3", vec![row(1, &"b".repeat(64), "killed", false)]);
    assert!(
        matches!(
            refused_as(&[one, two_of_three]),
            MergeError::Denominator {
                index: 2,
                expected: 2,
                actual: 3
            }
        ),
        "disjoint rows cut with different denominators leave an unknown part"
    );

    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let another_one = part("two", "1/2", vec![row(0, &"b".repeat(64), "killed", false)]);
    assert!(
        matches!(
            refused_as(&[one, another_one]),
            MergeError::DuplicateShard { index: 1, of: 2 }
        ),
        "two disjoint documents with the same label still leave shard 2/2 absent"
    );
}

#[test]
fn a_label_the_engine_would_refuse_cannot_become_a_part() {
    let result = std::panic::catch_unwind(|| part("one", "part one", Vec::new()));
    assert!(
        result.is_err(),
        "a label the engine would refuse cannot prove which part this report judged"
    );
}

#[test]
fn parts_of_two_different_trees_are_not_parts_of_one_catalog() {
    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &|source| source.repository.workspace_digest = "c".repeat(64),
    );

    let refused = merge(&run_id("the-whole"), &[one, two]).expect_err("two trees");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the repository evidence"),
        "adding up answers about two different trees produces an answer about neither: \
         {refused}"
    );
}

#[test]
fn parts_that_answered_to_different_contracts_are_not_added_into_one_that_answered_to_either() {
    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &|source| source.contract = Contract::DeepV1,
    );

    let refused = merge(&run_id("the-whole"), &[one, two]).expect_err("two contracts");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the contract"),
        "a report is one claim that a contract was met, and a claim assembled from a \
         part that met it and a part that did not is true of neither: {refused}"
    );

    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &|source| source.repository.configuration_digest = "d".repeat(64),
    );
    let refused = merge(&run_id("the-whole"), &[one, two]).expect_err("two configurations");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the repository evidence"),
        "{refused}"
    );
}

#[test]
fn a_mutant_two_parts_both_judged_says_the_parts_were_cut_differently() {
    let both = "a".repeat(64);
    let one = part("one", "1/2", vec![row(0, &both, "killed", false)]);
    let two = part("two", "2/2", vec![row(1, &both, "survived", false)]);

    let refused = merge(&run_id("the-whole"), &[one, two]).expect_err("an overlap");
    assert!(
        matches!(&refused, MergeError::Unsound { because } if because.contains(&both)),
        "every mutant belongs to exactly one part, so two parts holding one of them is \
         two runs cut with different N: {refused}"
    );
}

#[test]
fn parts_with_different_effective_scopes_or_tool_versions_are_not_one_catalog() {
    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &|source| source.scope.requested_packages.push("only-this".to_owned()),
    );
    let refused = merge(&run_id("the-whole"), &[one, two]).expect_err("two selected package sets");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the requested scope"),
        "the same tree can be cataloged over different package subsets: {refused}"
    );

    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &|source| source.run_kind = RunKind::Changed,
    );
    let refused = merge(&run_id("the-whole"), &[one, two]).expect_err("two run scopes");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the run kind"),
        "a full and changed catalog do not add up to either: {refused}"
    );

    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &|source| source.tool.rust_mutants = "another engine".to_owned(),
    );
    let refused = merge(&run_id("the-whole"), &[one, two]).expect_err("two engine versions");
    assert!(
        matches!(&refused, MergeError::Disagree { about, .. } if *about == "the producer versions"),
        "two engine versions need not enumerate the same catalog: {refused}"
    );
}

#[test]
fn the_whole_counts_every_disposition_its_parts_held() {
    let one = part(
        "one",
        "1/2",
        vec![
            row(0, &"a".repeat(64), "killed", true),
            row(2, &"b".repeat(64), "survived", true),
            row(4, &"c".repeat(64), "compile-rejected", false),
            row(6, &"9".repeat(64), "unconfirmed", false),
        ],
    );
    let two = part(
        "two",
        "2/2",
        vec![
            row(1, &"d".repeat(64), "step-limit-reached", false),
            row(3, &"e".repeat(64), "unreached", false),
            row(5, &"f".repeat(64), "equivalent", false),
        ],
    );

    let counts = whole("the-whole", &[one, two])
        .conclusion()
        .expect("the checked whole has a representable conclusion")
        .accounting
        .mutants;

    assert_eq!(counts.cataloged, 7);
    assert_eq!(counts.killed, 1);
    assert_eq!(counts.survived, 1);
    assert_eq!(counts.rejected, 1);
    assert_eq!(counts.step_limit_reached, 1);
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
fn an_unmatched_acceptance_every_shard_reports_is_one_finding_in_the_whole() {
    let raised = Finding::new(
        FindingKind::UnmatchedAcceptance,
        "ffff",
        "no mutant matches this acceptance",
    );
    let one = part_stated(
        "one",
        "1/2",
        vec![row(0, &"a".repeat(64), "killed", false)],
        &|_| {},
        &|source| source.findings.push(raised.clone()),
    );
    let two = part_stated(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &|_| {},
        &|source| source.findings.push(raised.clone()),
    );

    let whole = whole("the-whole", &[one, two]);
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(conclusion.findings, vec![raised]);
    assert_eq!(whole.verdict(), Verdict::Insufficient);
}

#[test]
fn a_finding_only_one_part_raised_is_carried_by_the_whole() {
    let one = part(
        "one",
        "1/2",
        vec![row(0, &"a".repeat(64), "survived", false)],
    );
    let two = part("two", "2/2", vec![row(1, &"b".repeat(64), "killed", false)]);

    let whole = whole("the-whole", &[one, two]);
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(conclusion.findings.len(), 1, "{:?}", conclusion.findings);
    assert_eq!(
        whole.verdict(),
        Verdict::Insufficient,
        "a finding in one part is a finding of the whole, and the whole is what carries \
         the verdict it makes"
    );
}

#[test]
fn the_whole_ran_from_the_first_start_to_the_last_finish_and_cost_what_the_parts_cost() {
    let one = part_varying(
        "one",
        "1/2",
        vec![row(0, &"a".repeat(64), "killed", false)],
        &|source| {
            source.timing = Timing {
                started: "2026-09-08T10:00:00Z".to_owned(),
                finished: "2026-09-08T10:05:00Z".to_owned(),
                duration_ms: 300_000,
            };
        },
    );
    let two = part("two", "2/2", vec![row(1, &"b".repeat(64), "killed", false)]);

    let timing = whole("the-whole", &[one, two])
        .conclusion()
        .expect("the checked whole has a representable conclusion")
        .timing;

    assert_eq!(
        timing.wall().started(),
        "2026-09-08T09:00:00Z",
        "the earliest start"
    );
    assert_eq!(
        timing.wall().finished(),
        "2026-09-08T10:05:00Z",
        "the latest finish"
    );
    assert_eq!(
        timing.compute_total_ms(),
        900_000,
        "and the sum of what they cost, not the span between them: parts run on \
         different machines at once, and the wall clock of the whole is not the work"
    );
}

#[test]
fn the_targets_of_the_whole_are_ordered_the_way_a_report_orders_them() {
    let vary = |source: &mut BuildReport| {
        if let Some(first) = source.targets.first_mut() {
            first.duration_ms = 1;
        }
        source.targets.push(TargetRecord {
            id: "two".to_owned(),
            name: "pkg/test/slow".to_owned(),
            package: "pkg".to_owned(),
            status: TargetStatus::Passed,
            duration_ms: 900,
            message: None,
        });
        source.sort_targets();
    };
    let one = part_varying(
        "one",
        "1/2",
        vec![row(0, &"a".repeat(64), "killed", false)],
        &vary,
    );
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "killed", false)],
        &vary,
    );

    let targets = whole("the-whole", &[one, two])
        .conclusion()
        .expect("the checked whole has a representable conclusion")
        .targets;

    assert_eq!(
        targets
            .iter()
            .map(|target| target.duration_ms)
            .collect::<Vec<u64>>(),
        vec![900, 1],
        "slowest first, which is the order every report is read in: {:?}",
        targets
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
    let one = part("one", "1/2", vec![row(0, &"a".repeat(64), "killed", false)]);
    let two = part_varying(
        "two",
        "2/2",
        vec![row(1, &"b".repeat(64), "survived", false)],
        &|source| {
            source.limitations.push(Limitation::new(
                "custom-harness",
                "the target brings its own harness",
            ));
            source.candidates = vec![offered("bbbbbbbbbbbbbbbbbbbb")];
        },
    );

    let whole = whole("the-whole", &[one, two]);
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(
        conclusion.findings.len(),
        1,
        "a whole that carried only what its first part found would report a clean run \
         for every gap the other parts are the ones holding: {:?}",
        conclusion.findings
    );
    assert_eq!(
        conclusion.candidates.len(),
        1,
        "{:?}",
        conclusion.candidates
    );
    assert_eq!(
        whole.verdict(),
        Verdict::Insufficient,
        "and the finding one part raised is what the whole concludes from"
    );
}

#[test]
fn the_acceptances_of_the_whole_are_the_ones_its_parts_recorded_and_no_others() {
    let mut one_row = row(0, &"a".repeat(64), "survived", false);
    one_row.accepted = true;
    let mut two_row = row(1, &"b".repeat(64), "survived", false);
    two_row.accepted = true;
    let one = part("one", "1/3", vec![one_row]);
    let two = part("two", "2/3", vec![two_row]);
    let three = part(
        "three",
        "3/3",
        vec![row(2, &"c".repeat(64), "killed", false)],
    );

    let counts = whole("the-whole", &[one, two, three])
        .conclusion()
        .expect("the checked whole has a representable conclusion")
        .accounting
        .mutants;

    assert_eq!(
        counts.accepted, 2,
        "an acceptance is a fact about a reviewer, and every mutant belongs to exactly \
         one part, so the rows add up to what a reviewer recorded and to nothing more: \
         a whole that counted one acceptance nobody made would excuse a survivor nobody \
         looked at"
    );
}

/// `rows` with `target` recorded as having answered `outcome` about each of them.
fn answered_by(rows: Vec<MutantRecord>, target: &str, outcome: &str) -> Vec<MutantRecord> {
    rows.into_iter()
        .map(|mut record| {
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
            record
        })
        .collect()
}

#[test]
fn a_target_silent_in_one_part_and_noticing_in_another_is_accused_by_neither_nor_by_the_whole() {
    let one_rows = answered_by(
        vec![row(0, &"a".repeat(64), "survived", false)],
        "pkg/lib/pkg",
        "survived",
    );
    let two_rows = answered_by(
        vec![row(1, &"b".repeat(64), "killed", false)],
        "pkg/lib/pkg",
        "killed",
    );
    let one = part("one", "1/2", one_rows.clone());
    let two = part("two", "2/2", two_rows.clone());

    let conclusion = whole("the-whole", &[one, two])
        .conclusion()
        .expect("the checked whole has a representable conclusion");
    assert!(
        !conclusion
            .findings
            .iter()
            .any(|one| one.kind == FindingKind::HollowTarget),
        "a part has seen a slice of the catalog, and the merge states only what a \
         source stated: {:?}",
        conclusion.findings
    );
    let mut union = one_rows;
    union.extend(two_rows);
    assert!(
        njutest_cli::report::hollow::found(&union).is_empty(),
        "and over the whole of it the target noticed something, so nobody is accused"
    );
}

#[test]
fn a_target_that_noticed_nothing_in_the_whole_catalog_is_accused_by_the_merge() {
    let one_rows = answered_by(
        vec![row(0, &"a".repeat(64), "survived", false)],
        "pkg/lib/pkg",
        "survived",
    );
    let two_rows = answered_by(
        vec![row(1, &"b".repeat(64), "survived", false)],
        "pkg/lib/pkg",
        "survived",
    );
    let one = part("one", "1/2", one_rows.clone());
    let two = part("two", "2/2", two_rows.clone());

    let whole = whole("the-whole", &[one, two]);
    drop(whole);
    let mut union = one_rows;
    union.extend(two_rows);
    let accused = njutest_cli::report::hollow::found(&union);
    assert_eq!(accused.len(), 1, "{accused:?}");
    assert!(
        accused[0].detail.contains("2 mutations"),
        "over the whole catalog rather than over either part of it: {}",
        accused[0].detail
    );
}
