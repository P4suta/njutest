// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Routing: which targets could notice a mutation, and what the route says when the measurement says nothing.

use std::path::Path;

use rust_mutants::coverage::{Block, Point};
use rust_mutants::reach::{Reached, UNMEASURED};
use rust_mutants::session::{Route, Routing};

/// A measurement in which `covered` ran and `instrumented` was built, over one file.
fn measured(instrumented: &[(&str, u32)], covered: &[(&str, &[u32])]) -> Reached {
    let block = |line: u32| Block {
        file: "src/lib.rs".into(),
        start: Point { line, column: 1 },
        end: Point { line, column: 80 },
    };
    Reached {
        targets: covered
            .iter()
            .map(|(target, lines)| {
                (
                    (*target).to_owned(),
                    lines.iter().map(|line| block(*line)).collect(),
                )
            })
            .collect(),
        instrumented: instrumented.iter().map(|(_, line)| block(*line)).collect(),
        limitations: Vec::new(),
    }
}

const fn at(line: u32) -> Point {
    Point { line, column: 5 }
}

const TARGETS: [&str; 3] = ["demo/lib/demo", "demo/test/parity", "demo/doc/demo"];

/// Everything a coverage build compiles, which is every target but the documented examples.
const MEASURABLE: [&str; 2] = ["demo/lib/demo", "demo/test/parity"];

#[test]
fn a_route_without_a_measurement_is_every_target_and_says_why() {
    let route = Route::decide(
        &Reached::default(),
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(route.granularity(), "all");
    assert_eq!(route.fallback(), Some("not-measured"));
    assert_eq!(route.reaching(), TARGETS.to_vec());
}

#[test]
fn a_position_no_measurement_instrumented_is_one_nothing_is_known_about() {
    let reached = measured(&[("demo/lib/demo", 10)], &[("demo/lib/demo", &[10])]);
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(
        route.granularity(),
        "all",
        "a place the build never instrumented is a place the measurement says nothing about"
    );
    assert_eq!(route.fallback(), Some("outside-blocks"));
}

#[test]
fn a_measured_position_is_routed_to_the_targets_that_ran_it() {
    let reached = measured(
        &[("demo/lib/demo", 3)],
        &[
            ("demo/lib/demo", &[3]),
            ("demo/test/parity", &[]),
            ("demo/doc/demo", &[]),
        ],
    );
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(route.granularity(), "block");
    assert_eq!(
        route.fallback(),
        None,
        "the measurement named every target the run built, so nothing is being fallen back on"
    );
    assert_eq!(route.reaching(), vec!["demo/lib/demo"]);
}

#[test]
fn a_measured_position_no_target_ran_is_unreached() {
    let reached = measured(
        &[("demo/lib/demo", 3)],
        &[
            ("demo/lib/demo", &[10]),
            ("demo/test/parity", &[]),
            ("demo/doc/demo", &[]),
        ],
    );
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(route.granularity(), "unreached");
    assert!(route.reaching().is_empty());
}

#[test]
fn a_measurement_that_names_only_some_of_the_targets_narrows_to_none_of_them() {
    let reached = measured(&[("demo/lib/demo", 3)], &[("demo/lib/demo", &[10])]);
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(
        route.reaching(),
        vec!["demo/test/parity"],
        "the one target the measurement named ran somewhere else, and the one it could have \
         named and did not is one nothing is known about. The documented examples are not \
         missing from the measurement, they are not what it is about."
    );
    assert_eq!(route.fallback(), Some("coverage-incomplete"));
}

#[test]
fn a_target_the_measurement_could_not_read_is_kept_in_every_route() {
    let mut reached = measured(
        &[("demo/lib/demo", 3)],
        &[
            ("demo/lib/demo", &[10]),
            ("demo/test/parity", &[]),
            ("demo/doc/demo", &[]),
        ],
    );
    reached
        .limitations
        .push(format!("{UNMEASURED}:demo/test/parity"));
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(
        route.reaching(),
        vec!["demo/test/parity"],
        "a target whose profile could not be read is a target the measurement says nothing about, \
         and what nothing is known about is run"
    );
    assert_eq!(route.granularity(), "block");
    assert_eq!(route.fallback(), Some("coverage-incomplete"));
}

#[test]
fn a_target_a_measurement_says_nothing_about_reaches_by_being_named() {
    let reached = measured(
        &[("demo/lib/demo", 3)],
        &[
            ("demo/lib/demo", &[10]),
            ("demo/test/parity", &[]),
            ("demo/doc/demo", &[]),
        ],
    );
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[TARGETS[2]],
        },
    );
    assert_eq!(route.granularity(), "block");
    assert!(
        route.reaching().contains(&TARGETS[2]),
        "a library's documented examples are compiled by rustdoc while cargo runs them, so no \
         coverage build instruments them and routing them by file is the widest a fallback \
         goes: {:?}",
        route.reaching()
    );
}

#[test]
fn what_an_execution_narrows_to_is_what_the_route_says_and_nothing_else() {
    let mut reached = measured(
        &[("demo/lib/demo", 3)],
        &[
            ("demo/lib/demo", &[3]),
            ("demo/test/parity", &[]),
            ("demo/doc/demo", &[]),
        ],
    );
    reached
        .limitations
        .push(format!("{UNMEASURED}:demo/test/parity"));
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(
        route.narrowing(),
        Some(vec![
            "demo/lib/demo".to_owned(),
            "demo/test/parity".to_owned()
        ]),
        "a target whose profile could not be read is one the measurement says nothing about,          and dropping it from what runs is a survivor nobody measured"
    );

    let nothing = Route::decide(
        &Reached::default(),
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(
        nothing.narrowing(),
        None,
        "a route that is everything narrows nothing, which is what lets a run that measured          no coverage run every target it selected"
    );

    let unreached = Route::decide(
        &measured(
            &[("demo/lib/demo", 3)],
            &[
                ("demo/lib/demo", &[10]),
                ("demo/test/parity", &[]),
                ("demo/doc/demo", &[]),
            ],
        ),
        Path::new("src/lib.rs"),
        at(3),
        &Routing {
            targets: &TARGETS,
            measurable: &MEASURABLE,
            also_reaching: &[],
        },
    );
    assert_eq!(
        unreached.narrowing(),
        Some(Vec::new()),
        "a mutation no measured target executes is one nothing runs"
    );
}

#[test]
fn a_target_the_measurement_never_names_is_one_nothing_is_known_about() {
    let mut reached = measured(&[("src/lib.rs", 10)], &[("demo/test/one", &[10])]);
    reached.targets.insert(
        "demo/test/two".to_owned(),
        std::collections::BTreeSet::new(),
    );

    let targets = ["demo/test/one", "demo/test/two", "demo/test/three"];
    let route = Route::decide(
        &reached,
        Path::new("src/lib.rs"),
        Point {
            line: 10,
            column: 1,
        },
        &Routing {
            targets: &targets,
            measurable: &targets,
            also_reaching: &[],
        },
    );
    let reaching = route.reaching();
    assert!(
        reaching.contains(&"demo/test/one"),
        "the target whose run covered it can notice it: {reaching:?}"
    );
    assert!(
        !reaching.contains(&"demo/test/two"),
        "a target the measurement read and which covered nothing there cannot: {reaching:?}"
    );
    assert!(
        reaching.contains(&"demo/test/three"),
        "a target the measurement never names is one nothing was established about, and a route \
         that drops it turns a kill into a survivor. Being absent from a measurement is not the \
         same as being measured and covering nothing: {reaching:?}"
    );
}

#[test]
fn a_measurement_that_could_not_read_a_target_is_not_one_to_remember() {
    let whole = measured(
        &[("demo/lib/demo", 3)],
        &[("demo/lib/demo", &[3]), ("demo/test/parity", &[])],
    );
    assert!(
        whole.whole(),
        "a measurement that read every target it set out to is one a later run of the same tree \
         can stand on"
    );
    assert!(whole.measured(), "and it measured something");
    let mut partial = whole;
    partial
        .limitations
        .push(format!("{UNMEASURED}:demo/test/parity"));
    assert!(
        !partial.whole(),
        "a measurement of some of the targets is sound to route by, because what it could not \
         read stays in every route, and wrong to keep, because a later run would have nothing \
         to tell it from a whole one"
    );
}
