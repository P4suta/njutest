// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run asked for faults fails the calls a `?` asks about, and says which failures the suite noticed.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a published report as a table"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest::cli::Environment;
use njutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-faults-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

fn verify(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["verify", "--offline", "--locked"];
    args.extend_from_slice(extra);
    asked(fixture, &args)
}

fn asked(fixture: &Fixture, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(&fixture.root),
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

fn environment(root: &Path) -> Environment {
    Environment {
        cache_directory: root.join(".cache"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[]),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn part(fixture: &Fixture) -> serde_json::Value {
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let path = fixture
        .root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(run.as_str())
        .join(njutest::app::reports::DOCUMENT_NAME);
    let text = std::fs::read_to_string(path).expect("the document");
    let whole: serde_json::Value = njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    whole["report"]["builds"][0]["parts"][0].clone()
}

fn decisions(part: &serde_json::Value) -> Vec<(u64, String)> {
    let mut decided: Vec<(u64, String)> = part["faults"]
        .as_array()
        .expect("a list of faults")
        .iter()
        .map(|fault| {
            (
                fault["position"]["line"]
                    .as_u64()
                    .expect("every site, put or not, says where it is"),
                fault["decision"]["decision"]
                    .as_str()
                    .expect("a decision")
                    .to_owned(),
            )
        })
        .collect();
    decided.sort();
    decided
}

fn named<'a>(
    part: &'a serde_json::Value,
    list: &str,
    field: &str,
    name: &str,
) -> Vec<&'a serde_json::Value> {
    part[list]
        .as_array()
        .expect("a list")
        .iter()
        .filter(|one| one[field] == name)
        .collect()
}

#[test]
fn a_run_asked_for_faults_says_which_failed_calls_the_suite_noticed() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![
            (13, "noticed".to_owned()),
            (22, "noticed".to_owned()),
            (33, "unnoticed".to_owned()),
            (46, "not-put".to_owned()),
            (57, "not-put".to_owned()),
        ],
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let unnoticed = named(&part, "findings", "kind", "unnoticed-fault");
    assert_eq!(
        unnoticed.len(),
        1,
        "one finding names the failure nothing noticed: {part}"
    );
    assert_eq!(unnoticed[0]["path"], "src/lib.rs", "{part}");
    assert!(
        njutest_devkit::process::strict_utf8(&output.stdout).contains(
            "FAULTS\tsites=5\tnoticed=2\tunnoticed=1\tunreached=0\twaited=0\tundecided=0\tnot_put=2"
        ),
        "the run says what the faults came to where it says what the mutations did: {}",
        njutest_devkit::process::strict_utf8(&output.stdout)
    );
    assert_eq!(
        named(&part, "limitations", "name", "fault-not-put").len(),
        1,
        "the two faults no run could put are stated once, as one class: {part}"
    );
    assert_eq!(
        part["accounting"]["faults"],
        serde_json::json!({
            "sites": 5, "noticed": 2, "unnoticed": 1, "unreached": 0,
            "waited": 0, "undecided": 0, "not_put": 2
        }),
        "{part}"
    );
}

#[test]
fn a_run_not_asked_for_faults_puts_none() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &[]);
    let part = part(&fixture);
    assert!(
        !njutest_devkit::process::strict_utf8(&output.stdout).contains("FAULTS"),
        "a run that put no fault says nothing about faults"
    );
    assert_eq!(
        part["faults"],
        serde_json::json!([]),
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        named(&part, "findings", "kind", "unnoticed-fault").is_empty(),
        "no fault was put, so none went unnoticed: {part}"
    );
}

#[test]
fn a_write_under_faults_no_execution_is_tied_to_is_not_measured() {
    let fixture = fixture("fixture-faulted-writes");
    let output = verify(&fixture, &["--faults"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![(13, "unnoticed".to_owned())],
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        named(&part, "findings", "kind", "broken-under-fault").is_empty(),
        "the faulted executions share one tree and run at once, so a write is not yet tied to \
         the failed call that made it, and a DEFECT nobody can attribute is a verdict about how \
         the run measured: {part}"
    );
    let unattributed: Vec<&serde_json::Value> = named(&part, "findings", "kind", "not-measured")
        .into_iter()
        .filter(|finding| finding["subject"] == "fault-write-unattributed")
        .collect();
    assert_eq!(unattributed.len(), 1, "{part}");
    let detail = unattributed[0]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("failed-read.log") && !detail.contains("always.log"),
        "the finding names what was written under a fault and not what every run writes: {detail}"
    );
    assert_ne!(
        output.status.code(),
        Some(njutest::cli::EXIT_DEFECT.into()),
        "and nothing the run could not attribute decides it: {part}"
    );
}

#[test]
fn why_follows_a_fault_from_every_target_it_was_put_to_to_what_it_came_to() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--faults", "--trace"]);
    let part = part(&fixture);
    let unnoticed = part["faults"]
        .as_array()
        .expect("a list of faults")
        .iter()
        .find(|fault| fault["decision"]["decision"] == "unnoticed")
        .and_then(|fault| fault["display_id"].as_str())
        .unwrap_or_else(|| {
            panic!(
                "the run holds the unnoticed fault: {part}\n{}",
                njutest_devkit::process::strict_utf8(&output.stderr)
            )
        })
        .to_owned();
    let why = asked(&fixture, &["why", "fault", &unnoticed]);
    let page = njutest_devkit::process::strict_utf8(&why.stdout);
    assert!(
        page.contains("every test that reached it passed with the call failing")
            && page.contains("asked fixture-faulted/test/calls  survived"),
        "the page names every target the fault was put to and what the run concluded: {page}\n{}",
        njutest_devkit::process::strict_utf8(&why.stderr)
    );
}

#[test]
fn a_tree_every_run_writes_into_is_not_broken_by_a_fault() {
    let fixture = fixture("fixture-writes-tree");
    let output = verify(&fixture, &["--faults"]);
    let part = part(&fixture);
    assert!(
        named(&part, "findings", "kind", "broken-under-fault").is_empty(),
        "the tree was written with no fault in place, so no fault wrote it: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
}
