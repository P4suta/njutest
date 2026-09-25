// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether removing the work changed the answer.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::collections::BTreeMap;

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::report::run::{RunDocument, RunMutantDocument};
use rust_mutants::run::NotRunReason;
use rust_mutants::runner::Cancel;
use rust_mutants::testkit::measuring::Measuring;
use rust_mutants::work::Work;
use rust_mutants_cli::{Environment, Streams};

/// The fixtures the layers have something to say about.
const FIXTURES: [&str; 7] = [
    "fixture-simple",
    "fixture-coverage",
    "fixture-unreached",
    "fixture-probeable",
    "fixture-order-dependent",
    "fixture-threaded",
    "fixture-item-reach",
];

/// What a run established about one tree, what it cost to establish it, and what its guards recorded.
struct Established {
    rows: BTreeMap<String, RunMutantDocument>,
    work: Work,
    touched: rust_mutants::touch::Touched,
}

fn established(name: &str, extra: &[&str]) -> Established {
    let fixture = Fixture::copy(name);
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run", "--tier", "all", "--offline", "--locked"])
            .chain(["--jobs", "1", "--ui", "quiet", "--no-cache"])
            .chain(extra.iter().copied())
            .chain(["--root", root.as_str()])
            .map(OsString::from),
        &environment(&fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    assert!(
        output.status.code().is_some_and(|code| code <= 1),
        "{name} {extra:?}: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let directory = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    );
    let text = std::fs::read_to_string(directory.join("run-report-v1.json")).expect("the report");
    let document: RunDocument =
        njutest_devkit::strictjson::decode_str(&text).expect("the report reads back");
    let touched = std::fs::read_to_string(directory.join("touched-v1.json")).expect("the record");
    Established {
        touched: njutest_devkit::strictjson::decode_str(&touched).expect("the record reads back"),
        work: Work::of(&document).expect("valid work ledger"),
        rows: document
            .mutants
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect(),
    }
}

/// What a run that removed nothing would have said about a mutant a proof removed.
const fn claimed(row: &RunMutantDocument) -> Outcome {
    match (row.outcome, row.not_run_reason) {
        (Outcome::NotRun, Some(NotRunReason::Unreached | NotRunReason::Discharged)) => {
            Outcome::Survived
        }
        (outcome, _) => outcome,
    }
}

/// Every site a test reached and every mutation a test noticed outside an item that test entered, and how many were held to one.
fn unentered(established: &Established) -> (Vec<String>, u32) {
    let mut breaches = Vec::new();
    let mut held: u32 = 0;
    let record = &established.touched;
    for row in established.rows.values() {
        let span = rust_mutants::span::Span {
            start: row.start_byte,
            end: row.end_byte,
        };
        let Some(item) = record.item_holding(&row.path, span) else {
            if !record.items.is_empty() {
                breaches.push(format!("{} sits in no item of the catalog", row.display_id));
            }
            continue;
        };
        let mut owed: Vec<(&str, String)> = Vec::new();
        for (target, touches) in &record.targets {
            for test in touches.reached.who(row.index) {
                owed.push((target.as_str(), test));
            }
        }
        if row.outcome == Outcome::Killed {
            owed.extend(
                row.killed_by
                    .iter()
                    .map(|test| (row.target.as_str(), test.clone())),
            );
        }
        for (target, test) in owed {
            let Some(touches) = record.targets.get(target) else {
                continue;
            };
            held = held.saturating_add(1);
            if !touches.entered_by(&test).contains(&item.index) {
                breaches.push(format!(
                    "{test} of {target} reached or noticed {} and never entered {}",
                    row.display_id, item.name
                ));
            }
        }
    }
    (breaches, held)
}

#[test]
fn every_proof_that_removed_a_run_claimed_the_answer_a_whole_run_gives() {
    let mut removed_something: u32 = 0;
    let mut claims: u32 = 0;
    let mut entries: u32 = 0;
    for name in FIXTURES {
        let whole = established(name, Measuring::NOTHING.flags());
        for measuring in Measuring::ALL {
            if measuring == Measuring::NOTHING {
                continue;
            }
            let mode = measuring.name();
            let proved = established(name, measuring.flags());
            let (breaches, held) = unentered(&proved);
            assert!(
                breaches.is_empty(),
                "{name} by {mode}: a test that reached a site or noticed a mutation entered the \
                 item it sits in, or a change there could be routed away from it: {breaches:#?}"
            );
            entries = entries.saturating_add(held);
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
                if row.outcome == Outcome::Inconclusive || other.outcome == Outcome::Inconclusive {
                    continue;
                }
                if row.outcome == Outcome::NotRun && claimed(row) != row.outcome {
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
    assert!(
        entries > 0,
        "no reach and no kill was held to an entered item, so the check above saw nothing"
    );
}

/// What a row says happened to it, as a sentence a failing assertion can carry.
fn describe(row: &RunMutantDocument) -> String {
    match (row.outcome, row.not_run_reason) {
        (Outcome::NotRun, Some(reason)) => format!("not run ({})", reason.as_str()),
        (outcome, _) => outcome.to_string(),
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
        if row.outcome == Outcome::Inconclusive || other.outcome == Outcome::Inconclusive {
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
        let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = rust_mutants_cli::run_from(
            std::iter::once("rust-mutants")
                .chain(["run", "--tier", "all", "--offline", "--locked"])
                .chain(["--jobs", "1", "--ui", "quiet"])
                .chain(extra.iter().copied())
                .chain(["--root", root.as_str()])
                .map(OsString::from),
            &environment(fixture),
            &Cancel::new(),
            Streams {
                out: &mut out,
                err: &mut err,
            },
        );
        let output = njutest_devkit::process::answered(code, out, err);
        assert!(
            output.status.code().is_some_and(|code| code <= 1),
            "{}",
            njutest_devkit::process::strict_utf8(&output.stderr)
        );
        let directory = njutest_devkit::fixture::newest_run(
            &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
        );
        let text =
            std::fs::read_to_string(directory.join("run-report-v1.json")).expect("the report");
        let document: RunDocument =
            njutest_devkit::strictjson::decode_str(&text).expect("the report reads back");
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
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["cache", "--clear-outcomes"])
            .chain(["--root", root.as_str()])
            .map(OsString::from),
        &environment(&fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let cleared = njutest_devkit::process::answered(code, out, err);
    assert_eq!(cleared.status.code(), Some(0), "{cleared:?}");
    let remembered = report(&fixture, &[]);
    assert_eq!(
        fresh, remembered,
        "a run that read the measurement back instead of making it again has to route every \
         mutant exactly as the run that made it did"
    );
    assert!(!fresh.is_empty(), "there was something to compare");
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        ci: rust_mutants_cli::CiHost::None,
    }
}
