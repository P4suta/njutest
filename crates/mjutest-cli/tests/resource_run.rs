// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run that holds an integration resource: what its tests see, and what its report says it held.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document as a table"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join("fixture-assured");
    let dir = tempfile::Builder::new()
        .prefix("mjutest-resource-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-assured");
    copy(&source, &root);
    Fixture { root, _dir: dir }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the directory");
    for entry in std::fs::read_dir(from).expect("the fixture") {
        let entry = entry.expect("an entry");
        let kind = entry.file_type().expect("a file type");
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy(&entry.path(), &target);
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target).expect("a copy");
        }
    }
}

/// The provider a run drives, read rather than written: see the script's own note.
fn provider() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/fake-provider.sh")
}

/// What the provider says when it is ready.
const READY: &str = r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"DATABASE_URL":"postgres://127.0.0.1/test"}}"#;

/// What it says when it cannot.
const UNWILLING: &str = r#"{"version":1,"status":"error","message":"no docker here"}"#;

/// What it says when it has stopped.
const STOPPED: &str = r#"{"version":1,"status":"stopped","instance":"pg-1"}"#;

fn verify(fixture: &Fixture, ready: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(["verify", "--offline", "--locked"])
        .current_dir(&fixture.root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env("FAKE_PROVIDER_READY", ready)
        .env("FAKE_PROVIDER_STOPPED", STOPPED)
        .env(
            "XDG_CACHE_HOME",
            mjutest_devkit::paths::cache_beside(&fixture.root).expect("a cache directory"),
        )
        .env(
            "TMPDIR",
            mjutest_devkit::paths::temp_beside(&fixture.root).expect("a temporary directory"),
        )
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        }))
        .output()
        .expect("mjutest runs")
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let index = fixture.root.join(mjutest_cli::app::reports::LATEST_ANY);
    let text = std::fs::read_to_string(&index).expect("the latest index");
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let directory = value["directory"].as_str().expect("a directory");
    let path = fixture
        .root
        .join(directory)
        .join(mjutest_cli::app::reports::DOCUMENT_NAME);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the report")).expect("JSON")
}

fn declaring(fixture: &Fixture) {
    std::fs::write(
        fixture.root.join(".mjutest.toml"),
        format!(
            "version = 1\n\n[resources.postgres]\ncommand = [\"/bin/sh\", {:?}]\n\
             timeout = \"10s\"\nenvironment = [\"FAKE_PROVIDER_READY\", \"FAKE_PROVIDER_STOPPED\"]\n",
            provider().to_string_lossy()
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
                .is_some_and(|name| name.contains("the_resource_told_this_test_where_it_is"))
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
    let complaint = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(
        output.status.code(),
        Some(3),
        "a world the configuration describes and the run could not make is an error: {complaint}"
    );
    assert!(complaint.contains("MJ5004"), "{complaint}");
    assert!(complaint.contains("no docker here"), "{complaint}");
}
