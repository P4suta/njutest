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
        .prefix("njutest-crashes-")
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
    let mut decided: Vec<(u64, String)> = part["crashes"]
        .as_array()
        .expect("a list of crashes")
        .iter()
        .map(|crash| {
            (
                crash["position"]["line"]
                    .as_u64()
                    .expect("every site, put or not, says where it is"),
                crash["decision"]["decision"]
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
fn a_count_written_in_pieces_is_torn_by_a_stop_and_one_moved_into_place_is_not() {
    let fixture = fixture("fixture-durable");
    let output = verify(&fixture, &["--crashes"]);
    let part = part(&fixture);
    assert_eq!(
        decisions(&part),
        vec![
            (31, "corrupt".to_owned()),
            (32, "corrupt".to_owned()),
            (33, "restarted".to_owned()),
            (43, "restarted".to_owned()),
            (44, "restarted".to_owned()),
        ],
        "a truncated file or a bare `count=` is one the next run cannot read, and a whole count \
         or one moved into place always is: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert_eq!(
        named(&part, "findings", "kind", "corrupt-after-crash").len(),
        2,
        "each torn write is a finding at its call: {part}"
    );
    assert_eq!(
        output.status.code(),
        Some(njutest::cli::EXIT_DEFECT.into()),
        "a program that cannot start over what it wrote has a defect: {part}"
    );
}

#[test]
fn a_run_not_asked_for_crashes_stops_nothing() {
    let fixture = fixture("fixture-durable");
    let output = verify(&fixture, &[]);
    let part = part(&fixture);
    assert_eq!(
        part["crashes"],
        serde_json::json!([]),
        "{part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
}

#[test]
fn a_tree_that_writes_nothing_has_nothing_to_stop_after() {
    let fixture = fixture("fixture-faulted");
    let output = verify(&fixture, &["--crashes"]);
    let part = part(&fixture);
    assert_eq!(
        named(&part, "limitations", "name", "crash-no-site").len(),
        1,
        "no measured file of it calls anything that writes: {part}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
}
