// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage routing: a mutant is only ever run against a target that reached it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::Path;

use njutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Request, Session};
use rust_mutants::testkit::measuring::Measuring;
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

fn prepared(fixture: &Fixture, coverage: bool) -> Session {
    measuring(
        fixture,
        if coverage {
            Measuring::BOTH
        } else {
            Measuring::GUARDS
        },
    )
}

/// A prepared session that measures by whichever of the two layers it is asked for.
fn measuring(fixture: &Fixture, measuring: Measuring) -> Session {
    let workspace = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open");
    workspace
        .prepare(&measuring.options(Tier::Balanced), &Cancel::new())
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
fn a_session_that_measured_neither_way_proves_nothing_about_reach() {
    let fixture = Fixture::copy("fixture-simple");
    let session = measuring(&fixture, Measuring::NOTHING);
    assert!(!session.reached().measured());
    assert!(!session.touched().measured());
    let one = mutant(&session, "gt-to-ge", 11);
    assert_eq!(session.reaches(one), None);
    session.close().expect("close");
}

#[test]
fn the_guards_prove_reach_with_no_coverage_build_at_all() {
    let fixture = Fixture::copy("fixture-simple");
    let session = measuring(&fixture, Measuring::GUARDS);
    assert!(
        !session.reached().measured(),
        "no coverage build was made and none is needed"
    );
    assert!(session.touched().measured());
    let one = mutant(&session, "gt-to-ge", 11);
    assert_eq!(
        session.reaches(one),
        Some(true),
        "the tests run this line, and the guards on it said so"
    );
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

#[test]
fn coverage_is_refused_only_for_target_specific_rustflags() {
    let fixture = Fixture::copy("fixture-simple");
    fixture.write(
        ".cargo/config.toml",
        b"[build]\nrustflags = [\"--cfg\", \"measured_here\"]\n",
    );
    let session = prepared(&fixture, true);
    assert!(
        session.reached().measured(),
        "flags a run can read are flags a coverage build can put back: {:?}",
        session.reached().limitations
    );
    session.close().expect("close");

    let refused = Fixture::copy("fixture-simple");
    refused.write(
        ".cargo/config.toml",
        b"[target.x86_64-unknown-linux-gnu]\nrustflags = [\"--cfg\", \"per_target\"]\n",
    );
    let session = prepared(&refused, true);
    assert_eq!(
        session.reached().limitations,
        vec![rust_mutants::reach::CONFIGURED_FLAGS.to_owned()],
        "which of cargo's target tables apply is cargo's decision, and a guess compiles \
         something other than the project's own binaries"
    );
    session.close().expect("close");
}

#[test]
fn a_configuration_nobody_can_parse_is_cargos_own_refusal_and_names_the_file() {
    let fixture = Fixture::copy("fixture-simple");
    fixture.write(".cargo/config.toml", b"[build\nrustflags = ]\n");
    let opened = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    );
    let error = opened.expect_err("a refusal");
    assert_eq!(error.code().code, "RM1014");
    assert!(
        error.to_string().contains("config.toml"),
        "the tree cargo refuses is named by the file it refused over: {error}"
    );
}
