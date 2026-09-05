// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run that asks for a repair: what it is offered, what it puts to the tests, and what `fix --apply` writes.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document as a table"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The test the provider offers: the one the `#[ignore]` left out.
const OFFERED: &str = "Ly8gU1BEWC1GaWxlQ29weXJpZ2h0VGV4dDogMjAyNiBtanV0ZXN0IGNvbnRyaWJ1dG9ycwovLyBTUERYLUxpY2Vuc2UtSWRlbnRpZmllcjogTUlUIE9SIEFwYWNoZS0yLjAKCi8vISBPZmZlcmVkIGJ5IGEgZ2VuZXJhdGlvbiBwcm92aWRlciB0byBjbG9zZSB0aGUgZ2FwIHRoZSBpZ25vcmVkIHRlc3QgbGVmdC4KCiNbdGVzdF0KZm4gemVyb19oYXNfYV9zaWduX29mX2l0c19vd24oKSB7CiAgICBhc3NlcnRfZXEhKGZpeHR1cmVfYmFzZWxpbmU6OnNpZ24oMCksICJ6ZXJvIik7Cn0K";

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join("fixture-baseline");
    let dir = tempfile::Builder::new()
        .prefix("mjutest-generation-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
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

fn provider(root: &Path, offering: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let path = root.join("generate.sh");
    std::fs::write(
        &path,
        format!("#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{offering}'\n"),
    )
    .expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn declaring(fixture: &Fixture, command: &Path) {
    std::fs::write(
        fixture.root.join(".mjutest.toml"),
        format!(
            "version = 1\n\n[generation]\ncommand = [{:?}]\n",
            command.to_string_lossy()
        ),
    )
    .expect("write");
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

fn offering(path: &str, content: &str) -> String {
    format!(
        r#"{{"version":1,"candidates":[{{"kind":"patch","path":"{path}","preimage_sha256":null,"content_base64":"{content}"}}]}}"#
    )
}

#[test]
fn a_candidate_that_closes_the_gap_is_offered_and_only_then_written() {
    let fixture = fixture();
    let command = provider(&fixture.root, &offering("tests/zero.rs", OFFERED));
    declaring(&fixture, &command);

    let verified = mjutest(&fixture, &["verify", "--offline", "--locked"]);
    assert_eq!(
        verified.status.code(),
        Some(2),
        "the gap is still there until somebody writes the test: {verified:?}"
    );
    let report = document(&fixture);
    let candidates = report["candidates"].as_array().expect("the candidates");
    assert!(!candidates.is_empty(), "{report}");
    for candidate in candidates {
        assert_eq!(candidate["path"], "tests/zero.rs", "{candidate}");
        assert_eq!(
            candidate["accepted"], true,
            "a candidate that closes the gap holds up: {candidate}"
        );
        assert_eq!(candidate["stability_runs"], 3, "{candidate}");
        assert_eq!(candidate["kill_runs"], 2, "{candidate}");
    }
    assert!(
        !fixture.root.join("tests/zero.rs").exists(),
        "a candidate is a proposal until somebody applies it"
    );

    let listed = mjutest(&fixture, &["fix"]);
    let said = String::from_utf8_lossy(&listed.stdout).into_owned();
    assert!(said.contains("tests/zero.rs"), "{said}");
    assert!(said.contains("held up"), "{said}");
    assert!(
        !fixture.root.join("tests/zero.rs").exists(),
        "listing writes nothing"
    );

    let applied = mjutest(&fixture, &["fix", "--apply", "--offline", "--locked"]);
    let said = String::from_utf8_lossy(&applied.stdout).into_owned();
    assert_eq!(applied.status.code(), Some(0), "{applied:?}");
    assert!(said.contains("wrote tests/zero.rs"), "{said}");
    assert!(
        said.contains("already what the candidate would write") || said.contains("1 written"),
        "the same repair offered for two findings is written once: {said}"
    );
    let written = std::fs::read_to_string(fixture.root.join("tests/zero.rs")).expect("the test");
    assert!(written.contains("zero_has_a_sign_of_its_own"), "{written}");
}

#[test]
fn a_candidate_that_does_not_close_the_gap_is_recorded_and_not_offered() {
    let fixture = fixture();
    let useless = "Ly8hIEEgdGVzdCB0aGF0IHJ1bnMgYW5kIHBhc3NlcyBhbmQgdGVsbHMgbm90aGluZyBhcGFydC4KCiNbdGVzdF0KZm4gcG9zaXRpdmVfaXNfcG9zaXRpdmUoKSB7CiAgICBhc3NlcnRfZXEhKGZpeHR1cmVfYmFzZWxpbmU6OnNpZ24oMSksICJwb3NpdGl2ZSIpOwp9Cg==";
    let command = provider(&fixture.root, &offering("tests/useless.rs", useless));
    declaring(&fixture, &command);

    let verified = mjutest(&fixture, &["verify", "--offline", "--locked"]);
    assert_eq!(verified.status.code(), Some(2), "{verified:?}");
    let report = document(&fixture);
    let candidates = report["candidates"].as_array().expect("the candidates");
    assert!(!candidates.is_empty(), "{report}");
    for candidate in candidates {
        assert_eq!(candidate["accepted"], false, "{candidate}");
        assert!(
            candidate["why"]
                .as_str()
                .is_some_and(|why| why.contains("does not notice the mutant")),
            "{candidate}"
        );
    }

    let applied = mjutest(&fixture, &["fix", "--apply", "--offline", "--locked"]);
    assert!(
        !fixture.root.join("tests/useless.rs").exists(),
        "nothing that did not hold up is written: {applied:?}"
    );
}
