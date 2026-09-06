// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage routing: a mutant is only ever run against a target that reached it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::Path;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepared(fixture: &Fixture, coverage: bool) -> Session {
    let workspace = Workspace::open(
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
    .expect("open");
    workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::Balanced,
                coverage,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare")
}

/// The mutant whose original text is `original` and whose rule is `rule`.
fn mutant<'a>(session: &'a Session, rule: &str, line: u32) -> &'a rust_mutants::catalog::Mutant {
    session
        .catalog()
        .mutants()
        .iter()
        .find(|one| {
            one.candidate.rule.name == rule
                && session.position(one).is_some_and(|at| at.line == line)
        })
        .unwrap_or_else(|| panic!("a {rule} mutant on line {line}"))
}

/// The one mutant of a rule the tree proposes exactly once.
fn only<'a>(session: &'a Session, rule: &str) -> &'a rust_mutants::catalog::Mutant {
    let mut found = session
        .catalog()
        .mutants()
        .iter()
        .filter(|one| one.candidate.rule.name == rule);
    let one = found.next().unwrap_or_else(|| panic!("a {rule} mutant"));
    assert!(found.next().is_none(), "more than one {rule} mutant");
    one
}

#[test]
fn a_session_that_was_not_asked_to_measure_coverage_proves_nothing_about_reach() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepared(&fixture, false);
    assert!(!session.reached().measured());
    let one = mutant(&session, "gt-to-ge", 11);
    assert_eq!(session.reaches(one), None);
    session.close().expect("close");
}

#[test]
fn each_target_reaches_the_code_its_own_tests_run_and_no_more() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepared(&fixture, true);
    let reached = session.reached();
    assert!(
        reached.measured(),
        "the coverage build was not measured: {:?}",
        reached.limitations
    );
    let measured: Vec<&str> = reached.targets.keys().map(String::as_str).collect();
    assert_eq!(
        measured,
        [
            "fixture-simple/lib/fixture_simple",
            "fixture-simple/test/parity"
        ]
    );

    let max = mutant(&session, "gt-to-ge", 11);
    let even = mutant(&session, "eq-to-neq", 16);
    assert_eq!(session.reaches(max), Some(true));
    assert_eq!(session.reaches(even), Some(true));
    let covering = |one: &rust_mutants::catalog::Mutant| -> Vec<String> {
        let position = session.position(one).expect("a position");
        reached
            .covering(
                Path::new(&one.candidate.path),
                rust_mutants::coverage::Point {
                    line: position.line,
                    column: position.byte_column,
                },
            )
            .expect("the measurement instrumented the place")
            .into_iter()
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(covering(max), ["fixture-simple/lib/fixture_simple"]);
    assert_eq!(covering(even), ["fixture-simple/test/parity"]);
    session.close().expect("close");
}

#[test]
fn a_mutant_no_measured_target_reached_is_not_run_at_all() {
    let fixture = Fixture::copy("fixture-simple");
    let mut source = std::fs::read_to_string(fixture.root().join("src/lib.rs")).expect("read");
    source.push_str("\n/// Nothing calls this.\npub fn unreached(a: i32) -> i32 {\n    a + 1\n}\n");
    std::fs::write(fixture.root().join("src/lib.rs"), source).expect("write");

    let session = prepared(&fixture, true);
    assert!(session.reached().measured());
    let alone = only(&session, "add-to-sub");
    assert_eq!(session.reaches(alone), Some(false));

    let result = session
        .exec(&Request::new(alone.id.clone()), &Cancel::new())
        .expect("exec");
    assert_eq!(
        result.outcome,
        Outcome::NotRun,
        "a mutant nothing reached is not run against everything to find that out again"
    );
    assert!(result.target.is_empty(), "{result:?}");
    session.close().expect("close");
}
