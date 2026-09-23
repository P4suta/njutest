// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One report from the several builds of a project a run measured.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::assigning_clones,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest::report::across::configured;
use njutest::report::{
    Blind, BlindIn, BuildName, BuildReport, BuildSelection, CatalogPart, Decided, Decision,
    Finding, Limitation, MutantAccounting, MutantRecord, Outcome, Position, Report, RunKind,
    StepBoundary, TargetRecord, TargetStatus,
};
use rust_mutants::id::RunId;

fn build() -> BuildSelection {
    rust_mutants::cargo::BuildConfig::default().selection()
}

fn for_builds(mut report: BuildReport, names: &[&str]) -> BuildReport {
    report.scope.configured_builds = names.iter().map(|name| (*name).to_owned()).collect();
    report
}

fn run_id(value: &str) -> RunId {
    RunId::try_from(value).expect("a canonical run id")
}

fn name(value: &str) -> BuildName {
    BuildName::try_from(value).expect("a canonical configured build name")
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

fn report(run: &str, outcomes: &[(&str, &str)]) -> BuildReport {
    let mut report = BuildReport::new(run, RunKind::Full, njutest::config::Contract::StandardV1);
    report.timing.started = "2026-09-08T00:00:00Z".to_owned();
    report.timing.finished = "2026-09-08T00:00:00Z".to_owned();
    report.timing.duration_ms = 1;
    report.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "this synthetic fixture has no repository process",
    ));
    report.targets.push(TargetRecord {
        id: "target".to_owned(),
        name: "pkg/lib/pkg".to_owned(),
        package: "pkg".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 1,
        message: None,
    });
    report.count_targets().expect("one exact target accounting");
    report.mutants = outcomes
        .iter()
        .zip(0u32..)
        .map(|((id, outcome), catalog_index)| {
            let outcome = Outcome::parse(outcome).unwrap_or(Outcome::Errored);
            let boundary = (outcome == Outcome::StepLimitReached)
                .then(|| StepBoundary::new(10, 11).expect("the first count beyond the allowance"));
            MutantRecord {
                catalog_index: njutest::report::CatalogIndex::new(catalog_index),
                id: id.repeat(64),
                display_id: id.repeat(20),
                path: "src/lib.rs".to_owned(),
                position: Position {
                    line: 8,
                    column: 9,
                    character_column: 9,
                },
                rule: "gt-to-ge@1".to_owned(),
                item: "sign".to_owned(),
                original: ">".to_owned(),
                replacement: ">=".to_owned(),
                outcome: Decided::of(outcome, Some("pkg/lib/pkg".to_owned()), boundary)
                    .or_else(|| Decided::of(outcome, None, boundary))
                    .unwrap_or(Decided::Survived),
                accepted: false,
                reuse: njutest::report::Reuse(njutest::report::Established::Here),
                blind_in: Vec::new(),
                routing: None,
            }
        })
        .collect();
    report.accounting.mutants = counted(&report.mutants);
    report.findings = report
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
    njutest::testkit::read_every_named_file(&mut report);
    report.verdict = report.concluded();
    report
}

fn accept_first(mut report: BuildReport) -> BuildReport {
    report.mutants[0].accepted = true;
    report.accounting.mutants = counted(&report.mutants);
    report.findings.clear();
    njutest::testkit::read_every_named_file(&mut report);
    report.verdict = report.concluded();
    report
}

/// The whole-catalog report these checked measurements complete into.
fn completed(run: &str, measured: Vec<(String, BuildSelection, BuildReport)>) -> Report {
    let measurements = njutest::report::across::BuildMeasurements::checked(measured)
        .expect("checked build measurements");
    let latticed = configured(&run_id(run), &measurements).expect("builds of one catalog");
    let njutest::report::LatticedDocument::Complete(whole) = latticed else {
        panic!("a whole-catalog fixture cannot be a shard");
    };
    whole
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

#[test]
fn a_mutation_only_one_build_noticed_is_a_mutation_the_run_did_not_notice() {
    let whole = completed(
        "the-run",
        vec![
            (
                "default".to_owned(),
                build(),
                for_builds(report("r", &[("a", "killed")]), &["default", "release"]),
            ),
            (
                "release".to_owned(),
                build(),
                for_builds(report("r-1", &[("a", "survived")]), &["default", "release"]),
            ),
        ],
    );
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(
        conclusion.mutants[0].decision(),
        Decision::Unnoticed,
        "the release build is a program somebody ships, so a mutation nothing \
         noticed there is a mutation nothing noticed; letting the debug build \
         outvote it would turn measuring more into a way of claiming more"
    );
    assert_eq!(
        conclusion.mutants[0].blind_in(),
        [BlindIn {
            build: name("release"),
            decision: Blind::Unnoticed,
        }]
        .as_slice(),
        "and the run says which build and what that build established, because a \
         survivor everywhere and a survivor in one build are different things to act \
         on — and so are a build whose tests noticed nothing and one that established \
         nothing at all"
    );
    assert_eq!(conclusion.accounting.mutants.observers.unnoticed, 1);
    assert_eq!(conclusion.accounting.mutants.observers.tests, 0);
}

#[test]
fn a_mutation_every_build_noticed_is_noticed_and_names_no_build() {
    let whole = completed(
        "the-run",
        vec![
            (
                "default".to_owned(),
                build(),
                for_builds(report("r", &[("a", "killed")]), &["default", "release"]),
            ),
            (
                "release".to_owned(),
                build(),
                for_builds(report("r-1", &[("a", "killed")]), &["default", "release"]),
            ),
        ],
    );
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(conclusion.mutants[0].decision(), Decision::Tests);
    assert!(
        conclusion.mutants[0].blind_in().is_empty(),
        "naming a build where nothing went wrong is a column a reader learns to skip"
    );
    assert_eq!(conclusion.accounting.mutants.observers.tests, 1);
}

#[test]
fn one_build_is_the_report_that_build_wrote() {
    let only = report("r", &[("a", "killed"), ("b", "survived")]);
    let whole = completed(
        "the-run",
        vec![(
            "default".to_owned(),
            build(),
            for_builds(only.clone(), &["default"]),
        )],
    );
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");
    assert_eq!(
        conclusion
            .mutants
            .iter()
            .map(|mutant| mutant.by_build()[0].outcome().name())
            .collect::<Vec<_>>(),
        only.mutants
            .iter()
            .map(|mutant| mutant.outcome.name())
            .collect::<Vec<_>>()
    );
    assert_eq!(conclusion.accounting.mutants, only.accounting.mutants);
}

#[test]
fn builds_that_catalogued_different_mutations_are_not_builds_of_one_catalog() {
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![
        (
            "default".to_owned(),
            build(),
            for_builds(report("r", &[("a", "killed")]), &["default", "release"]),
        ),
        (
            "release".to_owned(),
            build(),
            for_builds(report("r-1", &[("b", "killed")]), &["default", "release"]),
        ),
    ])
    .expect("checked build measurements");
    let refused = configured(&run_id("the-run"), &measurements);
    assert!(
        refused.is_err(),
        "a mutation one build catalogued and another did not is a mutation the run \
         cannot answer for across them, and quietly taking the one answer it has \
         would report a build's silence as agreement"
    );
}

#[test]
fn nothing_to_reconcile_is_refused_rather_than_invented() {
    let refusal = njutest::report::across::BuildMeasurements::checked(Vec::new());
    assert_eq!(
        refusal,
        Err(njutest::report::across::BuildMeasurementsError::Empty),
        "a report of no builds would say a run established something it never \
         looked at"
    );
}

#[test]
fn a_build_that_established_nothing_is_named_apart_from_one_the_tests_are_blind_in() {
    let whole = completed(
        "the-run",
        vec![
            (
                "default".to_owned(),
                build(),
                for_builds(report("r", &[("a", "survived")]), &["default", "release"]),
            ),
            (
                "release".to_owned(),
                build(),
                for_builds(report("r-1", &[("a", "errored")]), &["default", "release"]),
            ),
        ],
    );
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(
        conclusion.mutants[0].blind_in(),
        [
            BlindIn {
                build: name("default"),
                decision: Blind::Unnoticed,
            },
            BlindIn {
                build: name("release"),
                decision: Blind::Errored,
            },
        ]
        .as_slice(),
        "a list of names alone would make a reader believe the same thing happened \
         in both. It did not: one build ran the tests and nothing noticed, and the \
         other never answered — and the first wants a test written where the second \
         wants somebody to find out why first"
    );
}

#[test]
fn one_builds_acceptance_cannot_answer_another_builds_survivor() {
    let whole = completed(
        "the-run",
        vec![
            (
                "default".to_owned(),
                build(),
                for_builds(
                    accept_first(report("r", &[("a", "survived")])),
                    &["default", "release"],
                ),
            ),
            (
                "release".to_owned(),
                build(),
                for_builds(report("r-1", &[("a", "survived")]), &["default", "release"]),
            ),
        ],
    );
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(whole.verdict(), njutest::report::Verdict::Insufficient);
    assert_eq!(conclusion.findings.len(), 1);
    assert_eq!(
        conclusion.findings[0].origin,
        njutest::report::FindingOrigin::Source {
            build: name("release"),
            run_id: run_id("r-1"),
            part: CatalogPart::Whole,
        }
    );
    assert_eq!(
        conclusion.mutants[0].blind_in(),
        [BlindIn {
            build: name("release"),
            decision: Blind::Unnoticed,
        }]
        .as_slice()
    );
}

#[test]
fn a_weaker_accepted_survivor_cannot_hide_an_unaccepted_unreached_build() {
    let whole = completed(
        "the-run",
        vec![
            (
                "default".to_owned(),
                build(),
                for_builds(
                    accept_first(report("r", &[("a", "survived")])),
                    &["default", "release"],
                ),
            ),
            (
                "release".to_owned(),
                build(),
                for_builds(
                    report("r-1", &[("a", "unreached")]),
                    &["default", "release"],
                ),
            ),
        ],
    );
    let conclusion = whole
        .conclusion()
        .expect("the checked whole has a representable conclusion");

    assert_eq!(conclusion.mutants[0].decision(), Decision::Unnoticed);
    assert_eq!(
        conclusion.mutants[0].blind_in(),
        [BlindIn {
            build: name("release"),
            decision: Blind::Unreached,
        }]
        .as_slice(),
        "the acceptance one build holds cannot answer the row another build never ran"
    );
    assert_eq!(whole.verdict(), njutest::report::Verdict::Insufficient);
    assert_eq!(conclusion.findings.len(), 1);
    assert_eq!(
        conclusion.findings[0].origin,
        njutest::report::FindingOrigin::Source {
            build: name("release"),
            run_id: run_id("r-1"),
            part: CatalogPart::Whole,
        }
    );
}
