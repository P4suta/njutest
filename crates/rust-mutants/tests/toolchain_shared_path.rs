// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One file two members compile, which is one set of mutants and two suites that could notice them.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

fn prepared(fixture: &Fixture) -> Session {
    let workspace = Workspace::open(
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
    .expect("open");
    workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare")
}

/// The one mutant of `rule` the shared file proposes.
fn only(session: &Session, rule: &str) -> String {
    let mut found = session
        .catalog()
        .mutants()
        .iter()
        .filter(|one| one.candidate.rule.name == rule);
    let one = found.next().unwrap_or_else(|| panic!("a {rule} mutant"));
    assert!(
        found.next().is_none(),
        "a file two crates compile is still one file, and a mutant is a place in a file"
    );
    one.display_id.clone()
}

#[test]
fn a_file_two_members_share_through_path_is_one_set_of_mutants_routed_to_both_packages() {
    let fixture = Fixture::copy("fixture-shared-path");
    let session = prepared(&fixture);
    let cancel = Cancel::new();

    for one in session.catalog().mutants() {
        assert_eq!(
            one.candidate.path, "shared/util.rs",
            "only the shared file holds anything to mutate"
        );
    }

    let killed_by_left = session
        .exec(&Request::new(only(&session, "le-to-lt")), &cancel)
        .expect("exec");
    assert_eq!(killed_by_left.outcome, Outcome::Killed);
    assert_eq!(killed_by_left.target, "left/lib/left");

    let killed_by_right = session
        .exec(&Request::new(only(&session, "negate-condition")), &cancel)
        .expect("exec");
    assert_eq!(
        (killed_by_right.outcome, killed_by_right.target.as_str()),
        (Outcome::Killed, "right/lib/right"),
        "neither member's suite can answer for the whole file, so a run that stopped at the \
         first member would report a survivor the other member kills"
    );
    session.close().expect("close");
}
