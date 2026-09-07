// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guards as the measurement: what a prepared session knows about which test reached which mutant, and how it knows it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Session};
use rust_mutants::testkit::measuring::Measuring;
use rust_mutants::workspace::{OpenOptions, Workspace};

const LIBRARY: &str = "fixture-coverage/lib/fixture_coverage";

fn prepared(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
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
            coverage: false,
            branch_proofs: false,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

/// The one mutant of `rule` on `line`.
fn mutant(session: &Session, rule: &str, line: u32) -> u32 {
    session
        .catalog()
        .mutants()
        .iter()
        .find(|one| {
            one.candidate.rule.name == rule
                && session.position(one).is_some_and(|at| at.line == line)
        })
        .unwrap_or_else(|| panic!("a {rule} mutant on line {line}"))
        .index
}

/// The tests of `target` the measurement says reached `index`, in name order.
fn reaching(session: &Session, target: &str, index: u32) -> Vec<String> {
    let touches = session
        .touched()
        .targets
        .get(target)
        .unwrap_or_else(|| panic!("{target} was measured"));
    touches
        .tests
        .iter()
        .filter(|(_, sites)| sites.contains(&index))
        .map(|(name, _)| name.clone())
        .collect()
}

#[test]
fn the_verify_run_measures_which_test_reached_each_mutant_without_a_coverage_build() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    let touches = session
        .touched()
        .targets
        .get(LIBRARY)
        .unwrap_or_else(|| panic!("{LIBRARY} was measured"));
    let mut ran = touches.ran.clone();
    ran.sort();
    assert_eq!(
        ran,
        [
            "tests::a_short_list_is_short",
            "tests::a_version_is_earlier_than_a_later_one",
            "tests::clamp_returns_the_smaller",
        ],
        "every test the baseline ran is what a site nothing was attributed to reaches"
    );
    assert!(
        touches.loose.is_empty(),
        "every guard of this fixture is reached on the test's own thread: {:?}",
        touches.loose
    );
}

#[test]
fn a_mutant_is_reached_by_the_tests_that_exercise_its_function_and_by_no_other() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    assert_eq!(
        reaching(&session, LIBRARY, mutant(&session, "le-to-lt", 8)),
        ["tests::clamp_returns_the_smaller"],
        "one test of three calls clamp"
    );
    assert_eq!(
        reaching(&session, LIBRARY, mutant(&session, "le-to-lt", 16)),
        ["tests::a_version_is_earlier_than_a_later_one"]
    );
    assert_eq!(
        reaching(&session, LIBRARY, mutant(&session, "le-to-lt", 28)),
        ["tests::a_short_list_is_short"]
    );
}

#[test]
fn a_target_the_run_could_not_record_is_named_as_unmeasured_rather_than_read_as_empty() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    let touched = session.touched();
    for target in session.targets() {
        let id = target.id.as_str();
        assert!(
            touched.targets.contains_key(id)
                || touched
                    .limitations
                    .iter()
                    .any(|limitation| limitation.ends_with(id)),
            "{id} is neither measured nor accounted for: {touched:?}"
        );
    }
}

#[test]
fn a_mutant_is_routed_to_the_tests_that_reached_it_rather_than_to_the_whole_target() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    let index = mutant(&session, "le-to-lt", 8);
    let one = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| one.index == index)
        .expect("the mutant");
    let route = session.route(one);
    assert_eq!(
        route.granularity(),
        "test",
        "the measurement named tests, so the route does: {route:?}"
    );
    assert_eq!(
        route.tests_of(LIBRARY),
        ["tests::clamp_returns_the_smaller"],
        "one test of three calls clamp, and one process runs it: {route:?}"
    );
    assert!(
        route.reaching().contains(&"fixture-coverage/test/upper"),
        "upper calls clamp, so it reaches the condition: {route:?}"
    );
    assert!(
        route.tests_of("fixture-coverage/test/upper").is_empty(),
        "the one test upper has is every test it has, so it runs unfiltered: {route:?}"
    );
}

#[test]
fn routing_by_test_starts_fewer_tests_than_routing_by_target_would() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture);
    let of = |target: &str| {
        session
            .touched()
            .targets
            .get(target)
            .map_or(1, |touches| touches.ran.len().max(1))
    };
    let mut narrowed = 0;
    let mut whole = 0;
    for one in session.catalog().mutants() {
        let route = session.route(one);
        narrowed += route.started(of);
        whole += route.reaching().into_iter().map(of).sum::<usize>();
    }
    assert!(
        narrowed < whole,
        "the measurement named tests, so fewer of them start: {narrowed} against {whole}"
    );
}

/// A prepared session over `name`, with a recording a test can read back.
fn recorded(fixture: &Fixture) -> (Session, rust_mutants::trace::Recorder) {
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let session = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
            env: std::env::vars_os().collect(),
            locked: true,
            offline: true,
            trace: trace.clone(),
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect("open")
    .prepare(&Measuring::GUARDS.options(Tier::All), &Cancel::new())
    .expect("prepare");
    (session, trace)
}

/// Every note of `kind` the recording holds.
fn notes(trace: &rust_mutants::trace::Recorder, kind: &str) -> Vec<String> {
    trace
        .events()
        .into_iter()
        .filter_map(|event| match event.payload {
            rust_mutants::trace::Payload::Note { note } if note.kind == kind => Some(note.detail),
            _ => None,
        })
        .collect()
}

#[test]
fn a_test_that_only_passes_beside_its_neighbour_takes_its_target_off_test_routing() {
    let fixture = Fixture::copy("fixture-order-dependent");
    let (session, trace) = recorded(&fixture);
    let cancel = Cancel::new();
    for one in session.catalog().mutants() {
        drop(
            session
                .judge(
                    &rust_mutants::session::Request::new(one.display_id.clone()),
                    &rust_mutants::run::Quiet::default(),
                    &cancel,
                )
                .expect("judge"),
        );
    }
    let said = notes(&trace, rust_mutants::session::TEST_ROUTING_UNSOUND);
    assert!(
        !said.is_empty(),
        "a set of tests that does not answer on its own is one the run says so about: {:?}",
        rust_mutants::testkit::trace::type_names(&trace.events())
    );
    assert!(
        said.iter()
            .any(|one| one.contains("did not pass on its own")),
        "the note says why the set is not one to route by: {said:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_site_a_test_reached_on_a_thread_of_its_own_reaches_every_test_of_its_target() {
    let fixture = Fixture::copy("fixture-threaded");
    let session = prepared(&fixture);
    let library = "fixture-threaded/lib/fixture_threaded";
    let touches = session
        .touched()
        .targets
        .get(library)
        .unwrap_or_else(|| panic!("{library} was measured"));
    assert!(
        !touches.loose.is_empty(),
        "a thread the test spawned has no name a test answers for: {touches:?}"
    );
    let spawned = mutant(&session, "gt-to-ge", 9);
    let owned = mutant(&session, "add-to-sub", 15);
    assert_eq!(
        session.touched().reaching(library, spawned),
        Some(rust_mutants::touch::Reaching::Whole),
        "what nothing could attribute reaches every test of the target"
    );
    assert_eq!(
        session.touched().reaching(library, owned),
        Some(rust_mutants::touch::Reaching::Tests(vec![
            "tests::the_test_itself_reaches_the_next".to_owned()
        ])),
        "and what the test itself reached is put to that test alone"
    );
    session.close().expect("close");
}
