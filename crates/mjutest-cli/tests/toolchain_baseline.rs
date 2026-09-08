// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The baseline: what the one verified run of every target observed.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_cli::assure::baseline::{Baseline, Measured, Reporting, observe};
use mjutest_cli::report::TargetStatus;
use mjutest_cli::trace::Recorder;
use mjutest_cli::watch::Watch;
use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Failing, PrepareOptions, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepared(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            offline: true,
            locked: true,
            ..opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            failing: Failing::Exclude,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

fn measure(fixture: &Fixture) -> Baseline {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    observe(
        &prepared(fixture),
        Reporting {
            notes: &mut mjutest_cli::ui::Notes::Silent,
            watch: Watch::new(&cancel, &trace),
        },
    )
}

fn named<'a>(baseline: &'a Baseline, name: &str) -> &'a Measured {
    baseline
        .targets
        .iter()
        .find(|measured| measured.target.name() == name)
        .unwrap_or_else(|| {
            panic!(
                "no target called {name}; there are {:?}",
                baseline
                    .targets
                    .iter()
                    .map(|one| one.target.name())
                    .collect::<Vec<String>>()
            )
        })
}

#[test]
fn a_target_is_a_binary_and_its_row_says_how_many_tests_it_ran() {
    let fixture = Fixture::copy("fixture-simple");
    let baseline = measure(&fixture);

    for measured in &baseline.targets {
        assert!(
            measured.target.is_whole_binary(),
            "{} is a row about a binary, not about one of its tests",
            measured.target.name()
        );
    }
    let mut names: Vec<String> = baseline
        .targets
        .iter()
        .map(|one| one.target.name())
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(
        names.len(),
        baseline.targets.len(),
        "one row per binary, each named once"
    );

    let lib = named(&baseline, "fixture-simple/lib/fixture_simple");
    assert_eq!(lib.status, TargetStatus::Passed);
    assert!(
        lib.tests > 0,
        "the row carries how many tests the run of it executed, which is what its \
         duration is the cost of"
    );
}

#[test]
fn a_target_whose_own_tests_fail_is_a_row_and_a_finding_rather_than_a_refusal() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let baseline = measure(&fixture);

    let failed: Vec<&Measured> = baseline
        .targets
        .iter()
        .filter(|one| one.status == TargetStatus::Failed)
        .collect();
    assert!(
        failed.len() >= 2,
        "the table names every target that failed rather than the first: {:?}",
        baseline
            .targets
            .iter()
            .map(|one| (one.target.name(), one.status))
            .collect::<Vec<(String, TargetStatus)>>()
    );
    for measured in failed {
        assert!(
            measured
                .message
                .as_ref()
                .is_some_and(|said| !said.is_empty()),
            "{} failed and the row says what it said",
            measured.target.name()
        );
    }
    assert!(
        baseline
            .limitations
            .iter()
            .any(|name| name.starts_with(mjutest_cli::assure::baseline::NOT_PASSING_LIMITATION)),
        "and the run states why those targets answer nothing: {:?}",
        baseline.limitations
    );
}
