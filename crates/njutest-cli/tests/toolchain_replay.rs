// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind: `report`, `trace`, `diagnostics`, and the one that says what a run would do without doing it, `plan`.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest_cli::cli::Environment;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-replay-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

fn njutest(fixture: &Fixture, args: &[&str]) -> Output {
    asked(&of(&fixture.root, &[]), args)
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

/// The environment a run of this suite composes: the four variables a toolchain needs, what a test named, and nothing else.
fn environment(root: &Path, cache: &Path, named: &[(&str, &str)]) -> Environment {
    let mut vars: Vec<(OsString, OsString)> =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[]);
    for (name, value) in named {
        vars.push((OsString::from(*name), OsString::from(*value)));
    }
    Environment {
        cache_directory: cache.to_path_buf(),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars,
        cancel: Cancel::new(),
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn survivor(fixture: &Fixture) -> String {
    let index = fixture.root.join(njutest_cli::app::reports::LATEST_ANY);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index).expect("the index")).expect("JSON");
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            fixture
                .root
                .join(value["directory"].as_str().expect("a directory"))
                .join(njutest_cli::app::reports::DOCUMENT_NAME),
        )
        .expect("the document"),
    )
    .expect("JSON");
    report["findings"]
        .as_array()
        .expect("the findings")
        .iter()
        .find(|finding| finding["kind"] == "surviving-mutant")
        .and_then(|finding| finding["subject"].as_str().map(ToOwned::to_owned))
        .expect("a mutation nothing noticed")
}

/// A fixture with one completed run behind it.
fn verified(name: &str) -> Fixture {
    let fixture = fixture(name);
    let output = njutest(&fixture, &["verify", "--offline", "--locked"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture
}

#[test]
fn replaying_a_finding_nothing_has_answered_reproduces_it() {
    let fixture = verified("fixture-baseline");
    let finding = survivor(&fixture);

    let output = njutest(&fixture, &["replay", &finding, "--offline", "--locked"]);

    let said = stdout(&output);
    assert!(
        said.contains("REPRODUCED"),
        "the mutation is still one nothing notices: {said} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(1), "{said}");
}

#[test]
fn replaying_a_finding_the_tests_now_answer_resolves_it() {
    let fixture = verified("fixture-baseline");
    let finding = survivor(&fixture);
    let mutant = njutest(&fixture, &["explain", &finding]);
    let explained = stdout(&mutant);
    assert!(explained.contains(&finding), "{explained}");

    std::fs::write(
        fixture.root.join("tests/every_sign.rs"),
        "// SPDX-FileCopyrightText: 2026 njutest contributors\n\
         // SPDX-License-Identifier: MIT OR Apache-2.0\n\n\
         //! Every sign, so nothing about `sign` goes unnoticed.\n\n\
         #[test]\n\
         fn every_sign_is_named() {\n\
         assert_eq!(fixture_baseline::sign(1), \"positive\");\n\
         assert_eq!(fixture_baseline::sign(-1), \"negative\");\n\
         assert_eq!(fixture_baseline::sign(0), \"zero\");\n\
         }\n",
    )
    .expect("a test that notices");

    let output = njutest(&fixture, &["replay", &finding, "--offline", "--locked"]);

    let said = stdout(&output);
    assert!(
        said.contains("RESOLVED"),
        "a test now notices the mutation the finding was about: {said} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0), "{said}");
}

#[test]
fn replaying_something_no_finding_names_says_so() {
    let fixture = verified("fixture-baseline");

    let output = njutest(&fixture, &["replay", "ffffffffffffffffffff"]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ffffffffffffffffffff"), "{stderr}");
}

/// The environment of a fixture, with the cache and the scratch beside its root.
fn of(root: &Path, named: &[(&str, &str)]) -> Environment {
    let cache = njutest_devkit::paths::cache_beside(root).expect("a cache directory");
    environment(root, &cache, named)
}
