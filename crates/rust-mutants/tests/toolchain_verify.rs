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

fn reuse_checks(
    fixture: &Fixture,
    options: &PrepareOptions,
    first: (
        &std::collections::BTreeMap<String, rust_mutants::session::Measured>,
        &rust_mutants::touch::Touched,
        usize,
    ),
) -> Recorder {
    let second_trace = memory_trace();
    let second = traced_prepare(fixture, &second_trace, options);
    assert_eq!(second.verified().targets, *first.0);
    assert_eq!(second.verified().touched, *first.1);
    let second_events = second_trace.events();
    assert!(
        remembered(&second_trace),
        "the second identical build reads the passing measurement"
    );
    let verify_records = second_events
        .iter()
        .filter(|event| matches!(event.payload, Payload::Verify { .. }))
        .count();
    let named = second.targets().len();
    second.close().expect("close");
    assert_eq!(
        verify_records, named,
        "remembering work does not remove its auditable verify records"
    );
    assert!(
        first.2 > executions(&second_trace),
        "the remembered run starts no baseline target process"
    );
    second_trace
}

fn damage_the_remembered_baseline(fixture: &Fixture) {
    let baseline_path = baseline_path(fixture);
    let mut damaged: serde_json::Value = njutest_devkit::strictjson::decode_slice(
        &std::fs::read(&baseline_path).expect("read the remembered baseline"),
    )
    .expect("baseline document");
    damaged["touched"]["targets"] = serde_json::json!({});
    std::fs::write(
        &baseline_path,
        serde_json::to_vec(&damaged).expect("damaged document"),
    )
    .expect("damage the cache");
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

fn memory_trace() -> Recorder {
    Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    )
}

fn baseline_path(fixture: &Fixture) -> std::path::PathBuf {
    std::fs::read_dir(fixture.cache())
        .expect("cache directory")
        .map(|entry| entry.expect("cache directory entry"))
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with("baseline-"))
        })
        .expect("the remembered baseline")
}

#[test]
fn refusing_names_every_target_that_failed_rather_than_the_one_it_reached_first() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let error = prepare(&fixture, Failing::Refuse).expect_err("refused");
    assert!(
        matches!(
            &error,
            EngineError::Session(SessionError::VerifyFailed { .. })
        ),
        "a tree whose baseline fails is refused by RM5002: {error}"
    );
    let EngineError::Session(SessionError::VerifyFailed { targets, output }) = error else {
        return;
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
        .exec(&Request::new(mutant.to_string()), &Cancel::new())
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
        assert!(
            baseline.judgeable().is_some(),
            "{target} passed, so it hands back something a mutation can be judged against"
        );
        assert!(
            baseline.baseline().output.is_empty(),
            "{target} passed, so nobody reads what it printed"
        );
        assert_eq!(
            session.tests_of(target).max(1),
            baseline.baseline().tests.max(1),
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
        baseline.baseline().outcome,
        rust_mutants::outcome::Outcome::Inconclusive,
        "a target that ran no test decided nothing"
    );
    assert_eq!(baseline.baseline().tests, 0, "it ran none");
    assert_eq!(
        baseline.baseline().ignored,
        2,
        "and it says how many it was told to skip, which is what tells it from a harness \
         that printed no summary at all"
    );
}

#[test]
fn an_exact_passing_baseline_is_reused_without_starting_its_targets_again() {
    let fixture = Fixture::copy("fixture-simple");
    let options = baseline_options(&fixture);

    let first_trace = memory_trace();
    let first = traced_prepare(&fixture, &first_trace, &options);
    let first_targets = first.verified().targets.clone();
    let first_touched = first.verified().touched.clone();
    first.close().expect("close");
    assert!(
        !remembered(&first_trace),
        "the first run measured the baseline"
    );

    if !njutest_devkit::reproducible::builds_the_same_twice() {
        let again = memory_trace();
        let measured = traced_prepare(&fixture, &again, &options);
        assert!(
            !remembered(&again),
            "a remembered baseline is an answer about a program, and this machine builds \
             one tree to two of them: the bytes it would be answering about are not the \
             bytes anything ran"
        );
        measured.close().expect("close");
        return;
    }

    let second_trace = reuse_checks(
        &fixture,
        &options,
        (&first_targets, &first_touched, executions(&first_trace)),
    );
    drop(second_trace);

    damage_the_remembered_baseline(&fixture);
    let damaged_trace = memory_trace();
    let refused = Workspace::open(
        fixture.root(),
        OpenOptions {
            trace: damaged_trace.clone(),
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare(&options, &Cancel::new());
    assert!(
        !remembered(&damaged_trace),
        "a refused answer is not a remembered one"
    );
    assert!(
        matches!(
            &refused,
            Err(EngineError::BaselineCache(
                rust_mutants::session::BaselineCacheError::Contradiction { .. }
            ))
        ),
        "a remembered answer whose facts do not match its digest is refused by name, \
         never silently narrowed: {refused:?}"
    );

    let changed_trace = memory_trace();
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
                .to_str()
                .expect("the fixture writes exact UTF-8 names")
                .starts_with("baseline-")),
        "a failure is never an answer for another run"
    );
}

/// A target whose first run fails and whose second passes, keyed by a file it leaves in the run scratch above the temporary directory the run gives it: every process of one run shares that scratch, so the second finds what the first wrote, as a failure something outside the code decided is gone the second time.
const PASSES_ON_RETRY: &str = r#"// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose own test fails the first time it is run and passes the second.

/// Twice `n`.
#[must_use]
pub fn double(n: i32) -> i32 {
    n * 2
}

#[cfg(test)]
mod tests {
    use super::double;

    #[test]
    fn doubling_two_is_four_once_this_has_run_before() {
        let temporary = std::env::temp_dir();
        let run = temporary
            .ancestors()
            .find(|dir| {
                dir.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.starts_with("rm-scratch-"))
            })
            .expect("a run scratch above the temporary directory");
        let mark = run.join("been-here-before");
        assert!(mark.exists() || std::fs::write(&mark, b"1").is_err());
        assert_eq!(double(2), 4);
    }
}
"#;

/// A second target that passes every time, so that what the run says is about the first one.
const BESIDE: &str = r"// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target beside the library that passes whenever it is run.

#[test]
fn doubling_three_is_six() {
    assert_eq!(fixture_verify_fails::double(3), 6);
}
";

#[test]
fn a_target_that_did_not_pass_the_first_time_is_run_once_more_before_the_session_refuses() {
    let fixture = Fixture::copy("fixture-verify-fails");
    fixture.write("src/lib.rs", PASSES_ON_RETRY.as_bytes());
    fixture.write("tests/beside.rs", BESIDE.as_bytes());
    let session = prepare(&fixture, Failing::Refuse).expect(
        "a target that passes the second time is one a mutation can be put to, and a run \
         that refused it would have thrown away everything it spent getting here",
    );
    let verified = session.verified();
    assert!(
        verified.failing().is_empty(),
        "the second answer is the one the run is measured against: {:?}",
        verified.failing()
    );
    assert!(
        verified.touched.limitations.iter().any(|one| {
            one == &format!(
                "{}:fixture-verify-fails/lib/fixture_verify_fails",
                rust_mutants::limitation::BASELINE_PASSED_ON_RETRY
            )
        }),
        "and the run says which target it was, because a single result against a target \
         that once came out differently is worth that much less: {:?}",
        verified.touched.limitations
    );
}

/// A target whose own tests fail hands back nothing a result may rest on.
///
/// The check used to be a method every caller had to remember to call, and a caller who forgot would report a kill for every mutation put to a target that answers every one of them with the same failure.
/// There is no longer a way from a failing baseline to something a judgement can take.
#[test]
fn a_failing_baseline_is_not_something_a_result_can_rest_on() {
    let fixture = Fixture::copy("fixture-verify-fails");
    let session = prepare(&fixture, Failing::Exclude).expect("the table rather than a refusal");
    let verified = session.verified();

    let failing = verified.failing();
    assert!(
        !failing.is_empty(),
        "this fixture fails its own tests, which is what it is for"
    );
    for target in failing {
        assert!(
            verified.judgeable(target).is_none(),
            "{target} answers every mutation with the same failure, so there is nothing \
             here a kill could be about"
        );
        assert!(
            verified
                .targets
                .get(target)
                .is_some_and(|measured| !measured.baseline().output.is_empty()),
            "{target} still says what it printed, because a reader has to be told which \
             target it was and why"
        );
    }
}
