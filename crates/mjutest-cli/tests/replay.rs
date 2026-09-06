// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind: `report`, `trace`, `diagnostics`, and the one that says what a run would do without doing it, `plan`.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("mjutest-replay-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
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

fn mjutest(fixture: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(args)
        .current_dir(&fixture.root)
        .env_clear()
        .env("NO_COLOR", "1")
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn survivor(fixture: &Fixture) -> String {
    let index = fixture.root.join(mjutest_cli::app::reports::LATEST_ANY);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index).expect("the index")).expect("JSON");
    let report: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            fixture
                .root
                .join(value["directory"].as_str().expect("a directory"))
                .join(mjutest_cli::app::reports::DOCUMENT_NAME),
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
    let output = mjutest(&fixture, &["verify", "--offline", "--locked"]);
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

    let output = mjutest(&fixture, &["replay", &finding, "--offline", "--locked"]);

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
    let mutant = mjutest(&fixture, &["explain", &finding]);
    let explained = stdout(&mutant);
    assert!(explained.contains(&finding), "{explained}");

    std::fs::write(
        fixture.root.join("tests/every_sign.rs"),
        "// SPDX-FileCopyrightText: 2026 mjutest contributors\n\
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

    let output = mjutest(&fixture, &["replay", &finding, "--offline", "--locked"]);

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

    let output = mjutest(&fixture, &["replay", "ffffffffffffffffffff"]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ffffffffffffffffffff"), "{stderr}");
}
