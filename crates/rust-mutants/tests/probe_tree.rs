// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The probe pass against a real workspace: what it can ask about, what it records, and the one invariant everything else rests on.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

struct Fixture {
    root: PathBuf,
    temp: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-probe-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy(&mjutest_devkit::paths::fixtures_dir().join(name), &root);
    let temp = dir.path().join("temp");
    std::fs::create_dir_all(&temp).expect("mkdir");
    Fixture {
        root,
        temp,
        _dir: dir,
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        if entry.file_name() == "target" {
            continue;
        }
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), &destination).expect("copy");
        }
    }
}

fn prepared(fixture: &Fixture, cancel: &Cancel) -> Session {
    Workspace::open(
        &fixture.root,
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp.clone(),
            env: std::env::vars_os().collect(),
            offline: true,
            locked: true,
            ..OpenOptions::default()
        },
        cancel,
    )
    .expect("the workspace opens")
    .prepare(
        &PrepareOptions {
            tier: Tier::All,
            verify: false,
            probe: true,
            ..PrepareOptions::default()
        },
        cancel,
    )
    .expect("the session prepares")
}

#[test]
fn the_probe_asks_about_what_it_can_and_leaves_the_rest_alone() {
    let fixture = fixture("fixture-probeable");
    let cancel = Cancel::new();
    let session = prepared(&fixture, &cancel);
    let probed = session.probed();
    assert!(
        probed.limitations.is_empty(),
        "the probe tree built and ran: {:?}",
        probed.limitations
    );

    let asked: Vec<String> = probed
        .asked
        .iter()
        .filter_map(|index| session.catalog().by_index(*index))
        .map(|mutant| {
            format!(
                "{}:{}",
                mutant.candidate.rule.name,
                session.position(mutant).map_or(0, |at| at.line)
            )
        })
        .collect();
    assert!(
        asked.iter().any(|one| one.starts_with("return-default")),
        "a literal is something a probe can ask about: {asked:?}"
    );
    assert!(
        !asked.is_empty(),
        "the fixture holds probeable return replacements"
    );
    assert!(
        !probed.infected.is_empty(),
        "every test wrote what it infected: {probed:?}"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_test_that_killed_a_mutant_is_one_the_probe_recorded_infecting_it() {
    let fixture = fixture("fixture-probeable");
    let cancel = Cancel::new();
    let session = prepared(&fixture, &cancel);
    let probed = session.probed().clone();

    for index in session.accepted() {
        if !probed.asked.contains(index) {
            continue;
        }
        let Some(mutant) = session.catalog().by_index(*index) else {
            continue;
        };
        for target in session.targets() {
            let result = session
                .exec(
                    &Request {
                        mutant: mutant.id.clone(),
                        target: Some(target.id.clone()),
                        ..Request::default()
                    },
                    &cancel,
                )
                .expect("the mutant runs");
            if result.outcome != Outcome::Killed {
                continue;
            }
            let infected = probed.infected.get(&target.id);
            assert!(
                infected.is_some_and(|seen| seen.contains(index)),
                "{} killed {} without the probe recording it infecting it; a discharge \
                 would then remove a test that finds a defect",
                target.id,
                mutant.display_id
            );
        }
    }
    session.close().expect("the session closes");
}
