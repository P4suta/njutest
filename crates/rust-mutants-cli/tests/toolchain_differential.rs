// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether removing the work changed the answer.
//!
//! Every proof layer exists to not run something. The claim each one makes is
//! not "this was probably fine to skip" but "running it would have established
//! exactly this", and a claim of that shape is one a test can call. So: run a
//! fixture twice, once with every layer on and once with every layer off, and
//! hold the two reports to each other mutant by mutant.
//!
//! A mutant the proved run never started a process for, because a measurement
//! or a proof said no target could notice it, has to be one the whole run
//! found nothing noticed either. If a discharged mutant turns out to be killed
//! when something actually runs it, the proof is wrong, and this is where that
//! is found out rather than in somebody's report.
//!
//! There are two measurements now and they are checked separately, each against
//! a run with nothing removed: the guards, which record on the baseline run
//! which of a target's tests reached each mutation, and the LLVM coverage
//! build, which is kept as an independent second opinion. Two layers that
//! agree with a whole run agree with each other, and one that does not is
//! named here.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::collections::BTreeMap;
use std::process::Command;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::report::run::{RunDocument, RunMutantDocument};
use rust_mutants::testkit::measuring::Measuring;
use rust_mutants::work::Work;

/// The fixtures the layers have something to say about.
///
/// The last two are the fallbacks: a target whose tests only pass beside each
/// other, and one whose tests reach the code on threads of their own. Both are
/// cases where the guards cannot narrow, and a fallback that got the answer
/// wrong would show up here as a row that moved.
const FIXTURES: [&str; 6] = [
    "fixture-simple",
    "fixture-coverage",
    "fixture-unreached",
    "fixture-probeable",
    "fixture-order-dependent",
    "fixture-threaded",
];

/// What a run established about one tree, and what it cost to establish it.
struct Established {
    rows: BTreeMap<String, RunMutantDocument>,
    work: Work,
}

fn established(name: &str, extra: &[&str]) -> Established {
    let fixture = Fixture::copy(name);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .args(["run", "--tier", "all", "--offline", "--locked"])
        .args(["--jobs", "1", "--ui", "quiet", "--no-cache"])
        .args(extra)
        .args(["--root", &fixture.root().to_string_lossy()])
        .output()
        .expect("rust-mutants runs");
    assert!(
        output.status.code().is_some_and(|code| code <= 1),
        "{name} {extra:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = mjutest_devkit::fixture::newest_run(fixture.root());
    let text = std::fs::read_to_string(directory.join("run-report-v1.json")).expect("the report");
    let document: RunDocument = serde_json::from_str(&text).expect("the report reads back");
    Established {
        work: Work::of(&document),
        rows: document
            .mutants
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect(),
    }
}

/// What a run that removed nothing would have said about a mutant a proof removed.
///
/// A proof removes a (mutant, target) pair by claiming the target could not
/// have noticed. A mutant every target was removed from is therefore one no
/// test notices: a survivor. That is the whole claim, and it is falsifiable.
fn claimed(row: &RunMutantDocument) -> &str {
    match (row.outcome.as_str(), row.not_run_reason.as_deref()) {
        ("not_run", Some("unreached" | "discharged")) => "survived",
        (outcome, _) => outcome,
    }
}

#[test]
fn every_proof_that_removed_a_run_claimed_the_answer_a_whole_run_gives() {
    let mut removed_something: u32 = 0;
    let mut claims: u32 = 0;
    for name in FIXTURES {
        let whole = established(name, Measuring::NOTHING.flags());
        for measuring in Measuring::ALL {
            if measuring == Measuring::NOTHING {
                continue;
            }
            let mode = measuring.name();
            let proved = established(name, measuring.flags());
            assert_eq!(
                proved.rows.len(),
                whole.rows.len(),
                "{name} by {mode}: the two runs cataloged different trees, so nothing compares"
            );
            if proved.work.tests_started < whole.work.tests_started {
                removed_something = removed_something.saturating_add(1);
            }
            for (id, row) in &proved.rows {
                let Some(other) = whole.rows.get(id) else {
                    panic!("{name} by {mode}: {id} is in the proved run and not in the whole one");
                };
                if row.outcome == "inconclusive" || other.outcome == "inconclusive" {
                    continue;
                }
                if row.outcome == "not_run" && claimed(row) != row.outcome.as_str() {
                    claims = claims.saturating_add(1);
                }
                assert_eq!(
                    claimed(row),
                    claimed(other),
                    "{name} by {mode}: {} was {} with the layer on and {} with nothing removed. A \
                     proof that removes work has to leave the answer where a whole run leaves it; \
                     this one moved it.",
                    row.display_id,
                    describe(row),
                    describe(other),
                );
            }
        }
    }
    assert!(
        removed_something > 0,
        "no fixture cost less with the layers on than with them off, so this test proved nothing \
         about them"
    );
    assert!(
        claims > 0,
        "no mutant was removed by a proof at all, so every comparison above was between two \
         measurements and none of them was a claim being checked"
    );
}

/// What a row says happened to it, as a sentence a failing assertion can carry.
fn describe(row: &RunMutantDocument) -> String {
    match (row.outcome.as_str(), row.not_run_reason.as_deref()) {
        ("not_run", Some(reason)) => format!("not run ({reason})"),
        (outcome, _) => outcome.to_owned(),
    }
}

#[test]
fn the_layers_remove_work_rather_than_only_promising_to() {
    let whole = established("fixture-unreached", Measuring::NOTHING.flags());
    for measuring in Measuring::ALL {
        if measuring == Measuring::NOTHING {
            continue;
        }
        let mode = measuring.name();
        let proved = established("fixture-unreached", measuring.flags());
        assert!(
            proved.work.started < whole.work.started,
            "by {mode}: a fixture built to hold code no test reaches should start fewer \
             processes measured than unmeasured: {} against {}",
            proved.work.started,
            whole.work.started
        );
        assert!(
            proved.work.answers_for_the_whole(),
            "by {mode}: the run was asked for less than the whole catalog"
        );
    }
    assert!(
        whole.work.answers_for_the_whole(),
        "the run with nothing removed was asked for less than the whole catalog"
    );
}

#[test]
fn the_guards_put_a_mutation_to_fewer_tests_than_routing_by_target_can() {
    let whole = established("fixture-coverage", Measuring::NOTHING.flags());
    let guards = established("fixture-coverage", Measuring::GUARDS.flags());
    let coverage = established("fixture-coverage", Measuring::COVERAGE.flags());
    assert!(
        guards.work.tests_started < coverage.work.tests_started,
        "a target a region places a mutation in runs every test it has; a target a guard places \
         it in runs the tests that reached it: {} against {}",
        guards.work.tests_started,
        coverage.work.tests_started
    );
    assert!(
        coverage.work.tests_started <= whole.work.tests_started,
        "a region still removes whole targets: {} against {}",
        coverage.work.tests_started,
        whole.work.tests_started
    );
    assert!(
        guards.work.tests_started >= guards.work.established_tests(),
        "every test started to establish a filter is in the count the filters are judged by"
    );
    for (id, row) in &guards.rows {
        let other = whole.rows.get(id).expect("the same catalog");
        if row.outcome == "inconclusive" || other.outcome == "inconclusive" {
            continue;
        }
        assert_eq!(
            claimed(row),
            claimed(other),
            "{}: {} by the guards and {} with nothing removed",
            row.display_id,
            describe(row),
            describe(other)
        );
    }
}

#[test]
fn a_remembered_measurement_routes_a_run_exactly_as_a_fresh_one_would() {
    let fixture = Fixture::copy("fixture-coverage");
    let report = |fixture: &Fixture, extra: &[&str]| -> BTreeMap<String, String> {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
            .env("NO_COLOR", "1")
            .env("TMPDIR", fixture.temp())
            .env("XDG_CACHE_HOME", fixture.cache())
            .args(["run", "--tier", "all", "--offline", "--locked"])
            .args(["--jobs", "1", "--ui", "quiet"])
            .args(extra)
            .args(["--root", &fixture.root().to_string_lossy()])
            .output()
            .expect("rust-mutants runs");
        assert!(
            output.status.code().is_some_and(|code| code <= 1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let directory = mjutest_devkit::fixture::newest_run(fixture.root());
        let text =
            std::fs::read_to_string(directory.join("run-report-v1.json")).expect("the report");
        let document: RunDocument = serde_json::from_str(&text).expect("the report reads back");
        document
            .mutants
            .into_iter()
            .map(|row| {
                let route = row
                    .route
                    .map(|one| one.reaching.join(","))
                    .unwrap_or_default();
                (row.id, format!("{} {route}", row.outcome))
            })
            .collect()
    };
    let fresh = report(&fixture, &[]);
    let cleared = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .args(["cache", "--clear-outcomes"])
        .args(["--root", &fixture.root().to_string_lossy()])
        .output()
        .expect("rust-mutants runs");
    assert_eq!(cleared.status.code(), Some(0), "{cleared:?}");
    let remembered = report(&fixture, &[]);
    assert_eq!(
        fresh, remembered,
        "a run that read the measurement back instead of making it again has to route every \
         mutant exactly as the run that made it did"
    );
    assert!(!fresh.is_empty(), "there was something to compare");
}
