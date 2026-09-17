// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a route means to a report.

use njutest_cli::assure::route::{
    Asked, Discharge, Fallback, NEVER_INFECTED, Reaches, Route, detail, nothing_ran,
    ran_the_position,
};

fn reaches(target: &str) -> Reaches {
    Reaches {
        target: target.to_owned(),
        tests: Asked::Every,
    }
}

const fn decided(reaching: Vec<Reaches>) -> Route {
    Route::Block {
        reaching,
        discharged: Vec::new(),
        fallback: None,
    }
}

#[test]
fn every_fallback_a_route_can_carry_has_a_sentence_of_its_own() {
    let generic = detail("a name no release of this runner has ever emitted");
    for fallback in Fallback::ALL {
        let said = detail(fallback.name());
        assert_ne!(
            said,
            generic,
            "a route widened by {} reaches a report with no sentence a reader can act on: \
             the engine's vocabulary grew and this one did not",
            fallback.name()
        );
        assert!(!said.is_empty(), "{} says nothing at all", fallback.name());
    }
}

#[test]
fn a_name_this_release_does_not_know_still_says_which_direction_it_went() {
    assert_eq!(
        detail("a fallback from a later engine"),
        "the measurement did not decide it, so more was run rather than less",
        "an unknown fallback is still a fallback, and every fallback runs more"
    );
}

#[test]
fn only_a_route_the_measurement_decided_says_the_tests_ran_the_position() {
    assert!(
        ran_the_position(&decided(vec![reaches("core/lib/core")])),
        "the measurement placed a target at the position and kept it"
    );
}

#[test]
fn a_route_the_measurement_widened_did_not_run_the_position() {
    let widened = Route::Block {
        reaching: vec![reaches("core/lib/core")],
        discharged: Vec::new(),
        fallback: Some(Fallback::TouchIncomplete),
    };
    assert!(
        !ran_the_position(&widened),
        "a target is in this route because its record could not be read, which says \
         what it ran the file for and not what it ran the position for"
    );
    assert!(
        !ran_the_position(&Route::All {
            reaching: vec!["core/lib/core".to_owned()],
            fallback: Fallback::NotMeasured,
        }),
        "nothing was measured, so nothing is known about the position"
    );
    assert!(
        !nothing_ran(&Route::All {
            reaching: vec!["core/lib/core".to_owned()],
            fallback: Fallback::NotMeasured,
        }),
        "a route that runs every target runs a great deal"
    );
}

#[test]
fn a_route_that_kept_nothing_ran_nothing_and_proved_nothing() {
    let discharged = Route::Discharged {
        discharged: vec![Discharge {
            target: "core/lib/core".to_owned(),
            proof: NEVER_INFECTED,
        }],
    };
    let unreached = Route::Unreached {
        considered: vec!["core/lib/core".to_owned()],
    };
    let empty = decided(Vec::new());
    for route in [&discharged, &unreached, &empty] {
        assert!(nothing_ran(route), "{route:?} kept no target");
        assert!(
            !ran_the_position(route),
            "{route:?} ran nothing, so it did not run the position: identical code \
             there says the code is untested rather than that the mutation is \
             unobservable"
        );
    }
}
