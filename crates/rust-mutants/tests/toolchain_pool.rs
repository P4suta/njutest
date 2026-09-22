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
use rust_mutants::trace::{Payload, RouteRecord};
use rust_mutants::workspace::{OpenOptions, Workspace};

enum RelevantPayload<'a> {
    Route(&'a RouteRecord),
    Other,
}

const fn relevant_payload(payload: &Payload) -> RelevantPayload<'_> {
    match payload {
        Payload::Route { route } => RelevantPayload::Route(route),
        Payload::RunStart { .. }
        | Payload::PhaseStart { .. }
        | Payload::PhaseEnd { .. }
        | Payload::Open { .. }
        | Payload::Snapshot { .. }
        | Payload::Exec { .. }
        | Payload::DiscoverFile { .. }
        | Payload::Instrument { .. }
        | Payload::ValidateRound { .. }
        | Payload::Bisect { .. }
        | Payload::Build { .. }
        | Payload::Verify { .. }
        | Payload::Touch { .. }
        | Payload::Witness { .. }
        | Payload::SkipClaim { .. }
        | Payload::Kept { .. }
        | Payload::Cache { .. }
        | Payload::Select { .. }
        | Payload::Identical { .. }
        | Payload::Evidence { .. }
        | Payload::MutantExec { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => RelevantPayload::Other,
    }
}

fn prepared(fixture: &Fixture) -> Session {
    prepared_within(fixture, Timeout::Auto, 1_000_000)
}

/// A prepared session whose executions are bounded by `timeout`.
/// A session over `fixture`, with an allowance small enough that the count is never in a race with `timeout`.
///
/// The engine reads no configuration file, so the default of fifty million would apply here -- about a second and a half, against bounds these tests set at two seconds.
/// Which of the two answered would then be decided by the machine's load, and a test that asserts an outcome would pass or fail by it.
/// A million takes fires in about thirty milliseconds (ADR 0023).
fn prepared_within(fixture: &Fixture, timeout: Timeout, steps: u64) -> Session {
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
                mutant_steps: Some(steps),
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
    let session = prepared_within(
        &fixture,
        Timeout::Fixed(std::time::Duration::from_secs(2)),
        100,
    );
    let mut delivered = Delivered::default();
    let finished = measured(&session, 4, &mut delivered);
    session.close().expect("close");

    let timed_out = finished
        .judged
        .iter()
        .find(|one| one.outcome == Outcome::StepLimitReached)
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
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::Route(route) => Some(route.mutant.clone()),
            RelevantPayload::Other => None,
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
                one.identical,
                rust_mutants::run::CodegenIdentity::NotMeasured,
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
    let twice = njutest_devkit::reproducible::builds_the_same_twice();
    let answered: Vec<rust_mutants::run::CodegenIdentity> =
        survivors.iter().map(|one| one.identical).collect();
    assert!(
        if twice {
            answered.contains(&rust_mutants::run::CodegenIdentity::Identical)
        } else {
            answered
                .iter()
                .all(|identity| *identity == rust_mutants::run::CodegenIdentity::NotEstablished)
        },
        "the fixture holds a mutation the compiler renders identically, and what the layer \
         says is identical rather than equivalent — on a machine that renders one \
         unchanged tree two ways the layer still runs, because it cannot know beforehand, \
         and establishes neither answer rather than reporting a difference it did not \
         cause. This machine builds one tree the same twice: {twice}, and the survivors \
         answered {answered:?}"
    );
}
