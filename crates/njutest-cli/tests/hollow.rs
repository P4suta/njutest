// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target that was put to mutations and noticed none of them.

#![expect(
    clippy::indexing_slicing,
    reason = "a test asserts with panics and reads as a table; no finding where this reads one is the failure it is here to report"
)]

use njutest_cli::report::{Answered, FindingKind, MutantRecord, Position, Routing, hollow::found};

fn record(id: &str, outcome: &str, answered: &[(&str, &str)]) -> MutantRecord {
    MutantRecord {
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
        outcome: outcome.to_owned(),
        blind_in: Vec::new(),
        routing: Some(Routing {
            granularity: "block".to_owned(),
            reaching: answered
                .iter()
                .map(|(name, _)| (*name).to_owned())
                .collect(),
            discharged: Vec::new(),
            fallback: None,
            answered: answered
                .iter()
                .map(|(name, said)| Answered {
                    target: (*name).to_owned(),
                    outcome: (*said).to_owned(),
                })
                .collect(),
        }),
        killed_by: None,
        reused: false,
        source_run_id: None,
    }
}

#[test]
fn a_target_asked_about_mutations_that_noticed_none_is_named_with_how_many() {
    let findings = found(&[
        record("a", "killed", &[("sharp", "killed")]),
        record(
            "b",
            "survived",
            &[("sharp", "survived"), ("blunt", "survived")],
        ),
        record(
            "c",
            "survived",
            &[("sharp", "survived"), ("blunt", "survived")],
        ),
    ]);

    assert_eq!(
        findings.len(),
        1,
        "`sharp` noticed one, so it is not hollow; `blunt` was asked twice and \
         noticed nothing: {findings:?}"
    );
    assert_eq!(findings[0].kind, FindingKind::HollowTarget);
    assert_eq!(findings[0].subject, "blunt");
    assert!(
        findings[0].detail.contains('2'),
        "how many it was asked about is what a reader weighs: one missed mutation is \
         a coincidence and two hundred is a suite that asserts nothing. The finding \
         carries the number rather than a threshold somebody chose: {}",
        findings[0].detail
    );
}

#[test]
fn a_target_that_was_never_asked_is_not_a_target_that_noticed_nothing() {
    let findings = found(&[record("a", "killed", &[("sharp", "killed")])]);
    assert!(
        findings.is_empty(),
        "`blunt` reached nothing here and was asked nothing, and a run that never \
         put a mutation to a target has established nothing about it: {findings:?}"
    );
}

#[test]
fn a_target_outranked_every_time_is_never_called_hollow() {
    let findings = found(&[
        record("a", "killed", &[("fast", "killed")]),
        record("b", "killed", &[("fast", "killed")]),
    ]);
    assert!(
        findings.is_empty(),
        "`slow` reached both and answered neither, because `fast` detected first \
         and the run stopped. Reading that as `slow` noticing nothing is the \
         mistake this finding exists to avoid: {findings:?}"
    );
}

#[test]
fn a_run_that_reused_every_answer_asked_nobody_and_accuses_nobody() {
    let mut reused = record("a", "survived", &[]);
    reused.reused = true;
    assert!(
        found(&[reused]).is_empty(),
        "a run that read its answers back asked no target anything, and the empty \
         list says so rather than reading as everybody staying silent"
    );
}
