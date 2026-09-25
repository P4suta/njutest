// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A control started under a perturbation runs under it and is recorded apart, and one started without runs as the baseline did.

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
use rust_mutants::trace::{
    Measurement, MemorySink, Payload, PerturbedRecord, ReachRecord, Recorder, SetRecord, Sink,
};
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepare(fixture: &Fixture) -> Session {
    recording(fixture, Recorder::disabled())
}

/// `fixture` prepared with every event it records going to `trace`.
fn recording(fixture: &Fixture, trace: Recorder) -> Session {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            trace,
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
        },
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

#[test]
fn a_perturbed_control_is_recorded_apart_from_every_control_run_as_its_baseline_was() {
    let fixture = Fixture::copy("fixture-environment");
    let recorder = Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let session = recording(&fixture, recorder.clone());
    let zoned = Perturbation {
        environment: vec![(Variable::Tz, OsString::from("Australia/Lord_Howe"))],
        ..Perturbation::none()
    };
    for (target, outcome) in [
        ("environment/test/timezone", "killed"),
        ("environment/test/steady", "survived"),
    ] {
        let before = recorder.events().len();
        session
            .control_perturbed(
                &Request::new(String::new()).with_target(target),
                Conditions {
                    observing: Observing::Reach,
                    perturbation: &zoned,
                },
                &Cancel::new(),
            )
            .expect("the control runs");
        let written: Vec<Payload> = recorder
            .events()
            .into_iter()
            .skip(before)
            .map(|event| event.payload)
            .collect();
        let perturbed: Vec<&PerturbedRecord> = written
            .iter()
            .filter_map(|payload| {
                let Payload::PerturbedControl { perturbed } = payload else {
                    return None;
                };
                Some(perturbed)
            })
            .collect();
        let [one] = perturbed.as_slice() else {
            panic!("one perturbed record for the one control of {target}: {written:?}");
        };
        assert_eq!(one.target, target);
        assert_eq!(
            one.outcome, outcome,
            "only {target}'s own tests read the zone"
        );
        assert_eq!(
            one.perturbation.environment,
            [SetRecord {
                name: "TZ".to_owned(),
                value: Some("Australia/Lord_Howe".to_owned()),
            }],
            "the recording says what the control was started with, which is what an audit reads"
        );
        match (&one.reach, outcome) {
            (ReachRecord::NotRead, "killed") => {}
            (ReachRecord::Recorded { touch }, "survived") => assert_eq!(
                (touch.target.as_str(), touch.measured),
                (target, Measurement::Control),
                "a control that passed says what it reached, as a control of the target it ran"
            ),
            (reach, _) => panic!("{target} came to {outcome} and its reach is {reach:?}"),
        }
        assert!(
            !written.iter().any(|payload| matches!(
                payload,
                Payload::Touch { .. } | Payload::MutantExec { .. }
            )),
            "a perturbed control writes nothing a comparison under equal conditions reads: \
             {written:?}"
        );
    }
    session.close().expect("close");
}
