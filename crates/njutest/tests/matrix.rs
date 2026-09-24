// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every dimension's column is read off the records alone, each record placed by one exhaustive match (ADR 0033).

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use njutest::report::Limitation;
use njutest::report::faults::{FaultDecision, FaultRecord};
use njutest::report::matrix::{Column, Dimension, Evidence, pooled, rows};

fn fault(decision: FaultDecision) -> FaultRecord {
    FaultRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "c".repeat(64),
        display_id: "c".repeat(20),
        path: "src/lib.rs".to_owned(),
        item: "load".to_owned(),
        position: None,
        decision,
    }
}

fn column(evidence: &Evidence<'_>, dimension: Dimension) -> Column {
    rows(evidence)
        .into_iter()
        .find(|row| row.dimension == dimension)
        .map(|row| row.column)
        .expect("every dimension has a row")
}

const fn counts(column: &Column) -> Option<(usize, usize, usize)> {
    match column {
        Column::Measured {
            catalogued,
            answered,
            holes,
            ..
        } => Some((*catalogued, *answered, *holes)),
        Column::Unmeasured { .. } | Column::NotAsked | Column::NothingToAsk { .. } => None,
    }
}

#[test]
fn each_fault_decision_is_answered_a_hole_or_a_class_the_column_does_not_speak_about() {
    let faults: Vec<FaultRecord> = FaultDecision::every().into_iter().map(fault).collect();
    let evidence = Evidence {
        mutations: (4, 1),
        knobs: &[],
        faults: &faults,
        crashes: &[],
        concurrency: &[],
        seams: &[],
        limitations: &[],
        findings: &[],
    };
    let faulted = column(&evidence, Dimension::Fault);
    assert_eq!(counts(&faulted), Some((5, 3, 2)), "{faulted:?}");
    assert_eq!(
        counts(&column(&evidence, Dimension::Mutation)),
        Some((5, 4, 1))
    );
    assert_eq!(column(&evidence, Dimension::Repeatable), Column::NotAsked);
    assert!(
        matches!(
            column(&evidence, Dimension::Schedule),
            Column::NothingToAsk { .. }
        ),
        "with no test binary there is no schedule to ask about"
    );
}

#[test]
fn every_seam_that_could_not_be_watched_is_a_hole_of_its_own() {
    let unwatched = [
        Limitation::new("seam-not-watched", "payments could not be watched"),
        Limitation::new("seam-not-watched", "search could not be watched"),
    ];
    let evidence = Evidence {
        mutations: (0, 0),
        knobs: &[],
        faults: &[],
        crashes: &[],
        concurrency: &[],
        seams: &[],
        limitations: &unwatched,
        findings: &[],
    };
    assert_eq!(counts(&column(&evidence, Dimension::Wire)), Some((2, 0, 2)));
}

#[test]
fn a_hole_in_any_build_is_a_hole_of_every_build_together() {
    let measured = |catalogued, answered, holes| Column::Measured {
        catalogued,
        answered,
        holes,
        speaks_not_about: Vec::new(),
    };
    assert_eq!(
        counts(&pooled(vec![measured(3, 3, 0), measured(2, 1, 1)])),
        Some((5, 4, 1)),
        "where every build measured it, the counts add"
    );
    assert!(
        matches!(
            pooled(vec![
                measured(3, 3, 0),
                Column::Unmeasured {
                    why: "no baseline".to_owned()
                }
            ]),
            Column::Unmeasured { .. }
        ),
        "one build's unmeasured column is not hidden under another's counts"
    );
    assert_eq!(
        pooled(vec![measured(3, 3, 0), Column::NotAsked]),
        Column::NotAsked
    );
}

#[test]
fn a_whole_contract_puts_every_fault_and_knob_and_refuses_a_document_that_says_not_to() {
    let path = std::path::Path::new(".njutest.toml");
    let whole = njutest::config::Config::parse("version = 1\ncontract = \"whole-v1\"\n", path)
        .expect("a whole contract parses");
    assert!(whole.faults.inject, "whole-v1 puts every fault");
    assert_eq!(
        whole.repeatable.knobs,
        njutest::report::knobs::Knob::ALL.to_vec(),
        "and every knob"
    );
    for contradiction in [
        "[faults]\ninject = false\n",
        "[repeatable]\nknobs = [\"timezone\"]\n",
    ] {
        let refused = njutest::config::Config::parse(
            &format!("version = 1\ncontract = \"whole-v1\"\n{contradiction}"),
            path,
        );
        assert!(
            refused.is_err(),
            "a document that asks not to measure a dimension is not a whole run: {contradiction}"
        );
    }
}
