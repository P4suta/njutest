// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What preparing does about a target whose own tests do not pass with nothing active.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::EngineError;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Failing, PrepareOptions, Request, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::trace::{MemorySink, Payload, Recorder, Sink};
use rust_mutants::workspace::{OpenOptions, SessionError, Workspace};

fn open(fixture: &Fixture) -> Workspace {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            offline: true,
            locked: true,
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open")
}

fn prepare(fixture: &Fixture, failing: Failing) -> Result<Session, EngineError> {
    open(fixture).prepare(
        &PrepareOptions {
            tier: Tier::All,
            failing,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
}

fn baseline_options(fixture: &Fixture) -> PrepareOptions {
    PrepareOptions {
        tier: Tier::All,
        coverage: false,
        branch_proofs: false,
        doctests: false,
        measurements: Some(fixture.cache().to_path_buf()),
        ..PrepareOptions::default()
    }
}

fn traced_prepare(fixture: &Fixture, recorder: &Recorder, options: &PrepareOptions) -> Session {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            trace: recorder.clone(),
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare(options, &Cancel::new())
    .expect("prepare")
}

fn remembered(trace: &Recorder) -> bool {
    trace.events().iter().any(|event| {
        matches!(
            &event.payload,
            Payload::Note { note } if note.kind == "baseline-remembered"
        )
    })
}

fn executions(trace: &Recorder) -> usize {
    trace
        .events()
        .iter()
        .filter(|event| matches!(event.payload, Payload::Exec { .. }))
        .count()
}

fn baseline_path(fixture: &Fixture) -> std::path::PathBuf {
    std::fs::read_dir(fixture.cache())
        .expect("cache directory")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("baseline-"))
        })
        .expect("the remembered baseline")
}

#[test]
fn refusing_names_every_target_that_failed_rather_than_the_one_it_reached_first() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let error = prepare(&fixture, Failing::Refuse).expect_err("refused");
    let EngineError::Session(SessionError::VerifyFailed { targets, output }) = error else {
        panic!("a tree whose baseline fails is refused by RM5002: {error}");
    };
    assert_eq!(
        targets,
        vec![
            "fixture-verify-fails/lib/fixture_verify_fails".to_owned(),
            "fixture-verify-fails/test/beside".to_owned(),
        ],
        "verification does not stop at the first failure, because somebody is about to fix \
         what it names"
    );
    assert!(
        output.contains("doubling_two_is_five"),
        "and it quotes the first of them: {output}"
    );
}

#[test]
fn excluding_hands_back_the_table_of_what_every_target_came_to() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let session = prepare(&fixture, Failing::Exclude)
        .expect("excluding does not refuse, because the caller asked for the table");
    let verified = session.verified();
    assert_eq!(
        verified.failing(),
        vec![
            "fixture-verify-fails/lib/fixture_verify_fails",
            "fixture-verify-fails/test/beside",
        ],
        "every target of this fixture fails, and the caller is told which"
    );
    for target in verified.failing() {
        assert!(
            verified.targets.contains_key(target),
            "the table covers every target that ran, not only the ones that passed"
        );
        assert!(
            session.targets().iter().all(|kept| kept.id != target),
            "{target} was left out of the run rather than measured against"
        );
    }
    for target in verified.failing() {
        assert!(
            verified
                .touched
                .limitations
                .contains(&format!("baseline-not-passing:{target}")),
            "the record says why {target} is not in it: {:?}",
            verified.touched.limitations
        );
    }
}

#[test]
fn a_session_whose_every_target_was_excluded_refuses_to_run_rather_than_score_nothing() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let session = prepare(&fixture, Failing::Exclude).expect("prepare");
    let mutant = session.catalog().mutants()[0].display_id.clone();
    let error = session
        .exec(&Request::new(mutant), &Cancel::new())
        .expect_err("a run of no targets is not a run that found nothing");
    assert!(
        matches!(error, EngineError::Session(SessionError::NoTargets { .. })),
        "{error}"
    );
}

#[test]
fn a_tree_that_passes_reports_a_baseline_for_every_target_it_ran() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture, Failing::Refuse).expect("prepare");
    let verified = session.verified();
    assert!(
        verified.failing().is_empty(),
        "nothing of this fixture fails: {:?}",
        verified.failing()
    );
    assert!(
        !verified.targets.is_empty(),
        "the run asked every target and kept what each came to"
    );
    for (target, baseline) in &verified.targets {
        assert!(baseline.passed(), "{target} passed");
        assert!(
            baseline.output.is_empty(),
            "{target} passed, so nobody reads what it printed"
        );
        assert_eq!(
            session.tests_of(target).max(1),
            baseline.tests.max(1),
            "what one target's baseline ran is what asking the whole of it costs"
        );
    }
}

#[test]
fn a_target_whose_every_test_is_ignored_says_so_rather_than_saying_nothing() {
    let fixture = Fixture::copy("fixture-ignored");
    let session = prepare(&fixture, Failing::Refuse).expect("prepare");
    let baseline = session
        .verified()
        .targets
        .get("fixture-ignored/lib/fixture_ignored")
        .expect("the library target was verified");
    assert_eq!(
        baseline.outcome,
        rust_mutants::outcome::Outcome::Inconclusive,
        "a target that ran no test decided nothing"
    );
    assert_eq!(baseline.tests, 0, "it ran none");
    assert_eq!(
        baseline.ignored, 2,
        "and it says how many it was told to skip, which is what tells it from a harness \
         that printed no summary at all"
    );
}

#[test]
fn an_exact_passing_baseline_is_reused_without_starting_its_targets_again() {
    let fixture = Fixture::copy("fixture-simple");
    let options = baseline_options(&fixture);

    let first_trace = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let first = traced_prepare(&fixture, &first_trace, &options);
    let first_targets = first.verified().targets.clone();
    let first_touched = first.verified().touched.clone();
    first.close().expect("close");
    assert!(
        !remembered(&first_trace),
        "the first run measured the baseline"
    );

    let second_trace = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let second = traced_prepare(&fixture, &second_trace, &options);
    assert_eq!(second.verified().targets, first_targets);
    assert_eq!(second.verified().touched, first_touched);
    let second_events = second_trace.events();
    assert!(
        remembered(&second_trace),
        "the second identical build reads the passing measurement"
    );
    assert_eq!(
        second_events
            .iter()
            .filter(|event| matches!(event.payload, Payload::Verify { .. }))
            .count(),
        second.targets().len(),
        "remembering work does not remove its auditable verify records"
    );
    assert!(
        executions(&first_trace) > executions(&second_trace),
        "the remembered run starts no baseline target process"
    );
    second.close().expect("close");

    let baseline_path = baseline_path(&fixture);
    let mut damaged: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&baseline_path).expect("read the remembered baseline"),
    )
    .expect("baseline document");
    damaged["touched"]["targets"] = serde_json::json!({});
    std::fs::write(
        &baseline_path,
        serde_json::to_vec(&damaged).expect("damaged document"),
    )
    .expect("damage the cache");
    let damaged_trace = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let measured_again = traced_prepare(&fixture, &damaged_trace, &options);
    assert!(
        !remembered(&damaged_trace),
        "a parseable but incomplete answer is a miss, never a narrower route"
    );
    measured_again.close().expect("close");

    let changed_trace = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let changed = traced_prepare(
        &fixture,
        &changed_trace,
        &PrepareOptions {
            harness_args: vec!["--test-threads=1".to_owned()],
            ..options
        },
    );
    assert!(
        changed_trace.events().iter().all(|event| !matches!(
            &event.payload,
            Payload::Note { note } if note.kind == "baseline-remembered"
        )),
        "a different harness invocation is a different baseline"
    );
    changed.close().expect("close");
}

#[test]
fn a_failing_baseline_is_never_remembered() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let session = open(&fixture)
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                coverage: false,
                branch_proofs: false,
                doctests: false,
                failing: Failing::Exclude,
                measurements: Some(fixture.cache().to_path_buf()),
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("excluding returns the failing table");
    assert!(!session.verified().failing().is_empty());
    assert!(
        std::fs::read_dir(fixture.cache())
            .expect("cache directory")
            .all(|entry| !entry
                .expect("cache entry")
                .file_name()
                .to_string_lossy()
                .starts_with("baseline-")),
        "a failure is never an answer for another run"
    );
}
