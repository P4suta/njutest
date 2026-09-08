// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Combining the parts of one catalog into the report the whole would have written.

use mjutest_cli::config::Contract;
use mjutest_cli::report::merge::{MergeError, merge};
use mjutest_cli::report::{
    MutantRecord, Position, Report, RunKind, TargetRecord, TargetStatus, Verdict,
};

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
            position: Position {
                line: 1,
                column: 1,
                character_column: 1,
            },
            outcome: (*outcome).to_owned(),
            killed_by: Some("pkg/lib/pkg".to_owned()),
            reused: false,
            source_run_id: None,
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
    let two = part("2/3", &[(both.as_str(), "survived")]);

    let refused = merge(&[one, two]).expect_err("an overlap");
    assert!(
        matches!(&refused, MergeError::Overlapping { mutant } if mutant == &both),
        "every mutant belongs to exactly one part, so two parts holding one of them is \
         two runs cut with different N: {refused}"
    );
}
