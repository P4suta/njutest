// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which mutations nothing noticed are mutations nothing could have noticed.

use std::collections::BTreeSet;

use mjutest_cli::assure::equivalence::{Refused, Standing, askable};
use mjutest_cli::assure::route::{Asked, Discharge, Fallback, NEVER_INFECTED, Reaches, Route};

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
