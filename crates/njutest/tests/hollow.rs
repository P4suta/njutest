// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target that was put to mutations and noticed none of them.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking"
)]

use njutest::report::{Answered, FindingKind, MutantRecord, Position, Routing, hollow::found};
use njutest::report::{Decided, Outcome, StepBoundary};

fn record(id: &str, outcome: &str, answered: &[(&str, &str)]) -> MutantRecord {
    let outcome = Outcome::parse(outcome).unwrap_or(Outcome::Errored);
    let boundary = (outcome == Outcome::StepLimitReached)
        .then(|| StepBoundary::new(10, 11).expect("a first count beyond the allowance"));
    MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
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
        blind_in: Vec::new(),
        routing: Some(Routing {
            granularity: rust_mutants::session::Granularity::Block,
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
                    outcome: Outcome::parse(said).unwrap_or(Outcome::Errored),
                })
                .collect(),
        }),
        reuse: njutest::report::Reuse(njutest::report::Established::Here),
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
    reused.reuse = njutest::report::Reuse(njutest::report::Established::ReadBackFrom(
        "20260905T081500Z-000000".to_owned(),
    ));
    assert!(
        found(&[reused]).is_empty(),
        "a run that read its answers back asked no target anything, and the empty \
         list says so rather than reading as everybody staying silent"
    );
}

#[test]
fn a_target_whose_answers_were_all_undecided_is_not_accused_of_noticing_nothing() {
    let findings = found(&[
        record("a", "unconfirmed", &[("broken", "unconfirmed")]),
        record("b", "errored", &[("broken", "errored")]),
    ]);
    assert!(
        findings.is_empty(),
        "a target whose harness would not start did not notice nothing — the run \
         established nothing about it. Counting those as chances it failed to take \
         accuses a broken harness of asserting nothing, and puts the count behind the \
         accusation: {findings:?}"
    );
}

#[test]
fn an_undecided_answer_is_not_counted_among_the_ones_a_target_did_give() {
    let findings = found(&[
        record("a", "survived", &[("blunt", "survived")]),
        record("b", "errored", &[("blunt", "errored")]),
    ]);
    assert_eq!(findings.len(), 1, "it answered once and noticed nothing");
    assert!(
        findings[0].detail.contains("1 mutation") && !findings[0].detail.contains("2 mutation"),
        "the count is the weight of the accusation, so it counts what was answered \
         and not what was attempted: {}",
        findings[0].detail
    );
}

#[test]
fn a_target_that_noticed_something_anywhere_in_the_catalog_is_not_accused_for_a_slice_of_it() {
    let whole = [
        record("a", "survived", &[("sharp", "survived")]),
        record("b", "killed", &[("sharp", "killed")]),
    ];
    assert!(
        found(&whole).is_empty(),
        "hollow is a statement about a target over the whole catalog; a target \
         silent in one part and noticing in another has noticed something, and a \
         conclusion that came out differently because of where the catalog was cut \
         is an artefact of the measurement rather than a fact about the suite: {:?}",
        found(&whole)
    );
    assert_eq!(
        found(&whole[..1]).len(),
        1,
        "and the slice on its own does read as hollow, which is why a part may not \
         raise it"
    );
}
