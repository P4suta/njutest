// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run costs, counted in pairs, held to a ceiling that may fall and never rise.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and reads a ledger as a table"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants::work::Work;
use rust_mutants_cli::{Environment, Streams};

include!("support/directory.rs");

/// One line of the ceiling: a fixture, and what a whole run and this engine start of each unit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ceiling {
    fixture: String,
    whole: u64,
    started: u64,
    tests_whole: u64,
    tests_started: u64,
}

fn ceilings() -> Vec<Ceiling> {
    let path = njutest_devkit::paths::workspace_root().join("xtask/work_ceiling.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(
                fields.len(),
                5,
                "a ceiling is a fixture and four counts: {line}"
            );
            Ceiling {
                fixture: fields[0].to_owned(),
                whole: fields[1].parse::<u64>().expect("a count"),
                started: fields[2].parse::<u64>().expect("a count"),
                tests_whole: fields[3].parse::<u64>().expect("a count"),
                tests_started: fields[4].parse::<u64>().expect("a count"),
            }
        })
        .collect()
}

fn measured(name: &str) -> Work {
    let fixture = Fixture::copy(name);
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run", "--tier", "all", "--offline", "--locked"])
            .chain(["--jobs", "1", "--ui", "quiet"])
            .chain(["--root", root.as_str()])
            .map(OsString::from),
        &environment(&fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    assert!(
        output.status.code().is_some_and(|code| code <= 1),
        "{name}: {output:?}"
    );
    let directory = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    );
    let text = std::fs::read_to_string(directory.join("run-report-v1.json")).expect("the report");
    let document: rust_mutants::report::run::RunDocument =
        njutest_devkit::strictjson::decode_str(&text).expect("the report reads back");
    Work::of(&document).expect("valid work ledger")
}

#[test]
fn no_fixture_starts_more_processes_than_the_ceiling_allows() {
    let mut risen = Vec::new();
    let mut fallen = Vec::new();
    let mut asked = Vec::new();
    let mut measurements = Vec::new();
    for ceiling in ceilings() {
        let work = measured(&ceiling.fixture);
        assert!(
            work.balances().expect("exact balance arithmetic"),
            "{}: a pair nothing accounts for is work nobody can explain: {work:?}",
            ceiling.fixture
        );
        measurements.push(format!(
            "{:<20} {:>5} {:>8} {:>6} {:>14}",
            ceiling.fixture, work.whole, work.started, work.tests_whole, work.tests_started
        ));
        if (work.whole, work.tests_whole) != (ceiling.whole, ceiling.tests_whole) {
            asked.push(ceiling.fixture.clone());
        }
        for (unit, started, allowed) in [
            ("pairs", work.started, ceiling.started),
            ("tests", work.tests_started, ceiling.tests_started),
        ] {
            let said = format!(
                "{}: {started} {unit} started, {allowed} allowed",
                ceiling.fixture
            );
            if started > allowed {
                risen.push(said);
            } else if started < allowed {
                fallen.push(said);
            }
        }
    }
    assert!(
        asked.is_empty(),
        "what a whole run would start changed for {asked:?}. A fixture that gained code asks a \
         bigger question and a rule that was added asks more of the same one; either way the \
         ceiling states it rather than following it. Every row as it measures now, for the file:\n\
         # fixture             whole  started  tests  tests_started\n{}",
        measurements.join("\n")
    );
    assert!(
        risen.is_empty(),
        "the engine started more of something than it used to for the same question. That is a \
         change to argue for, not one to notice later:\n{}",
        risen.join("\n")
    );
    assert!(
        fallen.is_empty(),
        "the engine starts fewer than the ceiling allows, which is the point — lower \
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
        work.saved().expect("exact skipped count") > 0.5,
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

/// How many times a run may start cargo before somebody has to say why.
const CARGO_CEILING: u64 = 6;

/// How many times a run started each program, read back from its own recording.
fn programs(name: &str, extra: &[&str]) -> std::collections::BTreeMap<String, u64> {
    let fixture = Fixture::copy(name);
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run", "--tier", "all", "--offline", "--locked"])
            .chain(["--jobs", "1", "--ui", "quiet", "--trace"])
            .chain(extra.iter().copied())
            .chain(["--root", root.as_str()])
            .map(OsString::from),
        &environment(&fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    assert!(
        output.status.code().is_some_and(|code| code <= 1),
        "{name}: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let directory =
        std::fs::read_dir(rust_mutants_cli::app::stored::Store::read(fixture.root()).root())
            .expect("the run stored a report")
            .map(|entry| entry.expect("stored run directory entry"))
            .map(|entry| entry.path())
            .find(|path| test_directory(&path.join("trace")))
            .expect("a recording");
    let text = std::fs::read_to_string(directory.join("trace").join("trace.jsonl"))
        .expect("the recording");
    let events = rust_mutants::trace::read_events(text.as_bytes()).expect("it reads back");
    match rust_mutants::trace::summary::summarize(&events, 1) {
        Ok(summary) => summary.invocations,
        Err(error) => panic!("the fixture trace must summarize exactly: {error}"),
    }
}

#[test]
fn a_run_starts_no_more_compilers_than_the_ceiling_allows() {
    let started = programs("fixture-simple", &[]);
    let cargo = started.get("cargo").copied().unwrap_or_default();
    assert!(
        cargo <= CARGO_CEILING,
        "a run started cargo {cargo} times and {CARGO_CEILING} is what it used to take. Each one \
         is a compilation of the tree; if the extra one is worth it, raise the ceiling and say \
         why: {started:?}"
    );
    assert!(cargo >= 2, "a run has to ask cargo something: {started:?}");
}

#[test]
fn a_second_run_of_a_tree_nothing_changed_measures_it_again_no_harder_than_the_first() {
    let first = programs("fixture-coverage", &[]);
    let cargo = first.get("cargo").copied().unwrap_or_default();
    assert!(
        cargo <= CARGO_CEILING.saturating_add(1),
        "measuring coverage costs one compilation more than not measuring it: {first:?}"
    );
}

#[test]
fn a_second_run_of_a_tree_nothing_changed_measures_nothing_again() {
    let fixture = Fixture::copy("fixture-coverage");
    let started = |fixture: &Fixture| -> u64 {
        let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = rust_mutants_cli::run_from(
            std::iter::once("rust-mutants")
                .chain([
                    "run",
                    "--tier",
                    "all",
                    "--offline",
                    "--locked",
                    "--coverage",
                ])
                .chain(["--jobs", "1", "--ui", "quiet", "--trace"])
                .chain(["--root", root.as_str()])
                .map(OsString::from),
            &environment(fixture),
            &Cancel::new(),
            Streams {
                out: &mut out,
                err: &mut err,
            },
        );
        let output = njutest_devkit::process::answered(code, out, err);
        assert!(
            output.status.code().is_some_and(|code| code <= 1),
            "{}",
            njutest_devkit::process::strict_utf8(&output.stderr)
        );
        let directory =
            std::fs::read_dir(rust_mutants_cli::app::stored::Store::read(fixture.root()).root())
                .expect("the run stored a report")
                .map(|entry| entry.expect("stored run directory entry"))
                .map(|entry| entry.path())
                .filter(|path| test_directory(&path.join("trace")))
                .max()
                .expect("the newest recording");
        let text = std::fs::read_to_string(directory.join("trace").join("trace.jsonl"))
            .expect("the recording");
        let events =
            rust_mutants::trace::read_events(text.as_bytes()).expect("the recording reads back");
        let summary = match rust_mutants::trace::summary::summarize(&events, 1) {
            Ok(summary) => summary,
            Err(error) => panic!("the fixture trace must summarize exactly: {error}"),
        };
        summary
            .invocations
            .get("cargo")
            .copied()
            .unwrap_or_default()
    };
    let first = started(&fixture);
    let again = started(&fixture);
    assert!(
        again < first,
        "measuring a tree is a function of the tree, and nothing about it changed, so the second \\
         run should not have instrumented and rebuilt the whole graph to learn the same thing: \\
         {first} compilations then {again}"
    );
}

#[test]
fn a_tree_that_changed_is_measured_again_rather_than_remembered() {
    let fixture = Fixture::copy("fixture-coverage");
    let run = |fixture: &Fixture| -> u64 {
        let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = rust_mutants_cli::run_from(
            std::iter::once("rust-mutants")
                .chain([
                    "run",
                    "--tier",
                    "all",
                    "--offline",
                    "--locked",
                    "--coverage",
                ])
                .chain(["--jobs", "1", "--ui", "quiet", "--trace"])
                .chain(["--root", root.as_str()])
                .map(OsString::from),
            &environment(fixture),
            &Cancel::new(),
            Streams {
                out: &mut out,
                err: &mut err,
            },
        );
        let output = njutest_devkit::process::answered(code, out, err);
        assert!(
            output.status.code().is_some_and(|code| code <= 1),
            "{}",
            njutest_devkit::process::strict_utf8(&output.stderr)
        );
        let directory =
            std::fs::read_dir(rust_mutants_cli::app::stored::Store::read(fixture.root()).root())
                .expect("the run stored a report")
                .map(|entry| entry.expect("stored run directory entry"))
                .map(|entry| entry.path())
                .filter(|path| test_directory(&path.join("trace")))
                .max()
                .expect("the newest recording");
        let text = std::fs::read_to_string(directory.join("trace").join("trace.jsonl"))
            .expect("the recording");
        let events =
            rust_mutants::trace::read_events(text.as_bytes()).expect("the recording reads back");
        let summary = match rust_mutants::trace::summary::summarize(&events, 1) {
            Ok(summary) => summary,
            Err(error) => panic!("the fixture trace must summarize exactly: {error}"),
        };
        summary
            .invocations
            .get("cargo")
            .copied()
            .unwrap_or_default()
    };
    let first = run(&fixture);
    let remembered = run(&fixture);
    assert!(remembered < first, "{first} then {remembered}");

    let path = fixture.root().join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the library");
    std::fs::write(&path, format!("{source}\npub const CHANGED: u8 = 1;\n"))
        .expect("the library changes");
    let after = run(&fixture);
    assert_eq!(
        after, first,
        "a measurement is only the same measurement while the tree is the same tree; this one \
         changed and was remembered anyway"
    );
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    }
}
