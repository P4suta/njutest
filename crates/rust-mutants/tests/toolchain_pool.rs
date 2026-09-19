// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Measuring several mutants at once establishes the same thing as measuring them one at a time.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::run::{Judged, NotRunReason, Observer, Options, Quiet, Run, run};
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Session, Timeout};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepared(fixture: &Fixture) -> Session {
    prepared_within(fixture, Timeout::Auto)
}

/// A prepared session whose executions are bounded by `timeout`.
fn prepared_within(fixture: &Fixture, timeout: Timeout) -> Session {
    let workspace = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open");
    workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                mutant_timeout: timeout,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare")
}

/// What each mutant was decided to be, in catalog order.
fn outcomes(finished: &Run) -> Vec<(String, Outcome)> {
    finished
        .judged
        .iter()
        .map(|one| (one.display_id.clone(), one.outcome))
        .collect()
}

/// The order the observer heard about them in.
#[derive(Default)]
struct Delivered {
    order: Vec<String>,
    counts: Vec<u32>,
}

impl Observer for Delivered {
    fn judged(&mut self, judged: &Judged, completed: u32, _total: u32) {
        self.order.push(judged.display_id.clone());
        self.counts.push(completed);
    }
}

fn measured(session: &Session, jobs: usize, observer: &mut Delivered) -> Run {
    let quiet = Quiet::default();
    run(
        session,
        &Options {
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
        &Cancel::new(),
        observer,
    )
    .expect("the run finishes")
}

#[test]
fn a_run_with_four_workers_judges_the_same_set_as_one_and_the_report_is_in_catalog_order() {
    let one = Fixture::copy("fixture-modern");
    let alone = prepared(&one);
    let mut serially = Delivered::default();
    let serial = measured(&alone, 1, &mut serially);
    alone.close().expect("close");

    let four = Fixture::copy("fixture-modern");
    let together = prepared(&four);
    let mut in_parallel = Delivered::default();
    let parallel = measured(&together, 4, &mut in_parallel);
    together.close().expect("close");

    assert_eq!(
        outcomes(&parallel),
        outcomes(&serial),
        "how many mutants a run measures at once is not a fact about any of them"
    );
    assert_eq!(
        parallel
            .judged
            .iter()
            .map(|one| one.index)
            .collect::<Vec<_>>(),
        serial
            .judged
            .iter()
            .map(|one| one.index)
            .collect::<Vec<_>>(),
        "the report is in catalog order however the run got there, because that is the order \
         a reader compares two runs in"
    );
    assert_eq!(
        in_parallel.counts,
        (1..=u32::try_from(parallel.judged.len()).expect("a count")).collect::<Vec<_>>(),
        "the count a caller is given is how many have been delivered, not which one this is"
    );
    let mut delivered = in_parallel.order.clone();
    delivered.sort();
    let mut expected: Vec<String> = serially.order.clone();
    expected.sort();
    assert_eq!(delivered, expected);
}

#[test]
fn a_slow_mutant_does_not_delay_the_delivery_of_the_ones_that_finished() {
    let fixture = Fixture::copy("fixture-hang");
    let session = prepared_within(&fixture, Timeout::Fixed(std::time::Duration::from_secs(2)));
    let mut delivered = Delivered::default();
    let finished = measured(&session, 4, &mut delivered);
    session.close().expect("close");

    let timed_out = finished
        .judged
        .iter()
        .find(|one| one.outcome == Outcome::Waited)
        .expect("the mutation that never returns");
    let at = delivered
        .order
        .iter()
        .position(|id| *id == timed_out.display_id)
        .expect("it was delivered");
    assert!(
        at > 0,
        "a mutant that runs for its whole budget would otherwise hold back every result behind \
         it, and a progress line would wait on it: {:?}",
        delivered.order
    );
}

/// An observer that stops the run as soon as it hears about one mutant.
struct StopsAtTheFirst<'a> {
    cancel: &'a Cancel,
    heard: u32,
}

impl Observer for StopsAtTheFirst<'_> {
    fn judged(&mut self, _judged: &Judged, _completed: u32, _total: u32) {
        self.heard = self.heard.saturating_add(1);
        self.cancel.cancel();
    }
}

#[test]
fn cancellation_leaves_every_unjudged_mutant_not_run_and_the_run_interrupted() {
    let fixture = Fixture::copy("fixture-modern");
    let session = prepared(&fixture);
    let cancel = Cancel::new();
    let quiet = Quiet::default();
    let mut observer = StopsAtTheFirst {
        cancel: &cancel,
        heard: 0,
    };
    let finished = run(
        &session,
        &Options {
            expectations: &[],
            quiet: &quiet,
            equivalence: None,
            jobs: 4,
            args: &[],
            shard: None,
            outcomes: None,
            filter: None,
            fail_fast: false,
        },
        &cancel,
        &mut observer,
    )
    .expect("the run ends");
    session.close().expect("close");

    assert!(finished.interrupted, "a run that was stopped says so");
    let not_run = finished
        .judged
        .iter()
        .filter(|one| one.outcome == Outcome::NotRun)
        .count();
    assert!(
        not_run > 0,
        "a mutant nothing measured is one the run says it never reached"
    );
    for one in &finished.judged {
        if one.outcome == Outcome::NotRun {
            assert_eq!(
                one.not_run_reason.map(NotRunReason::name),
                Some("interrupted"),
                "and says why, so nobody reads it as a mutation no test can notice"
            );
        }
    }
    assert!(
        observer.heard > 0,
        "the run delivered what it had measured before it was stopped"
    );
}

#[test]
fn every_judged_mutant_leaves_one_route_record_from_the_engine() {
    let fixture = Fixture::copy("fixture-simple");
    let recorder = rust_mutants::testkit::trace::memory_recorder();
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            trace: recorder.clone(),
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare");
    let mut delivered = Delivered::default();
    let finished = measured(&session, 4, &mut delivered);
    session.close().expect("close");

    let routes: Vec<rust_mutants::trace::Event> = recorder
        .events()
        .into_iter()
        .filter(|event| event.payload.type_name() == "route")
        .collect();
    assert_eq!(
        routes.len(),
        finished.judged.len(),
        "one record per judged mutant, whatever became of it"
    );
    let mut named: Vec<String> = routes
        .iter()
        .filter_map(|event| match &event.payload {
            rust_mutants::trace::Payload::Route { route } => Some(route.mutant.clone()),
            _ => None,
        })
        .collect();
    named.sort();
    named.dedup();
    assert_eq!(
        named.len(),
        finished.judged.len(),
        "and never two for one mutant, however many times it was executed"
    );
}

#[test]
fn the_equivalence_layer_asks_only_about_survivors_and_writes_identical_never_equivalent() {
    let fixture = Fixture::copy("fixture-equivalent");
    let session = prepared(&fixture);
    let quiet = Quiet::default();
    let asking = rust_mutants::run::Equivalence {
        root: fixture.root(),
        options: rust_mutants::equivalence::ProveOptions {
            build: rust_mutants::cargo::BuildConfig::default(),
            open: opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
            timeout: None,
        },
    };
    let finished = run(
        &session,
        &Options {
            expectations: &[],
            quiet: &quiet,
            equivalence: Some(&asking),
            jobs: 1,
            args: &[],
            shard: None,
            outcomes: None,
            filter: None,
            fail_fast: false,
        },
        &Cancel::new(),
        &mut Delivered::default(),
    )
    .expect("the run finishes");
    session.close().expect("close");

    for one in &finished.judged {
        if one.outcome != Outcome::Survived {
            assert_eq!(
                one.identical, None,
                "a mutation a test noticed is one the compiler plainly rendered, and asking \
                 about it would pay a build for an answer the run already has"
            );
        }
    }
    let survivors: Vec<&_> = finished
        .judged
        .iter()
        .filter(|one| one.outcome == Outcome::Survived)
        .collect();
    assert!(
        if njutest_devkit::reproducible::builds_the_same_twice() {
            survivors.iter().any(|one| one.identical == Some(true))
        } else {
            survivors.iter().all(|one| one.identical.is_none())
        },
        "the fixture holds a mutation the compiler renders identically, and what the layer \
         says is identical rather than equivalent — on a machine that renders one \
         unchanged tree two ways it says nothing at all instead, because a difference it \
         did not cause is not one to report"
    );
}
