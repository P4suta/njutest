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

/// An observer that keeps what it was told rather than drawing it.
#[derive(Default)]
struct Watching {
    /// How many mutants the run said it was about to judge.
    total: u32,
    /// Every mutant it said it was starting, by identity.
    started: Vec<String>,
    /// How many it said it had finished.
    finished: u32,
    /// Whether it was told the run was over.
    over: bool,
}

impl rust_mutants::run::Observer for Watching {
    fn starting(&mut self, total: u32) {
        self.total = total;
    }

    fn started(&mut self, mutant: &rust_mutants::catalog::Mutant) {
        self.started.push(mutant.display_id.clone());
    }

    fn judged(&mut self, _judged: &rust_mutants::run::Judged, completed: u32, _total: u32) {
        self.finished = completed;
    }

    fn finished(&mut self, _duration: Duration) {
        self.over = true;
    }
}

#[test]
fn one_job_and_several_judge_a_catalog_the_same_way() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepared(&fixture, &[]);
    let quiet = Quiet::default();
    let cancel = Cancel::new();

    let answered = |jobs: usize| {
        let mut watching = Watching::default();
        let run = rust_mutants::run::run(
            &session,
            &rust_mutants::run::Options {
                expectations: &[],
                quiet: &quiet,
                equivalence: None,
                jobs,
                args: &[],
                shard: None,
                outcomes: None,
                filter: None,
                fail_fast: false,
            },
            &cancel,
            &mut watching,
        )
        .expect("the run answers");
        assert_eq!(
            usize::try_from(watching.total).unwrap_or(0),
            run.judged.len(),
            "a run says how many mutants it is about to judge before it judges one, or a \
             caller drawing progress has no denominator"
        );
        assert_eq!(
            watching.finished, watching.total,
            "and says so about each of them as it finishes"
        );
        assert_eq!(
            watching.started.len(),
            run.judged.iter().filter(|one| one.route.is_some()).count(),
            "and names each one as it starts it, or a caller drawing which mutant is running \
             now has nothing to draw: {:?}",
            watching.started
        );
        assert!(watching.over, "and says when there is no more to come");
        assert!(
            run.judged.iter().all(|one| one.route.is_some()),
            "every judged mutant carries the route it was put to, which is the only place a \
             reader sees a proof layer remove work"
        );
        run.judged
            .iter()
            .map(|one| (one.index, one.outcome, one.target.clone()))
            .collect::<Vec<_>>()
    };

    let alone = answered(1);
    let together = answered(4);
    assert!(
        alone.len() > 1,
        "this fixture holds more than one mutation: {alone:?}"
    );
    assert_eq!(
        alone, together,
        "measuring one mutant at a time and measuring several at once are two ways of doing \
         the same work, so a catalog answers the same either way; a run that differed would \
         make the answer depend on how busy the machine was"
    );
    session.close().expect("the session closes");
}
