// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crash lets a call that writes finish and stops the process, and the next run starts over what it left (ADR 0035).

use std::collections::BTreeMap;

use njutest_devkit::fixture::Fixture;
use rust_mutants::instrument::CRASH_EXIT;
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

const TARGET: &str = "fixture-durable/test/counter";

#[test]
fn a_write_torn_by_a_stop_is_one_the_next_run_cannot_start_over() {
    let fixture = Fixture::copy("fixture-durable");
    let session = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            operators: vec!["crash-after-write".to_owned()],
            touch: true,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let mut restarted: BTreeMap<String, &str> = BTreeMap::new();
    for mutant in session.catalog().mutants() {
        let item = session.item_of(mutant.index).expect("an item").to_owned();
        let test = if item == "save_in_pieces" {
            "a_count_kept_in_pieces_goes_up"
        } else {
            "a_count_kept_whole_goes_up"
        };
        let asked = |mutant: String| {
            Request::new(mutant)
                .with_target(TARGET)
                .test(Some(test.to_owned()))
        };
        let (crashed, kept) = session
            .exec_keeping(&asked(mutant.id.to_string()), &Cancel::new())
            .expect("the crash runs");
        assert_eq!(
            crashed.exit_code,
            CRASH_EXIT,
            "the process stops just after the call at {item}:{}: {}",
            mutant.index,
            njutest_devkit::process::strict_utf8(&crashed.output)
        );
        assert!(
            kept.stopped(),
            "the runtime said it stopped at this call, which is what makes the status a stop"
        );
        assert!(
            !kept.left().expect("the scratch reads").is_empty(),
            "the crashed run left what it wrote in its scratch"
        );
        let next = session
            .control_in(&asked(String::new()), &kept, &Cancel::new())
            .expect("the next run runs");
        let said = match next.outcome() {
            Outcome::Survived => "passed",
            Outcome::Killed => "failed",
            other @ (Outcome::NotRun
            | Outcome::StepLimitReached
            | Outcome::Waited
            | Outcome::Inconclusive
            | Outcome::Errored) => other.name(),
        };
        restarted.insert(format!("{item}#{}", mutant.index), said);
    }
    assert_eq!(
        restarted,
        BTreeMap::from([
            ("save_in_pieces#0".to_owned(), "failed"),
            ("save_in_pieces#1".to_owned(), "failed"),
            ("save_in_pieces#2".to_owned(), "passed"),
            ("save_whole#3".to_owned(), "passed"),
            ("save_whole#4".to_owned(), "passed"),
        ]),
        "a truncated or half-written count is one the next run cannot read, and a count moved \
         into place whole never is"
    );
    session.close().expect("close");
}

#[test]
fn a_test_that_ends_with_the_stop_status_itself_is_not_a_stop() {
    let fixture = Fixture::copy("fixture-stop-status");
    let session = Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            operators: vec!["crash-after-write".to_owned()],
            touch: true,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare");
    let mutant = session
        .catalog()
        .mutants()
        .first()
        .expect("the one call that writes")
        .id
        .to_string();
    let (ran, kept) = session
        .exec_keeping(
            &Request::new(mutant)
                .with_target("fixture-stop-status/test/status")
                .test(Some("a_count_is_kept".to_owned())),
            &Cancel::new(),
        )
        .expect("the run runs");
    assert_eq!(
        ran.exit_code,
        CRASH_EXIT,
        "the program ends with the stop's status on its own, before the call: {}",
        njutest_devkit::process::strict_utf8(&ran.output)
    );
    assert!(
        !kept.stopped(),
        "a status the program chose is not a stop the runtime made, so nothing is decided on it"
    );
    session.close().expect("close");
}
