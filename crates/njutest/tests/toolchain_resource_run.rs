// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run that holds an integration resource: what its tests see, and what its report says it held.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document as a table"
)]

use njutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest::cli::Environment;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join("fixture-assured");
    let dir = tempfile::Builder::new()
        .prefix("njutest-resource-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-assured");
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

/// The provider a run drives, as a program every platform can start.
fn provider(own: &Path) -> PathBuf {
    njutest_devkit::fake_cargo::example_in("fake_provider", own)
}

/// What the provider says when it is ready.
const READY: &str = r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"DATABASE_URL":"postgres://127.0.0.1/test"}}"#;

/// What it says when it cannot.
const UNWILLING: &str = r#"{"version":1,"status":"error","message":"no docker here"}"#;

/// What it says when it has stopped.
const STOPPED: &str = r#"{"version":1,"status":"stopped","instance":"pg-1"}"#;

fn verify(fixture: &Fixture, ready: &str) -> Output {
    asked(
        &of(
            &fixture.root,
            &[
                ("FAKE_PROVIDER_READY", ready),
                ("FAKE_PROVIDER_STOPPED", STOPPED),
            ],
        ),
        &["verify", "--offline", "--locked"],
    )
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
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
    let mut vars: rust_mutants::vars::Variables =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect::<rust_mutants::vars::Variables>();
    for (name, value) in named {
        vars.set(*name, *value);
    }
    Environment {
        cache_directory: cache.to_path_buf(),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let run = njutest::app::reports::pointed_at(&fixture.root, njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let path = fixture
        .root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(run.as_str())
        .join(njutest::app::reports::DOCUMENT_NAME);
    let whole: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&path).expect("the report"),
    )
    .expect("JSON");
    assert_eq!(whole["document_type"], "complete", "{whole}");
    whole["report"]["builds"][0]["parts"][0].clone()
}

fn declaring(fixture: &Fixture) {
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        format!(
            "version = 1\n\n[resources.postgres]\ncommand = [{:?}, \"resource\"]\n\
             timeout = \"10s\"\nenvironment = [\"FAKE_PROVIDER_READY\", \"FAKE_PROVIDER_STOPPED\"]\n",
            provider(fixture.root.parent().expect("the fixture's own directory")).to_str().expect("test protocol paths are UTF-8")
        ),
    )
    .expect("write");
}

/// A test that only passes when the resource told the run where the database is.
fn needing_the_resource(fixture: &Fixture) {
    std::fs::write(
        fixture.root.join("tests/leased.rs"),
        "#[test]\nfn the_resource_told_this_test_where_it_is() {\n    \
         assert_eq!(\n        std::env::var(\"DATABASE_URL\").ok().as_deref(),\n        \
         Some(\"postgres://127.0.0.1/test\")\n    );\n}\n",
    )
    .expect("write");
}

#[test]
fn a_test_of_a_leasing_run_sees_what_the_provider_said() {
    let fixture = fixture();
    declaring(&fixture);
    needing_the_resource(&fixture);

    let output = verify(&fixture, READY);
    let report = document(&fixture);
    assert_eq!(
        report["resources"],
        serde_json::json!([{
            "capability": "postgres",
            "instance": "pg-1",
            "environment": ["DATABASE_URL"]
        }]),
        "a run says what world its tests ran in"
    );
    let leased = report["targets"]
        .as_array()
        .expect("the targets")
        .iter()
        .find(|target| {
            target["name"]
                .as_str()
                .is_some_and(|name| name.contains("fixture-assured/test/leased"))
        })
        .cloned()
        .unwrap_or_else(|| panic!("the leasing test ran: {output:?}"));
    assert_eq!(
        leased["status"], "passed",
        "the test saw what the provider said: {leased}"
    );
}

#[test]
fn a_run_that_cannot_start_what_it_was_told_to_start_does_not_run_the_tests_without_it() {
    let fixture = fixture();
    declaring(&fixture);

    let output = verify(&fixture, UNWILLING);
    let complaint = njutest_devkit::process::strict_utf8(&output.stderr).into_owned();
    assert_eq!(
        output.status.code(),
        Some(3),
        "a world the configuration describes and the run could not make is an error: {complaint}"
    );
    assert!(complaint.contains("NJ5004"), "{complaint}");
    assert!(complaint.contains("no docker here"), "{complaint}");
}

/// The environment of a fixture, with the cache and the scratch beside its root.
fn of(root: &Path, named: &[(&str, &str)]) -> Environment {
    let cache = njutest_devkit::paths::cache_beside(root).expect("a cache directory");
    environment(root, &cache, named)
}
