// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run decides about a mutant, including the one it has to ask twice about.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::time::Duration;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::run::Quiet;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session, Timeout, TimeoutSource};
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepared(fixture: &Fixture, env: &[(&str, String)]) -> Session {
    let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    for (name, value) in env {
        vars.push((
            std::ffi::OsString::from(name),
            std::ffi::OsString::from(value),
        ));
    }
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
            env: vars,
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
                tier: Tier::All,
                mutant_timeout: Timeout::Fixed(Duration::from_secs(2)),
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare")
}

/// The mutation of `rule` at `line`.
fn mutant(session: &Session, rule: &str, line: u32) -> String {
    session
        .catalog()
        .mutants()
        .iter()
        .find(|one| {
            one.candidate.rule.name == rule
                && session.position(one).is_some_and(|at| at.line == line)
        })
        .unwrap_or_else(|| panic!("a {rule} mutant on line {line}"))
        .display_id
        .clone()
}

#[test]
fn a_timeout_that_does_not_repeat_is_inconclusive_and_one_that_does_is_timed_out() {
    let fixture = Fixture::copy("fixture-hang");
    let markers = fixture.temp().join("markers");
    std::fs::create_dir_all(&markers).expect("the marker directory");
    let session = prepared(
        &fixture,
        &[
            (
                "FIXTURE_HANG_MARKER",
                markers.to_string_lossy().into_owned(),
            ),
            ("FIXTURE_HANG_PAUSE_MS", "4000".to_owned()),
        ],
    );
    let quiet = Quiet::default();
    let cancel = Cancel::new();

    let never = mutant(&session, "delete-compound-assignment", 13);
    let stopped = session
        .judge(&Request::new(never), &quiet, &cancel)
        .expect("judge");
    assert_eq!(stopped.result.outcome, Outcome::TimedOut);
    assert!(
        stopped.retried,
        "a timeout is believed only when it repeats"
    );
    assert_eq!(
        stopped.attempts.len(),
        2,
        "one expired budget buys one quiet measurement, and no more"
    );
    assert_eq!(stopped.timeout, Duration::from_secs(2));
    assert_eq!(stopped.timeout_source, TimeoutSource::Configured);

    let slow_once = mutant(&session, "gt-to-ge", 25);
    let undecided = session
        .judge(&Request::new(slow_once), &quiet, &cancel)
        .expect("judge");
    assert_eq!(
        undecided.result.outcome,
        Outcome::Inconclusive,
        "a mutation that was slow once and quick again is one the run cannot decide, and \
         calling it a timeout would report a finding the second measurement contradicts"
    );
    assert!(undecided.retried);
    assert_eq!(undecided.attempts.len(), 2);
    session.close().expect("close");
}

#[test]
fn a_mutant_nothing_delays_is_judged_once() {
    let fixture = Fixture::copy("fixture-hang");
    let session = prepared(&fixture, &[]);
    let ordinary = mutant(&session, "delete-compound-assignment", 12);
    let judged = session
        .judge(&Request::new(ordinary), &Quiet::default(), &Cancel::new())
        .expect("judge");
    assert_eq!(judged.result.outcome, Outcome::Killed);
    assert!(!judged.retried);
    assert_eq!(judged.attempts.len(), 1);
    session.close().expect("close");
}
