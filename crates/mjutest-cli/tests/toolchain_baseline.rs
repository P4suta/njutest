// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The baseline: what the one verified run of every target observed.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_cli::assure::baseline::{
    Baseline, Measured, RAN_NOTHING, Reporting, observe, refused, status_of,
};
use mjutest_cli::report::TargetStatus;
use mjutest_cli::trace::{Clock, MemorySink, Payload, Recorder, Sink, StartRecord};
use mjutest_cli::watch::Watch;
use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Failing, PrepareOptions, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepared(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            offline: true,
            locked: true,
            ..opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            failing: Failing::Exclude,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

fn measure(fixture: &Fixture) -> Baseline {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    observe(
        &prepared(fixture),
        Reporting {
            notes: &mut mjutest_cli::ui::Notes::Silent,
            watch: Watch::new(&cancel, &trace),
        },
    )
}

fn named<'a>(baseline: &'a Baseline, name: &str) -> &'a Measured {
    baseline
        .targets
        .iter()
        .find(|measured| measured.target.name() == name)
        .unwrap_or_else(|| {
            panic!(
                "no target called {name}; there are {:?}",
                baseline
                    .targets
                    .iter()
                    .map(|one| one.target.name())
                    .collect::<Vec<String>>()
            )
        })
}

#[test]
fn a_target_is_a_binary_and_its_row_says_how_many_tests_it_ran() {
    let fixture = Fixture::copy("fixture-simple");
    let baseline = measure(&fixture);

    for measured in &baseline.targets {
        assert!(
            measured.target.is_whole_binary(),
            "{} is a row about a binary, not about one of its tests",
            measured.target.name()
        );
    }
    let mut names: Vec<String> = baseline
        .targets
        .iter()
        .map(|one| one.target.name())
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(
        names.len(),
        baseline.targets.len(),
        "one row per binary, each named once"
    );

    let lib = named(&baseline, "fixture-simple/lib/fixture_simple");
    assert_eq!(lib.status, TargetStatus::Passed);
    assert!(
        lib.tests > 0,
        "the row carries how many tests the run of it executed, which is what its \
         duration is the cost of"
    );
}

#[test]
fn a_target_whose_own_tests_fail_is_a_row_and_a_finding_rather_than_a_refusal() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let baseline = measure(&fixture);

    let failed: Vec<&Measured> = baseline
        .targets
        .iter()
        .filter(|one| one.status == TargetStatus::Failed)
        .collect();
    assert!(
        failed.len() >= 2,
        "the table names every target that failed rather than the first: {:?}",
        baseline
            .targets
            .iter()
            .map(|one| (one.target.name(), one.status))
            .collect::<Vec<(String, TargetStatus)>>()
    );
    for measured in failed {
        assert!(
            measured
                .message
                .as_ref()
                .is_some_and(|said| !said.is_empty()),
            "{} failed and the row says what it said",
            measured.target.name()
        );
    }
    assert!(
        baseline
            .limitations
            .iter()
            .any(|name| name.starts_with(mjutest_cli::assure::baseline::NOT_PASSING_LIMITATION)),
        "and the run states why those targets answer nothing: {:?}",
        baseline.limitations
    );
}

#[test]
fn a_target_libtest_skipped_every_test_of_is_not_a_target_nothing_is_known_about() {
    let fixture = Fixture::copy("fixture-ignored");
    let baseline = measure(&fixture);

    let skipped: Vec<&Measured> = baseline
        .targets
        .iter()
        .filter(|one| one.status == TargetStatus::Skipped)
        .collect();
    assert!(
        !skipped.is_empty(),
        "a target whose every test carries #[ignore] ran nothing and was told to: {:?}",
        baseline
            .targets
            .iter()
            .map(|one| (one.target.name(), one.status, one.tests))
            .collect::<Vec<(String, TargetStatus, u32)>>()
    );
    for measured in skipped {
        assert_ne!(
            measured.status,
            TargetStatus::Missing,
            "{} is a target with nothing to say, not one nothing was learned about, and \
             only the second is a finding",
            measured.target.name()
        );
    }
}

#[test]
fn what_one_target_came_to_is_read_off_what_the_engine_said_about_it() {
    use rust_mutants::outcome::Outcome;

    for (outcome, ignored, output, status, says) in [
        (Outcome::Survived, 0, "", TargetStatus::Passed, None),
        (Outcome::Survived, 3, "", TargetStatus::Passed, None),
        (
            Outcome::Inconclusive,
            1,
            "",
            TargetStatus::Skipped,
            Some("libtest was told to skip every test of it: 1 ignored"),
        ),
        (
            Outcome::Inconclusive,
            2,
            "",
            TargetStatus::Skipped,
            Some("libtest was told to skip every test of it: 2 ignored"),
        ),
        (
            Outcome::Inconclusive,
            0,
            "",
            TargetStatus::Missing,
            Some(RAN_NOTHING),
        ),
    ] {
        let (was, said) = status_of(outcome, ignored, output);
        assert_eq!(was, status, "{outcome:?} with {ignored} ignored");
        assert_eq!(said.as_deref(), says, "{outcome:?} with {ignored} ignored");
    }
}

#[test]
fn a_target_that_did_not_pass_says_what_a_reader_acts_on() {
    use rust_mutants::outcome::Outcome;

    for (outcome, ignored, output, status, says) in [
        (
            Outcome::Killed,
            0,
            "   Compiling pkg\ntest src/lib.rs - f (line 7) ... FAILED\n",
            TargetStatus::Failed,
            Some("test src/lib.rs - f (line 7) ... FAILED"),
        ),
        (
            Outcome::Killed,
            0,
            "   Compiling pkg v0.1.0\nerror: could not compile\n",
            TargetStatus::Failed,
            Some("error: could not compile"),
        ),
        (
            Outcome::Killed,
            0,
            "   only a build log\n",
            TargetStatus::Failed,
            Some("only a build log"),
        ),
        (
            Outcome::Killed,
            0,
            "",
            TargetStatus::Failed,
            Some("the target failed"),
        ),
        (
            Outcome::TimedOut,
            0,
            "",
            TargetStatus::Failed,
            Some("the target ran out of time"),
        ),
        (
            Outcome::NotRun,
            0,
            "",
            TargetStatus::Missing,
            Some("the target was not run, so nothing was observed"),
        ),
        (
            Outcome::Errored,
            0,
            "",
            TargetStatus::Missing,
            Some("the target could not be started, so nothing was observed"),
        ),
        (
            Outcome::Errored,
            0,
            "error: the harness died\n",
            TargetStatus::Missing,
            Some("error: the harness died"),
        ),
    ] {
        let (was, said) = status_of(outcome, ignored, output);
        assert_eq!(
            was, status,
            "{outcome:?} with {ignored} ignored is {status:?}"
        );
        assert_eq!(
            said.as_deref(),
            says,
            "{outcome:?} with {ignored} ignored says what a reader acts on"
        );
    }
}

#[test]
fn a_target_that_failed_quotes_the_test_that_failed_and_not_the_build_log() {
    use rust_mutants::outcome::Outcome;

    let (_was, said) = status_of(
        Outcome::Killed,
        0,
        "   Compiling fixture v0.1.0\ntest adds ... ok\nerror: unrelated\ntest doubling ... FAILED\n",
    );
    assert_eq!(
        said.as_deref(),
        Some("test doubling ... FAILED"),
        "a target cargo runs prints a build log first, and which crate was compiled is \
         true and not what somebody looking at a failing test needs. The line has to \
         both name a test and say it failed: a test that passed names one and did not, \
         and taking either would quote the wrong line"
    );
}

#[test]
fn a_workspace_that_does_not_compile_is_a_finding_and_not_an_error() {
    let broken = mjutest_cli::error::RunnerError::from(rust_mutants::EngineError::from(
        rust_mutants::workspace::SessionError::PristineBroken {
            first: "error[E0425]: cannot find value `x`".to_owned(),
        },
    ));
    let said = refused(&broken).expect("a build failure is what this run says about the tree");
    assert_eq!(
        said.failure.as_deref(),
        Some("error[E0425]: cannot find value `x`"),
        "the compiler's first line is what a person reading the report acts on"
    );
    assert!(said.targets.is_empty());

    let elsewhere = mjutest_cli::error::RunnerError::from(rust_mutants::EngineError::from(
        rust_mutants::workspace::SessionError::NoTargets {
            packages: vec!["pkg".to_owned()],
        },
    ));
    assert!(
        refused(&elsewhere).is_none(),
        "every other refusal is about this run rather than about the tree, and turning \
         one into a finding would report a broken run as a broken workspace"
    );
}

/// A recorder over a memory sink, which is what a test reads back.
fn recording() -> Recorder {
    Recorder::new(
        Sink::Memory(MemorySink::unbounded()),
        Clock::stepping(
            jiff::Timestamp::from_second(1_800_000_000).expect("in range"),
            std::time::Duration::from_secs(1),
        ),
        StartRecord::of(
            "20260908T000000Z-000001",
            mjutest_cli::report::RunKind::Full,
            mjutest_cli::config::Contract::StandardV1,
        ),
    )
}

#[test]
fn reading_the_baseline_is_a_phase_that_says_where_it_is() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepared(&fixture);
    let cancel = Cancel::new();
    let trace = recording();

    let baseline = observe(
        &session,
        Reporting {
            notes: &mut mjutest_cli::ui::Notes::Silent,
            watch: Watch::new(&cancel, &trace),
        },
    );

    let events = trace.events();
    let phases: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::PhaseStart { phase } | Payload::PhaseEnd { phase } => {
                Some(phase.name.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        vec!["baseline-measure", "baseline-measure"],
        "a phase that starts and does not end leaves a reader waiting for a duration \
         that never comes: {events:?}"
    );

    let progress: Vec<(String, Option<u64>, Option<u64>)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::Progress { progress } => {
                Some((progress.message.clone(), progress.done, progress.total))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        progress.len(),
        baseline.targets.len(),
        "one line per target the baseline carries: {progress:?}"
    );
    let total = u64::try_from(baseline.targets.len()).expect("a small number");
    for (at, (named, done, said)) in progress.iter().enumerate() {
        let counted = u64::try_from(at).expect("a small number").saturating_add(1);
        assert_eq!(
            (done, said),
            (&Some(counted), &Some(total)),
            "progress counts from one to the number of targets there are; a count that \
             starts elsewhere or names a different total is a reader watching the wrong \
             number approach the wrong end: {progress:?}"
        );
        assert_eq!(
            Some(named.as_str()),
            baseline
                .targets
                .get(at)
                .map(|one| one.target.name())
                .as_deref(),
            "and names the target it is about, in the order the rows are in"
        );
    }
}
