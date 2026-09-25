// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a mutant whose tests change what the run executes stops the run and is named, rather than every later mutant erroring for a reason it did not cause.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::rule::Tier;
use rust_mutants::run::{Quiet, Silent};
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

fn prepare(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
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
    .expect("prepare")
}

#[test]
fn a_mutant_that_removes_the_test_binaries_stops_the_run_and_is_named() {
    let fixture = Fixture::copy("fixture-apparatus");
    let session = prepare(&fixture);
    let sweeping: Vec<String> = session
        .catalog()
        .mutants()
        .iter()
        .filter(|mutant| session.item_of(mutant.index) == Some("sweep"))
        .map(|mutant| mutant.display_id.to_string())
        .collect();
    let answered = rust_mutants::run::run(
        &session,
        &rust_mutants::run::Options {
            expectations: &[],
            quiet: &Quiet::default(),
            equivalence: None,
            jobs: rust_mutants::run::Jobs::count(1).expect("a positive count"),
            args: &[],
            shard: None,
            outcomes: None,
            filter: None,
            fail_fast: false,
        },
        &Cancel::new(),
        &mut Silent,
    );
    let said = match &answered {
        Ok(run) => format!(
            "the run completed with {:?}",
            run.judged
                .iter()
                .map(|one| (one.display_id.as_str(), one.outcome))
                .collect::<Vec<_>>()
        ),
        Err(error) => error.to_string(),
    };
    assert!(
        answered.is_err()
            && said.contains("RM5009")
            && sweeping.iter().any(|mutant| said.contains(mutant.as_str())),
        "a mutant whose tests removed the binaries the run executes stops the run, naming \
         itself, rather than surviving while every mutant after it errors for a cause it did \
         not have: {said}"
    );
    session.close().expect("close");
}

#[test]
fn a_message_about_a_changed_apparatus_names_a_few_changes_and_counts_the_rest() {
    use rust_mutants::apparatus::{Change, NAMED, summary};
    let many: Vec<Change> = (0..100)
        .map(|at| Change::Missing(std::path::PathBuf::from(format!("/t/deps/file-{at}"))))
        .collect();
    let said = summary(&many);
    assert!(
        said.contains("/t/deps/file-0 is gone")
            && said.ends_with(&format!("and {} more", 100 - NAMED))
            && !said.contains("file-99"),
        "a test that empties the directory the binaries run from changes every file in it, and \
         a message naming each one is one nobody reads: {said}"
    );
    assert_eq!(
        summary(many.get(..2).expect("two changes")),
        "/t/deps/file-0 is gone; /t/deps/file-1 is gone",
        "and a few changes are named whole"
    );
}

#[test]
fn several_jobs_name_every_mutant_running_and_leave_the_callers_cancel_alone() {
    let fixture = Fixture::copy("fixture-apparatus");
    let session = prepare(&fixture);
    let emptying: Vec<String> = session
        .catalog()
        .mutants()
        .iter()
        .filter(|mutant| {
            ["negate-condition", "condition-to-true", "string-to-empty"]
                .contains(&mutant.candidate.rule.name)
        })
        .map(|mutant| mutant.display_id.to_string())
        .collect();
    let cancel = Cancel::new();
    let answered = rust_mutants::run::run(
        &session,
        &rust_mutants::run::Options {
            expectations: &[],
            quiet: &Quiet::default(),
            equivalence: None,
            jobs: rust_mutants::run::Jobs::count(4).expect("a positive count"),
            args: &[],
            shard: None,
            outcomes: None,
            filter: None,
            fail_fast: false,
        },
        &cancel,
        &mut Silent,
    );
    let said = match &answered {
        Ok(_run) => "the run completed".to_owned(),
        Err(error) => error.to_string(),
    };
    assert!(
        !cancel.is_cancelled(),
        "a run that stops its own workers after a failure has not been interrupted, and raising \
         the caller's cancel made the command exit 130 as though somebody had: {said}"
    );
    assert!(
        said.contains("RM5009") && emptying.iter().any(|mutant| said.contains(mutant.as_str())),
        "with several jobs the mutant that noticed the change may not be the one that made it, so \
         the run names every mutant that was running then, which includes the one that did: \
         {said}"
    );
    session.close().expect("close");
}
