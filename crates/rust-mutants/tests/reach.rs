// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Routing: which targets could notice a mutation, and what the route says when the measurement says nothing.

use std::path::Path;

use rust_mutants::coverage::{Block, Point};
use rust_mutants::reach::{Reached, UNMEASURED};
use rust_mutants::session::Route;

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

#[test]
fn a_route_without_a_measurement_is_every_target_and_says_why() {
    let route = Route::decide(
        &Reached::default(),
        Path::new("src/lib.rs"),
        at(3),
        &TARGETS,
    );
    assert_eq!(route.granularity(), "all");
    assert_eq!(route.fallback(), Some("not-measured"));
    assert_eq!(route.reaching(), TARGETS.to_vec());
}

#[test]
fn a_position_no_measurement_instrumented_is_one_nothing_is_known_about() {
    let reached = measured(&[("demo/lib/demo", 10)], &[("demo/lib/demo", &[10])]);
    let route = Route::decide(&reached, Path::new("src/lib.rs"), at(3), &TARGETS);
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
        &[("demo/lib/demo", &[3]), ("demo/test/parity", &[])],
    );
    let route = Route::decide(&reached, Path::new("src/lib.rs"), at(3), &TARGETS);
    assert_eq!(route.granularity(), "block");
    assert_eq!(route.fallback(), None);
    assert_eq!(route.reaching(), vec!["demo/lib/demo"]);
}

#[test]
fn a_measured_position_no_target_ran_is_unreached() {
    let reached = measured(&[("demo/lib/demo", 3)], &[("demo/lib/demo", &[10])]);
    let route = Route::decide(&reached, Path::new("src/lib.rs"), at(3), &TARGETS);
    assert_eq!(route.granularity(), "unreached");
    assert!(route.reaching().is_empty());
}

#[test]
fn a_target_the_measurement_could_not_read_is_kept_in_every_route() {
    let mut reached = measured(&[("demo/lib/demo", 3)], &[("demo/lib/demo", &[10])]);
    reached
        .limitations
        .push(format!("{UNMEASURED}:demo/test/parity"));
    let route = Route::decide(&reached, Path::new("src/lib.rs"), at(3), &TARGETS);
    assert_eq!(
        route.reaching(),
        vec!["demo/test/parity"],
        "a target whose profile could not be read is a target the measurement says nothing about, \
         and what nothing is known about is run"
    );
    assert_eq!(route.granularity(), "block");
    assert_eq!(route.fallback(), Some("coverage-incomplete"));
}
