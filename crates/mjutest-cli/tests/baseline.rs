// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The baseline: every test in its own process, under instrumentation, and
//! what each one reached.
//!
//! Two things here are load-bearing. Per-target coverage must actually
//! differ between targets, or routing a mutant to the tests that reach it is
//! no better than running them all. And a target that did not run must never
//! be recorded as one that passed: libtest exits 0 for a filter that matched
//! nothing, so the summary line is what decides.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;
use std::ffi::OsString;

use mjutest_cli::assure::baseline::{Baseline, BaselineOptions, Workspace, run, status_of};
use mjutest_cli::build::{Cargo, Selection};
use mjutest_cli::coverage::Block;
use mjutest_cli::report::TargetStatus;
use mjutest_cli::trace::Recorder;
use mjutest_cli::watch::Watch;
use rust_mutants::cargo::{Driver, LocateOptions, Metadata, MetadataOptions, Toolchain};
use rust_mutants::execute::Summary;
use rust_mutants::runner::Cancel;

fn env() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        })
        .collect()
}

fn measure(fixture: &str) -> (Baseline, tempfile::TempDir) {
    let root = mjutest_devkit::paths::fixtures_dir().join(fixture);
    let scratch = tempfile::Builder::new()
        .prefix("mjutest-baseline-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let engine_trace = rust_mutants::trace::Recorder::disabled();
    let trace = Recorder::disabled();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            search_path: None,
            env: Some(env()),
        },
        &root,
        &cancel,
    )
    .expect("a toolchain");
    let metadata = Metadata::load(
        &Driver {
            toolchain: &toolchain,
            dir: &root,
            cancel: &cancel,
            trace: &engine_trace,
        },
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    let profiles = scratch.path().join("profiles");
    std::fs::create_dir_all(&profiles).expect("somewhere for the profiles");

    let baseline = run(
        Workspace {
            toolchain: &toolchain,
            packages: &metadata.packages,
        },
        &BaselineOptions {
            root,
            selection: Selection::default(),
            cargo: Cargo {
                offline: true,
                locked: true,
            },
            env: env(),
            target_dir: scratch.path().join("layer"),
            scratch_build_dir: scratch.path().join("build"),
            profiles_dir: profiles,
            timeout: None,
            test_args: Vec::new(),
        },
        &mut mjutest_cli::ui::Silent,
        Watch::new(&cancel, &trace),
    )
    .expect("the baseline runs");
    (baseline, scratch)
}

fn named(baseline: &Baseline, name: &str) -> usize {
    baseline
        .targets
        .iter()
        .position(|measured| measured.target.name().contains(name))
        .unwrap_or_else(|| {
            panic!(
                "no target named {name}: {:?}",
                baseline
                    .targets
                    .iter()
                    .map(|measured| measured.target.name())
                    .collect::<Vec<_>>()
            )
        })
}

// --- what a baseline observes -------------------------------------------------------

#[test]
fn every_test_is_a_target_of_its_own_and_says_what_became_of_it() {
    let (baseline, _scratch) = measure("fixture-baseline");
    assert!(baseline.failure.is_none(), "{:?}", baseline.failure);
    assert_eq!(baseline.targets.len(), 3, "the fixture's three tests");

    let mut states: Vec<(String, TargetStatus)> = baseline
        .targets
        .iter()
        .map(|measured| (measured.target.path.clone(), measured.status))
        .collect();
    states.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        states,
        [
            (
                "doubling_is_addition_twice".to_owned(),
                TargetStatus::Passed
            ),
            (
                "tests::sign_names_both_sides_of_zero".to_owned(),
                TargetStatus::Passed
            ),
            (
                "tests::zero_has_a_sign_of_its_own".to_owned(),
                TargetStatus::Skipped
            ),
        ],
        "an ignored test is skipped, never a pass nobody observed"
    );
}

#[test]
fn two_targets_reach_different_regions_which_is_what_routing_rests_on() {
    let (baseline, _scratch) = measure("fixture-baseline");
    let sign = &baseline.targets[named(&baseline, "sign_names_both_sides_of_zero")];
    let doubling = &baseline.targets[named(&baseline, "doubling_is_addition_twice")];

    assert!(!sign.covered.is_empty(), "the sign test reached something");
    assert!(!doubling.covered.is_empty(), "so did the doubling test");
    assert_ne!(
        sign.covered, doubling.covered,
        "if every target reached the same regions, routing would be worth nothing"
    );

    let both: BTreeSet<&Block> = sign.covered.intersection(&doubling.covered).collect();
    assert!(
        both.len() < sign.covered.len(),
        "and neither is a subset of the other: {both:?}"
    );
}

#[test]
fn what_a_target_reached_is_a_part_of_what_the_build_instrumented() {
    let (baseline, _scratch) = measure("fixture-baseline");
    assert!(!baseline.instrumented.is_empty());
    for measured in &baseline.targets {
        assert!(
            measured.covered.is_subset(&baseline.instrumented),
            "{}: reached a region the build never instrumented",
            measured.target.name()
        );
    }
}

#[test]
fn a_skipped_target_reached_nothing_because_it_never_ran() {
    let (baseline, _scratch) = measure("fixture-baseline");
    let ignored = &baseline.targets[named(&baseline, "zero_has_a_sign_of_its_own")];
    assert_eq!(ignored.status, TargetStatus::Skipped);
    assert!(
        ignored.covered.is_empty(),
        "a test libtest did not run reached nothing"
    );
}

// --- what the summary line decides ---------------------------------------------------

const fn summary(passed: u32, failed: u32, ignored: u32) -> Summary {
    Summary {
        ok: failed == 0,
        passed,
        failed,
        ignored,
        measured: 0,
        filtered_out: 0,
    }
}

#[test]
fn the_summary_line_decides_and_not_the_exit_code() {
    assert_eq!(
        status_of(Some(summary(1, 0, 0)), false).0,
        TargetStatus::Passed
    );
    assert_eq!(
        status_of(Some(summary(0, 1, 0)), false).0,
        TargetStatus::Failed
    );
    assert_eq!(
        status_of(Some(summary(0, 0, 1)), false).0,
        TargetStatus::Skipped
    );
    assert_eq!(
        status_of(Some(summary(0, 0, 0)), false).0,
        TargetStatus::Missing,
        "libtest exits 0 for a filter that matched nothing; a target that did not \
         run is not a target that passed"
    );
    assert_eq!(
        status_of(None, false).0,
        TargetStatus::Missing,
        "and no summary line at all is no observation at all"
    );
}

#[test]
fn a_target_that_ran_out_of_time_failed_and_says_so() {
    let (status, message) = status_of(Some(summary(1, 0, 0)), true);
    assert_eq!(status, TargetStatus::Failed);
    assert!(
        message.unwrap_or_default().contains("time"),
        "a timeout is a failure a reader can recognise"
    );
}

#[test]
fn a_target_that_failed_says_what_it_said() {
    let (status, message) = status_of(Some(summary(0, 2, 0)), false);
    assert_eq!(status, TargetStatus::Failed);
    assert!(message.unwrap_or_default().contains('2'), "how many failed");
}
