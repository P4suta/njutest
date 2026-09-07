// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run costs, counted in pairs, held to a ceiling that may fall and never rise.
//!
//! A pair is one mutant asked of one target: one test process started. It is
//! the same number on every machine, at every job count, under every load,
//! which is what makes it a thing a gate can hold. A duration is not.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and reads a ledger as a table"
)]

use std::process::Command;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::work::Work;

/// One line of the ceiling: a fixture, what a whole run would start, and what this engine starts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ceiling {
    fixture: String,
    whole: u64,
    started: u64,
}

fn ceilings() -> Vec<Ceiling> {
    let path = mjutest_devkit::paths::workspace_root().join("xtask/work_ceiling.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(
                fields.len(),
                3,
                "a ceiling is a fixture and two counts: {line}"
            );
            Ceiling {
                fixture: fields[0].to_owned(),
                whole: fields[1].parse().expect("a count"),
                started: fields[2].parse().expect("a count"),
            }
        })
        .collect()
}

fn measured(name: &str) -> Work {
    let fixture = Fixture::copy(name);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-mutants"))
        .env("NO_COLOR", "1")
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .args(["run", "--tier", "all", "--offline", "--locked"])
        .args(["--jobs", "1", "--ui", "quiet"])
        .args(["--root", &fixture.root().to_string_lossy()])
        .output()
        .expect("rust-mutants runs");
    assert!(
        output.status.code().is_some_and(|code| code <= 1),
        "{name}: {output:?}"
    );
    let directory = std::fs::read_dir(fixture.root().join("reports/mutation"))
        .expect("the run stored a report")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.join("run-report-v1.json").is_file())
        .expect("a stored run");
    let text = std::fs::read_to_string(directory.join("run-report-v1.json")).expect("the report");
    let document: rust_mutants::report::run::RunDocument =
        serde_json::from_str(&text).expect("the report reads back");
    Work::of(&document)
}

#[test]
fn no_fixture_starts_more_processes_than_the_ceiling_allows() {
    let mut risen = Vec::new();
    let mut fallen = Vec::new();
    for ceiling in ceilings() {
        let work = measured(&ceiling.fixture);
        assert!(
            work.balances(),
            "{}: a pair nothing accounts for is work nobody can explain: {work:?}",
            ceiling.fixture
        );
        assert_eq!(
            work.whole, ceiling.whole,
            "{}: what a whole run would start changed; if that is right, the ceiling says so too",
            ceiling.fixture
        );
        if work.started > ceiling.started {
            risen.push(format!(
                "{}: {} started, {} allowed",
                ceiling.fixture, work.started, ceiling.started
            ));
        }
        if work.started < ceiling.started {
            fallen.push(format!(
                "{}: {} started, {} allowed",
                ceiling.fixture, work.started, ceiling.started
            ));
        }
    }
    assert!(
        risen.is_empty(),
        "the engine started more processes than it used to for the same question. That is a \
         change to argue for, not one to notice later:\n{}",
        risen.join("\n")
    );
    assert!(
        fallen.is_empty(),
        "the engine starts fewer processes than the ceiling allows, which is the point — lower \
         xtask/work_ceiling.txt to what it is now, so it can never rise back:\n{}",
        fallen.join("\n")
    );
}

#[test]
fn every_removal_a_whole_run_still_answers_for_is_a_proof_a_reader_can_name() {
    let work = measured("fixture-unreached");
    assert!(
        work.answers_for_the_whole(),
        "nothing here was filtered, so this run answers for the whole catalog: {work:?}"
    );
    assert!(
        work.saved() > 0.5,
        "a fixture built to hold code no test reaches should cost less than half a whole run: \
         {work:?}"
    );
    for removed in &work.removed {
        assert!(
            !removed.reason.is_empty() && removed.pairs > 0,
            "{removed:?}"
        );
    }
}
