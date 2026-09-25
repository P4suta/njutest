// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run decides about a mutant, including the one it has to ask twice about.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::time::Duration;

use njutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::run::Quiet;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Judgement, PrepareOptions, Request, Session, Timeout, TimeoutSource};
use rust_mutants::workspace::{OpenOptions, Workspace};

/// A session over `fixture`, bounded so that each half of its contract is answered by the thing that should answer it.
///
/// The engine reads no configuration file, so a fixture's own `steps` does not reach here and the default of fifty million would apply.
/// A hundred takes spend about 789ms on Windows against this two-second bound, a margin of two and a half that load closes; ten spend 129ms and do not.
/// What settles the race is the cost of the count, not the length of the bound: a longer bound wins it by making every failure wait the bound out twice, which is the worst moment to make a suite slow to read (ADR 0023).
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
            cargo: Some(njutest_devkit::paths::cargo_binary()),
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
                mutant_steps: Some(10),
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare")
}

/// What to add to a failure when the judgement says the clock answered instead of the count, and nothing when it does not.
///
/// Both stoppers are timers, so which one answers is whichever arrives first, and on a slow enough machine that is the bound.
/// Left alone this fails saying only that `Waited` was not `StepLimitReached`, which reads as a broken step protocol and sends the reader into machinery that is working.
/// The allowance takes roughly a fifteenth of the bound here, so losing it means a take cost fifteen times what it costs on the machine this was measured on.
fn outran_by_the_clock(judged: &Judgement) -> String {
    if judged.result().outcome() != Outcome::Waited || judged.result().step_notice().is_some() {
        return String::new();
    }
    "\n\nThe bound answered before the count did, which is this machine being slow rather than \
     the step protocol being broken: no step notice was left, because the allowance was never \
     reached. Lower `mutant_steps` here — it costs nothing, and is bounded below only by what \
     an ordinary execution of this fixture spends, which is five"
        .to_owned()
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
        .to_string()
}

#[test]
fn a_mutation_that_cannot_end_is_stopped_by_a_count_and_one_that_is_merely_slow_by_the_clock() {
    let fixture = Fixture::copy("fixture-hang");
    let markers = fixture.temp().join("markers");
    std::fs::create_dir_all(&markers).expect("the marker directory");
    let session = prepared(
        &fixture,
        &[
            (
                "FIXTURE_HANG_MARKER",
                markers
                    .to_str()
                    .expect("fixture paths are exact UTF-8")
                    .to_owned(),
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
    assert_eq!(
        stopped.result().outcome(),
        Outcome::StepLimitReached,
        "the mutation deletes the step of a loop's counter, so the guard at its site is \
         taken once an iteration and the allowance is spent long before the bound. This says \
         where the execution stopped; it does not prove that the mutant cannot terminate.{}",
        outran_by_the_clock(&stopped)
    );
    assert!(stopped.result().step_notice().is_some());
    assert!(
        !stopped.retried(),
        "and it is not put again: the serial retry exists because a clock is unreliable, \
         and a count cannot disagree with itself on a second reading"
    );
    assert_eq!(stopped.attempts.attempt_count(), 1);
    assert_eq!(stopped.timeout, Duration::from_secs(2));
    assert_eq!(stopped.timeout_source, TimeoutSource::Configured);

    let slow_once = mutant(&session, "gt-to-ge", 25);
    let undecided = session
        .judge(&Request::new(slow_once), &quiet, &cancel)
        .expect("judge");
    assert_eq!(
        undecided.result().outcome(),
        Outcome::Inconclusive,
        "a mutation that was slow once and quick again is one the run cannot decide, and \
         calling it a wait would report a finding the second measurement contradicts. The \
         count does not answer here: nothing is spinning, the process is merely asleep"
    );
    assert!(undecided.retried());
    assert_eq!(undecided.attempts.attempt_count(), 2);
    session.close().expect("close");
}

#[test]
fn a_judgement_names_every_target_it_asked_and_what_each_answered() {
    let fixture = Fixture::copy("fixture-hang");
    let session = prepared(&fixture, &[]);
    let ordinary = mutant(&session, "delete-compound-assignment", 12);
    let judged = session
        .judge(&Request::new(ordinary), &Quiet::default(), &Cancel::new())
        .expect("judge");

    assert!(
        !judged.asked.is_empty(),
        "a run that establishes something about a mutation asked somebody, and a \
         judgement that cannot say who was asked leaves a reader unable to tell a \
         target that passed from one that was never given the chance: {judged:?}"
    );
    assert!(
        judged
            .asked
            .iter()
            .any(|one| one.target == judged.result().target),
        "the target whose answer the run took is one of the targets it asked"
    );
    assert!(
        judged.asked.iter().all(|one| !one.target.is_empty()),
        "and every one of them is named: {:?}",
        judged
            .asked
            .iter()
            .map(|one| &one.target)
            .collect::<Vec<_>>()
    );
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
    assert_eq!(judged.result().outcome(), Outcome::Killed);
    assert!(!judged.retried());
    assert_eq!(judged.attempts.attempt_count(), 1);
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
        self.started.push(mutant.display_id.to_string());
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
                jobs: rust_mutants::run::Jobs::count(jobs).expect("a positive count"),
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
        let total = usize::try_from(watching.total);
        assert!(total.is_ok(), "the progress total fits usize: {total:?}");
        let Ok(total) = total else { return Vec::new() };
        assert_eq!(
            total,
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

#[test]
fn the_budget_one_execution_gets_is_derived_from_what_that_target_cost() {
    let fixture = Fixture::copy("fixture-simple");
    let session = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
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
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("a session whose budget nobody chose");

    let target = session
        .targets()
        .first()
        .expect("a target the run built")
        .id
        .clone();
    let measured = session
        .baseline(&target)
        .expect("a target that was verified says what its own baseline took");
    assert!(
        measured > Duration::ZERO,
        "running a target takes time, and a budget derived from nothing is a budget nobody \
         calibrated: {measured:?}"
    );

    let (derived, source) = session
        .timeout_for(&Request::new(String::new()), &target)
        .expect("the measured baseline fits the timeout multiplier");
    assert_eq!(source, TimeoutSource::Derived);
    assert!(
        derived > measured,
        "a mutation is given more than the unmutated program took, or every mutation of a \
         target would time out on the machine that verified it: {derived:?} against {measured:?}"
    );

    let chosen = Duration::from_secs(17);
    assert_eq!(
        session
            .timeout_for(
                &Request::new(String::new()).with_timeout(Some(chosen)),
                &target
            )
            .expect("the chosen finite duration needs no derivation"),
        (chosen, TimeoutSource::Configured),
        "and a caller who chose one is a caller who chose one"
    );
    session.close().expect("the session closes");
}
