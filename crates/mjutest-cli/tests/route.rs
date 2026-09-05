// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which tests could notice one mutation.

use std::collections::BTreeSet;
use std::path::PathBuf;

use mjutest_cli::assure::baseline::Measured;
use mjutest_cli::assure::route::{Body, Fallback, Proven, Route, discharge, route};
use mjutest_cli::coverage::{Block, Point};
use mjutest_cli::report::TargetStatus;
use mjutest_cli::targets::{Target, UnitKind};

const fn at(line: u32, column: u32) -> Point {
    Point { line, column }
}

fn block(file: &str, from: (u32, u32), to: (u32, u32)) -> Block {
    Block {
        file: PathBuf::from(file),
        start: at(from.0, from.1),
        end: at(to.0, to.1),
    }
}

/// One measured target, with what it reached and what it cost.
fn measured(id: &str, duration_ms: u64, status: TargetStatus, covered: &[Block]) -> Measured {
    Measured {
        target: Target {
            id: id.to_owned(),
            package: "core".to_owned(),
            unit: UnitKind::Lib,
            unit_name: "core".to_owned(),
            path: format!("tests::{id}"),
            ignored: false,
            executable: PathBuf::from("/nowhere"),
            cwd: PathBuf::from("/nowhere"),
            env: Vec::new(),
        },
        status,
        duration_ms,
        message: None,
        covered: covered.iter().cloned().collect(),
        restored: false,
    }
}

fn instrumented(blocks: &[Block]) -> BTreeSet<Block> {
    blocks.iter().cloned().collect()
}

#[test]
fn only_the_tests_whose_coverage_contains_the_position_are_run() {
    let inside = measured(
        "inside",
        10,
        TargetStatus::Passed,
        &[block("src/lib.rs", (10, 1), (20, 1))],
    );
    let elsewhere = measured(
        "elsewhere",
        10,
        TargetStatus::Passed,
        &[block("src/lib.rs", (30, 1), (40, 1))],
    );
    let other_file = measured(
        "other-file",
        10,
        TargetStatus::Passed,
        &[block("src/other.rs", (10, 1), (20, 1))],
    );
    let all = instrumented(&[
        block("src/lib.rs", (10, 1), (20, 1)),
        block("src/lib.rs", (30, 1), (40, 1)),
    ]);

    let routed = route(
        "src/lib.rs",
        Some(at(12, 5)),
        &[inside, elsewhere, other_file],
        &all,
    );
    assert!(matches!(routed, Route::Block { .. }), "{routed:?}");
    assert_eq!(routed.reaching(), ["inside"]);
    assert_eq!(
        routed.file_candidates(),
        2,
        "two targets touched the file; one of them reaches the position"
    );
    assert_eq!(
        routed.fallback(),
        None,
        "a block route has no fallback reason to carry, and the type says so"
    );
}

#[test]
fn the_cheapest_test_that_could_find_a_kill_goes_first() {
    let covering = block("src/lib.rs", (10, 1), (20, 1));
    let one = std::slice::from_ref(&covering);
    let targets = [
        measured("slow", 900, TargetStatus::Passed, one),
        measured("quick", 5, TargetStatus::Passed, one),
        measured("middling", 60, TargetStatus::Passed, one),
    ];
    let routed = route("src/lib.rs", Some(at(12, 5)), &targets, &instrumented(one));
    assert_eq!(
        routed.reaching(),
        ["quick", "middling", "slow"],
        "a kill ends the search, so the order is the whole optimisation"
    );
}

#[test]
fn a_position_the_measurement_describes_that_nothing_ran_reaches_nobody() {
    let target = measured(
        "one",
        10,
        TargetStatus::Passed,
        &[block("src/lib.rs", (10, 1), (20, 1))],
    );
    let all = instrumented(&[
        block("src/lib.rs", (10, 1), (20, 1)),
        block("src/lib.rs", (30, 1), (40, 1)),
    ]);

    let routed = route("src/lib.rs", Some(at(32, 1)), &[target], &all);
    assert_eq!(
        routed,
        Route::Unreached { file_candidates: 1 },
        "the mutation lives in code the measured tests never execute, and that is \
         the answer rather than a fallback"
    );
}

#[test]
fn a_mutant_whose_position_is_unknown_is_routed_by_file() {
    let covering = block("src/lib.rs", (10, 1), (20, 1));
    let targets = [
        measured(
            "a",
            10,
            TargetStatus::Passed,
            std::slice::from_ref(&covering),
        ),
        measured(
            "b",
            20,
            TargetStatus::Passed,
            &[block("src/lib.rs", (30, 1), (40, 1))],
        ),
    ];
    let routed = route(
        "src/lib.rs",
        None,
        &targets,
        &instrumented(std::slice::from_ref(&covering)),
    );
    assert_eq!(routed.granularity(), "file");
    assert_eq!(routed.fallback(), Some(Fallback::PositionUnknown));
    assert_eq!(
        routed.reaching(),
        ["a", "b"],
        "not knowing where something is is not evidence that nothing runs it"
    );
}

#[test]
fn a_position_outside_every_instrumented_region_is_routed_by_file() {
    let covering = block("src/lib.rs", (10, 1), (20, 1));
    let one = std::slice::from_ref(&covering);
    let targets = [measured("a", 10, TargetStatus::Passed, one)];

    let routed = route("src/lib.rs", Some(at(25, 1)), &targets, &instrumented(one));
    assert_eq!(routed.granularity(), "file");
    assert_eq!(routed.fallback(), Some(Fallback::OutsideBlocks));
    assert_eq!(
        routed.reaching(),
        ["a"],
        "a gap between the regions llvm-cov cut is a gap in the measurement, not \
         proof that the code never runs"
    );
}

#[test]
fn a_test_that_did_not_pass_is_no_evidence_of_reaching_anything() {
    let covering = block("src/lib.rs", (10, 1), (20, 1));
    let one = std::slice::from_ref(&covering);
    let targets = [
        measured("failed", 10, TargetStatus::Failed, one),
        measured("skipped", 10, TargetStatus::Skipped, one),
        measured("missing", 10, TargetStatus::Missing, one),
    ];
    let routed = route("src/lib.rs", Some(at(12, 1)), &targets, &instrumented(one));
    assert_eq!(
        routed.granularity(),
        "unreached",
        "a target that did not pass cannot later be said to have noticed a change"
    );
}

#[test]
fn a_region_that_ends_where_the_position_is_does_not_contain_it() {
    let covering = block("src/lib.rs", (10, 1), (20, 1));
    let targets = [measured(
        "a",
        10,
        TargetStatus::Passed,
        std::slice::from_ref(&covering),
    )];
    let routed = route(
        "src/lib.rs",
        Some(at(20, 1)),
        &targets,
        &instrumented(&[covering, block("src/lib.rs", (20, 1), (30, 1))]),
    );
    assert_eq!(
        routed.granularity(),
        "unreached",
        "the end is exclusive, which is what llvm-cov means by it"
    );
}

#[test]
fn a_route_cannot_say_it_reached_nothing_and_name_targets_anyway() {
    assert!(mjutest_cli::assure::route::Reaching::new(Vec::new()).is_none());
    let one = mjutest_cli::assure::route::Reaching::new(vec!["a".to_owned()])
        .expect("one target is one route");
    assert_eq!(one.as_slice(), ["a"]);
}

/// The body a narrowed condition gates.
const fn body(from: (u32, u32), to: (u32, u32)) -> Body {
    Body {
        start: at(from.0, from.1),
        end: at(to.0, to.1),
    }
}

#[test]
fn a_test_that_never_took_the_branch_is_removed_without_being_run() {
    let gated = block("src/lib.rs", (10, 20), (12, 6));
    let took = measured(
        "took",
        1,
        TargetStatus::Passed,
        &[block("src/lib.rs", (10, 8), (10, 18)), gated.clone()],
    );
    let did_not = measured(
        "did-not",
        1,
        TargetStatus::Passed,
        &[block("src/lib.rs", (10, 8), (10, 18))],
    );
    let baseline = [took, did_not];
    let seen = instrumented(&[block("src/lib.rs", (10, 8), (10, 18)), gated]);

    let route = route("src/lib.rs", Some(at(10, 12)), &baseline, &seen);
    assert_eq!(route.reaching().len(), 2, "both touched the position");

    let narrowed = discharge(
        route,
        &Proven {
            path: "src/lib.rs",
            body: body((10, 20), (12, 6)),
            baseline: &baseline,
            instrumented: &seen,
        },
    );
    assert_eq!(narrowed.reaching(), ["took"]);
    assert_eq!(narrowed.discharged(), ["did-not"]);
    assert_eq!(narrowed.granularity(), "block");
}

#[test]
fn a_mutant_every_test_was_discharged_for_is_resolved_without_one_execution() {
    let gated = block("src/lib.rs", (10, 20), (12, 6));
    let never = measured(
        "never",
        1,
        TargetStatus::Passed,
        &[block("src/lib.rs", (10, 8), (10, 18))],
    );
    let baseline = [never];
    let seen = instrumented(&[block("src/lib.rs", (10, 8), (10, 18)), gated]);

    let narrowed = discharge(
        route("src/lib.rs", Some(at(10, 12)), &baseline, &seen),
        &Proven {
            path: "src/lib.rs",
            body: body((10, 20), (12, 6)),
            baseline: &baseline,
            instrumented: &seen,
        },
    );
    assert_eq!(narrowed.granularity(), "discharged");
    assert!(narrowed.reaching().is_empty());
    assert_eq!(narrowed.discharged(), ["never"]);
}

#[test]
fn a_body_nothing_instrumented_discharges_nobody() {
    let touched = measured(
        "touched",
        1,
        TargetStatus::Passed,
        &[block("src/lib.rs", (10, 8), (10, 18))],
    );
    let baseline = [touched];
    let seen = instrumented(&[block("src/lib.rs", (10, 8), (10, 18))]);

    let narrowed = discharge(
        route("src/lib.rs", Some(at(10, 12)), &baseline, &seen),
        &Proven {
            path: "src/lib.rs",
            body: body((10, 20), (12, 6)),
            baseline: &baseline,
            instrumented: &seen,
        },
    );
    assert_eq!(
        narrowed.reaching(),
        ["touched"],
        "no target's silence about a body nothing measured means anything"
    );
    assert!(narrowed.discharged().is_empty());
}

#[test]
fn a_route_decided_by_file_is_never_narrowed_by_a_proof() {
    let touched = measured(
        "touched",
        1,
        TargetStatus::Passed,
        &[block("src/lib.rs", (10, 8), (10, 18))],
    );
    let baseline = [touched];
    let seen = instrumented(&[block("src/lib.rs", (10, 8), (10, 18))]);
    let by_file = route("src/lib.rs", None, &baseline, &seen);
    assert_eq!(by_file.granularity(), "file");

    let narrowed = discharge(
        by_file,
        &Proven {
            path: "src/lib.rs",
            body: body((10, 20), (12, 6)),
            baseline: &baseline,
            instrumented: &seen,
        },
    );
    assert_eq!(narrowed.granularity(), "file");
    assert_eq!(narrowed.reaching(), ["touched"]);
}

#[test]
fn a_target_restored_from_a_checkpoint_carries_no_regions_to_argue_with() {
    let gated = block("src/lib.rs", (10, 20), (12, 6));
    let mut restored = measured(
        "restored",
        1,
        TargetStatus::Passed,
        &[block("src/lib.rs", (0, 0), (u32::MAX, u32::MAX))],
    );
    restored.restored = true;
    let baseline = [restored];
    let seen = instrumented(&[block("src/lib.rs", (10, 8), (10, 18)), gated]);

    let narrowed = discharge(
        route("src/lib.rs", Some(at(10, 12)), &baseline, &seen),
        &Proven {
            path: "src/lib.rs",
            body: body((10, 20), (12, 6)),
            baseline: &baseline,
            instrumented: &seen,
        },
    );
    assert_eq!(narrowed.reaching(), ["restored"]);
    assert!(narrowed.discharged().is_empty());
}
