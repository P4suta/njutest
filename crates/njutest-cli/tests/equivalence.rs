// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which mutations nothing noticed are mutations nothing could have noticed.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a list this test built holds what this test put in it, and a shorter one is the failure it is here to report"
)]

use std::collections::BTreeSet;

use njutest_cli::assure::equivalence::{Decided, Refused, Standing, askable, settle};
use njutest_cli::assure::mutation::{Disposition, Judged};
use njutest_cli::assure::route::{Asked, Discharge, Fallback, NEVER_INFECTED, Reaches, Route};

fn block(targets: &[&str]) -> Route {
    Route::Block {
        reaching: targets
            .iter()
            .map(|one| Reaches {
                target: (*one).to_owned(),
                tests: Asked::Every,
            })
            .collect(),
        discharged: Vec::new(),
        fallback: None,
    }
}

const fn standing<'a>(route: &'a Route, unsafe_packages: &'a BTreeSet<String>) -> Standing<'a> {
    Standing {
        route,
        package: "core",
        unsafe_packages,
        tree_written: false,
        withdrawn: false,
    }
}

#[test]
fn a_mutation_in_code_no_test_calls_keeps_its_finding_whatever_the_compiler_did_with_it() {
    let none = BTreeSet::new();

    for (route, why) in [
        (
            Route::Unreached {
                considered: vec!["one".to_owned()],
            },
            Refused::NothingReached,
        ),
        (
            Route::Discharged {
                discharged: vec![Discharge {
                    target: "one".to_owned(),
                    proof: NEVER_INFECTED,
                }],
            },
            Refused::NothingReached,
        ),
    ] {
        assert_eq!(
            askable(standing(&route, &none)),
            Err(why),
            "the linker drops what nothing calls, and the artifacts then come out identical \
             for the opposite of a reassuring reason: {route:?}"
        );
    }
}

#[test]
fn a_route_widened_past_the_position_is_not_one_this_rests_on() {
    let none = BTreeSet::new();
    let everything = Route::All {
        reaching: vec!["one".to_owned()],
        fallback: Fallback::OutsideBlocks,
    };
    assert_eq!(
        askable(standing(&everything, &none)),
        Err(Refused::RouteWidened)
    );

    let unreadable = Route::Block {
        reaching: vec![Reaches {
            target: "one".to_owned(),
            tests: Asked::Every,
        }],
        discharged: Vec::new(),
        fallback: Some(Fallback::CoverageIncomplete),
    };
    assert_eq!(
        askable(standing(&unreadable, &none)),
        Err(Refused::RouteWidened),
        "a target is in this route because its measurement could not be read, which \
         says what it ran the file for and not what it ran the position for"
    );
}

#[test]
fn a_package_that_holds_unsafe_is_one_where_the_same_instructions_are_not_the_same_sentence() {
    let unsafe_packages: BTreeSet<String> = std::iter::once("core".to_owned()).collect();
    assert_eq!(
        askable(standing(&block(&["one"]), &unsafe_packages)),
        Err(Refused::PackageHoldsUnsafe)
    );
}

#[test]
fn a_tree_a_test_wrote_into_and_a_control_that_drifted_both_withdraw_the_answer() {
    let none = BTreeSet::new();
    let route = block(&["one"]);
    let mut written = standing(&route, &none);
    written.tree_written = true;
    assert_eq!(askable(written), Err(Refused::TreeWritten));

    let mut withdrawn = standing(&route, &none);
    withdrawn.withdrawn = true;
    assert_eq!(askable(withdrawn), Err(Refused::ControlWithdrawn));
}

#[test]
fn a_position_the_tests_ran_is_one_the_compiler_may_be_asked_about() {
    let none = BTreeSet::new();
    assert_eq!(askable(standing(&block(&["one"]), &none)), Ok(()));
}

fn survivor(display_id: &str) -> Judged {
    Judged {
        catalog_index: 0,
        id: display_id.repeat(4),
        display_id: display_id.to_owned(),
        path: "src/lib.rs".to_owned(),
        rule: "add-to-sub@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: String::new(),
        position: None,
        disposition: Disposition::Survived {
            route: block(&["core/lib/core"]),
        },
        source_run_id: None,
        routing: None,
    }
}

fn decided(display_id: &str, equivalent: bool) -> Decided {
    Decided {
        display_id: display_id.to_owned(),
        equivalent,
        detail: if equivalent {
            "the compiler renders it identically".to_owned()
        } else {
            "the compiler renders it differently".to_owned()
        },
    }
}

fn recording() -> njutest_cli::trace::Recorder {
    njutest_cli::trace::Recorder::new(
        njutest_cli::trace::Sink::Memory(njutest_cli::trace::MemorySink::unbounded()),
        njutest_cli::trace::Clock::stepping(
            jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
            std::time::Duration::from_secs(1),
        ),
        njutest_cli::trace::StartRecord::of(
            "20260909T000000Z-000001",
            njutest_cli::report::RunKind::Full,
            njutest_cli::config::Contract::StandardV1,
        ),
    )
}

#[test]
fn what_this_layer_decided_about_each_mutation_is_in_the_recording() {
    let cancel = rust_mutants::runner::Cancel::new();
    let trace = recording();
    let mut judged = vec![survivor("aaaa"), survivor("bbbb")];

    settle(
        &mut judged,
        &[decided("aaaa", false), decided("bbbb", true)],
        njutest_cli::watch::Watch::new(&cancel, &trace),
    )
    .expect("one answer per mutation");

    let notes: Vec<String> = trace
        .events()
        .iter()
        .filter_map(|event| {
            njutest_cli::testkit::payload::of(&event.payload)
                .note()
                .map(|note| format!("{} {}", note.kind, note.detail))
        })
        .collect();
    assert_eq!(
        notes.len(),
        2,
        "this layer removes findings, and a run that removed one has to say which and \
         why: a proof nobody can read back is one nobody can audit, which is what ADR \
         0004 decision 4 asks of every layer: {notes:?}"
    );
    for (mutant, said) in [("aaaa", "differently"), ("bbbb", "identically")] {
        assert!(
            notes.iter().any(|note| note.starts_with("equivalence ")
                && note.contains(mutant)
                && note.contains(said)),
            "and it says what the compiler answered about each, under a topic a reader \
             can filter for: {notes:?}"
        );
    }
}

#[test]
fn one_mutation_this_layer_says_nothing_about_does_not_end_what_it_says_about_the_rest() {
    let cancel = rust_mutants::runner::Cancel::new();
    let trace = njutest_cli::trace::Recorder::disabled();
    let mut judged = vec![
        survivor("aaaa"),
        survivor("bbbb"),
        survivor("cccc"),
        survivor("dddd"),
    ];

    settle(
        &mut judged,
        &[
            decided("aaaa", false),
            decided("cccc", true),
            decided("dddd", true),
        ],
        njutest_cli::watch::Watch::new(&cancel, &trace),
    )
    .expect("one answer per mutation");

    let equivalent: Vec<&str> = judged
        .iter()
        .filter(|one| matches!(one.disposition, Disposition::Equivalent { .. }))
        .map(|one| one.display_id.as_str())
        .collect();
    assert_eq!(
        equivalent,
        vec!["cccc", "dddd"],
        "the first mutation the compiler renders differently, and the one this layer was \
         never asked about, are two reasons to move on and neither is a reason to stop: \
         a layer that stopped would leave every later equivalence reported as a gap in \
         the tests, and nothing in the report would say the layer had given up"
    );
    assert!(
        matches!(judged[0].disposition, Disposition::Survived { .. })
            && matches!(judged[1].disposition, Disposition::Survived { .. }),
        "and what it says nothing about it leaves alone"
    );
}
