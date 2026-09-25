// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A control started under a perturbation runs under it, and one started without runs as the baseline did.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::execute::{Launcher, Schedule, Variable};
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{
    Conditions, Observing, Perturbation, PrepareOptions, Request, Session,
};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

fn prepare(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(&PrepareOptions::default(), &Cancel::new())
    .expect("prepare")
}

/// What a control of `target` came to under `perturbation`.
fn controlled(session: &Session, target: &str, perturbation: &Perturbation) -> Outcome {
    session
        .control_perturbed(
            &Request::new(String::new()).with_target(target),
            Conditions {
                observing: Observing::Nothing,
                perturbation,
            },
            &Cancel::new(),
        )
        .expect("the control runs")
        .result
        .outcome()
}

#[test]
fn a_control_runs_under_the_variables_the_launcher_and_the_arguments_it_is_given() {
    let fixture = Fixture::copy("fixture-environment");
    let session = prepare(&fixture);
    let none = Perturbation::none();
    let mut cases = vec![
        (
            "environment/test/timezone",
            Perturbation {
                environment: vec![(Variable::Tz, OsString::from("Australia/Lord_Howe"))],
                ..Perturbation::none()
            },
        ),
        (
            "environment/test/threads",
            Perturbation {
                schedule: Schedule::OneThread,
                ..Perturbation::none()
            },
        ),
    ];
    if cfg!(unix) {
        cases.push((
            "environment/test/umask",
            Perturbation {
                launcher: Some(Launcher::Umask { mask: 0o077 }),
                ..Perturbation::none()
            },
        ));
    }
    for (target, perturbation) in &cases {
        assert_eq!(
            controlled(&session, target, &none),
            Outcome::Survived,
            "{target} passes as the baseline ran it"
        );
        assert_ne!(
            controlled(&session, target, perturbation),
            Outcome::Survived,
            "{target} depends on exactly what {perturbation:?} changes, so a control that really \
             ran under it did not pass"
        );
    }
    session.close().expect("close");
}

#[test]
fn a_schedule_only_libtest_understands_is_refused_for_a_harness_that_is_not_libtest() {
    let fixture = Fixture::copy("fixture-custom-harness");
    let session = prepare(&fixture);
    let target = "fixture-custom-harness/test/by_exit_code";
    let one_thread = Perturbation {
        schedule: Schedule::OneThread,
        ..Perturbation::none()
    };
    assert_eq!(
        controlled(&session, target, &Perturbation::none()),
        Outcome::Survived,
        "the target passes as the baseline ran it"
    );
    assert_eq!(
        controlled(&session, target, &one_thread),
        Outcome::Errored,
        "`--test-threads=1` handed to a program that is not libtest is an argument it never \
         agreed to take, and whatever it did with it would be an answer about the apparatus"
    );
    session.close().expect("close");
}
