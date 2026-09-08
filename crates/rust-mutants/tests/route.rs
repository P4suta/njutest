// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a route says about a mutation, read as a pure function of the record.
//!
//! The route is where a proof layer stops being an argument and becomes a
//! decision, so what it says about the targets it removed is as much part of
//! the answer as which ones it kept.

/// What one target's tests reached, as a table a case reads at a glance.
type Reached<'a> = &'a [(&'a str, &'a [u32])];

/// Every target of a record, each with what its tests reached.
type Records<'a> = &'a [(&'a str, Reached<'a>)];

use rust_mutants::session::{Fallback, Route, Routing};
use rust_mutants::touch::{Seen, TargetTouches, Touched};

/// A record in which each named target's tests reached exactly the mutations given.
fn touched(targets: Records<'_>, compared: &[u32]) -> Touched {
    let mut held = Touched::default();
    held.narrowing.compared = compared.iter().copied().collect();
    for (target, tests) in targets {
        let mut reached = Seen::default();
        for (test, indices) in *tests {
            drop(
                reached
                    .tests
                    .insert((*test).to_owned(), indices.iter().copied().collect()),
            );
        }
        let mut touches = TargetTouches::default();
        touches.reached = reached;
        touches.ran = tests.iter().map(|(test, _)| (*test).to_owned()).collect();
        drop(held.targets.insert((*target).to_owned(), touches));
    }
    held
}

#[test]
fn a_route_that_reaches_nothing_names_every_target_it_asked() {
    let record = touched(
        &[
            ("demo/lib/demo", &[("one", &[7])]),
            ("demo/test/wide", &[("two", &[7])]),
        ],
        &[],
    );
    let targets = ["demo/lib/demo", "demo/test/wide"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );

    assert_eq!(
        route.granularity(),
        "unreached",
        "nothing of either target reached mutation 1"
    );
    assert_eq!(
        route.considered(),
        targets,
        "and a layer that removed every execution has to say who it asked, or the word \
         `unreached` has nothing behind it and an audit can only confirm that the engine said it"
    );
}

#[test]
fn a_route_that_keeps_a_target_names_nobody_as_having_missed_it() {
    let record = touched(&[("demo/lib/demo", &[("one", &[1])])], &[]);
    let targets = ["demo/lib/demo"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );

    assert_eq!(route.reaching(), ["demo/lib/demo"]);
    assert!(
        route.considered().is_empty(),
        "one route cannot answer one question two ways: a target it kept is not one it says \
         reached nothing"
    );
}

#[test]
fn a_route_every_target_answered_says_nothing_widened_it() {
    let record = touched(&[("demo/lib/demo", &[("one", &[1])])], &[]);
    let targets = ["demo/lib/demo"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );
    assert_eq!(
        route.fallback(),
        None,
        "every target of this record was asked and answered, so nothing was widened back"
    );

    let unmeasured = Touched::default();
    let widened = Route::by_touch(
        &unmeasured,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );
    assert_eq!(
        widened.fallback(),
        Some(Fallback::NotMeasured.name()),
        "and a record that measured nothing says so rather than removing anything"
    );
}

#[test]
fn a_target_the_record_does_not_name_is_kept_and_says_the_route_is_incomplete() {
    let record = touched(&[("demo/lib/demo", &[("one", &[9])])], &[]);
    let targets = ["demo/lib/demo", "demo/test/absent"];
    let route = Route::by_touch(
        &record,
        1,
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );

    assert_eq!(
        route.reaching(),
        ["demo/test/absent"],
        "a target nothing was recorded about is one nothing was established about, and it stays"
    );
    assert_eq!(
        route.fallback(),
        Some(Fallback::TouchIncomplete.name()),
        "and the route says why it is wider than the record alone would make it"
    );
    assert!(
        route.considered().is_empty(),
        "a route that keeps something carries no such list: the target the record placed \
         elsewhere is dropped without being named, which is the asymmetry `considered` closes \
         only for a route that keeps nothing"
    );
}
