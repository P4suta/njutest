// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guards as the measurement: what a prepared session knows about which test reached which mutant, and how it knows it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

const LIBRARY: &str = "fixture-coverage/lib/fixture_coverage";

fn prepared(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
            env: std::env::vars_os().collect(),
            locked: true,
            offline: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            tier: Tier::All,
            coverage: false,
            branch_proofs: false,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

/// The one mutant of `rule` on `line`.
fn mutant(session: &Session, rule: &str, line: u32) -> u32 {
    session
        .catalog()
        .mutants()
        .iter()
        .find(|one| {
            one.candidate.rule.name == rule
                && session.position(one).is_some_and(|at| at.line == line)
        })
        .unwrap_or_else(|| panic!("a {rule} mutant on line {line}"))
        .index
}

/// The tests of `target` the measurement says reached `index`, in name order.
fn reaching(session: &Session, target: &str, index: u32) -> Vec<String> {
    let touches = session
        .touched()
        .targets
        .get(target)
        .unwrap_or_else(|| panic!("{target} was measured"));
    touches
        .tests
        .iter()
        .filter(|(_, sites)| sites.contains(&index))
        .map(|(name, _)| name.clone())
        .collect()
}

#[test]
fn the_verify_run_measures_which_test_reached_each_mutant_without_a_coverage_build() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    let touches = session
        .touched()
        .targets
        .get(LIBRARY)
        .unwrap_or_else(|| panic!("{LIBRARY} was measured"));
    let mut ran = touches.ran.clone();
    ran.sort();
    assert_eq!(
        ran,
        [
            "tests::a_short_list_is_short",
            "tests::a_version_is_earlier_than_a_later_one",
            "tests::clamp_returns_the_smaller",
        ],
        "every test the baseline ran is what a site nothing was attributed to reaches"
    );
    assert!(
        touches.loose.is_empty(),
        "every guard of this fixture is reached on the test's own thread: {:?}",
        touches.loose
    );
}

#[test]
fn a_mutant_is_reached_by_the_tests_that_exercise_its_function_and_by_no_other() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    assert_eq!(
        reaching(&session, LIBRARY, mutant(&session, "le-to-lt", 8)),
        ["tests::clamp_returns_the_smaller"],
        "one test of three calls clamp"
    );
    assert_eq!(
        reaching(&session, LIBRARY, mutant(&session, "le-to-lt", 16)),
        ["tests::a_version_is_earlier_than_a_later_one"]
    );
    assert_eq!(
        reaching(&session, LIBRARY, mutant(&session, "le-to-lt", 28)),
        ["tests::a_short_list_is_short"]
    );
}

#[test]
fn a_target_the_run_could_not_record_is_named_as_unmeasured_rather_than_read_as_empty() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    let touched = session.touched();
    for target in session.targets() {
        let id = target.id.as_str();
        assert!(
            touched.targets.contains_key(id)
                || touched
                    .limitations
                    .iter()
                    .any(|limitation| limitation.ends_with(id)),
            "{id} is neither measured nor accounted for: {touched:?}"
        );
    }
}
