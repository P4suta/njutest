// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One report from the several builds of a project a run measured.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_cli::report::across::configured;
use njutest_cli::report::{
    Blind, BlindIn, Decided, MutantAccounting, MutantRecord, ObserverAccounting, Outcome, Position,
    Report, RunKind, Verdict,
};

fn report(run: &str, outcomes: &[(&str, &str)]) -> Report {
    let mut report = Report::new(
        run,
        RunKind::Full,
        njutest_cli::config::Contract::StandardV1,
    );
    report.verdict = Verdict::Assured;
    report.mutants = outcomes
        .iter()
        .map(|(id, outcome)| MutantRecord {
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
            outcome: Decided::of(
                Outcome::parse(outcome).unwrap_or(Outcome::Errored),
                Some("pkg/lib/pkg".to_owned()),
            )
            .or_else(|| Decided::of(Outcome::parse(outcome).unwrap_or(Outcome::Errored), None))
            .unwrap_or(Decided::Survived),
            reused: false,
            source_run_id: None,
            blind_in: Vec::new(),
            routing: None,
        })
        .collect();
    let mut observers = ObserverAccounting::default();
    for record in &report.mutants {
        observers.counted(record.outcome.decision());
    }
    report.accounting.mutants = MutantAccounting {
        cataloged: u32::try_from(report.mutants.len()).expect("a count"),
        observers,
        ..MutantAccounting::default()
    };
    report
}

#[test]
fn a_mutation_only_one_build_noticed_is_a_mutation_the_run_did_not_notice() {
    let whole = configured(&[
        ("default".to_owned(), report("r", &[("a", "killed")])),
        ("release".to_owned(), report("r", &[("a", "survived")])),
    ])
    .expect("two builds of one catalog");

    assert_eq!(
        whole.mutants[0].outcome.name(),
        "survived",
        "the release build is a program somebody ships, so a mutation nothing \
         noticed there is a mutation nothing noticed; letting the debug build \
         outvote it would turn measuring more into a way of claiming more"
    );
    assert_eq!(
        whole.mutants[0].blind_in,
        vec![BlindIn {
            build: "release".to_owned(),
            decision: Blind::Unnoticed,
        }],
        "and the run says which build and what that build established, because a \
         survivor everywhere and a survivor in one build are different things to act \
         on — and so are a build whose tests noticed nothing and one that established \
         nothing at all"
    );
    assert_eq!(whole.accounting.mutants.observers.unnoticed, 1);
    assert_eq!(whole.accounting.mutants.observers.tests, 0);
}

#[test]
fn a_mutation_every_build_noticed_is_noticed_and_names_no_build() {
    let whole = configured(&[
        ("default".to_owned(), report("r", &[("a", "killed")])),
        ("release".to_owned(), report("r", &[("a", "killed")])),
    ])
    .expect("two builds of one catalog");

    assert_eq!(whole.mutants[0].outcome.name(), "killed");
    assert!(
        whole.mutants[0].blind_in.is_empty(),
        "naming a build where nothing went wrong is a column a reader learns to skip"
    );
    assert_eq!(whole.accounting.mutants.observers.tests, 1);
}

#[test]
fn one_build_is_the_report_that_build_wrote() {
    let only = report("r", &[("a", "killed"), ("b", "survived")]);
    let whole = configured(&[("default".to_owned(), only.clone())]).expect("one build");
    assert_eq!(whole.mutants, only.mutants);
    assert_eq!(whole.accounting.mutants, only.accounting.mutants);
}

#[test]
fn builds_that_catalogued_different_mutations_are_not_builds_of_one_catalog() {
    let refused = configured(&[
        ("default".to_owned(), report("r", &[("a", "killed")])),
        ("release".to_owned(), report("r", &[("b", "killed")])),
    ]);
    assert!(
        refused.is_err(),
        "a mutation one build catalogued and another did not is a mutation the run \
         cannot answer for across them, and quietly taking the one answer it has \
         would report a build's silence as agreement"
    );
}

#[test]
fn nothing_to_reconcile_is_refused_rather_than_invented() {
    assert_eq!(
        configured(&[]).err(),
        Some(njutest_cli::report::across::ConfiguredError::Nothing),
        "a report of no builds would say a run established something it never \
         looked at"
    );
}

#[test]
fn a_build_that_established_nothing_is_named_apart_from_one_the_tests_are_blind_in() {
    let whole = configured(&[
        ("default".to_owned(), report("r", &[("a", "survived")])),
        ("release".to_owned(), report("r", &[("a", "errored")])),
    ])
    .expect("two builds of one catalog");

    assert_eq!(
        whole.mutants[0].blind_in,
        vec![
            BlindIn {
                build: "default".to_owned(),
                decision: Blind::Unnoticed,
            },
            BlindIn {
                build: "release".to_owned(),
                decision: Blind::Errored,
            },
        ],
        "a list of names alone would make a reader believe the same thing happened \
         in both. It did not: one build ran the tests and nothing noticed, and the \
         other never answered — and the first wants a test written where the second \
         wants somebody to find out why first"
    );
}
