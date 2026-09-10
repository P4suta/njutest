// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run that asks for a repair: what it is offered, what it puts to the tests, and what `fix --apply` writes.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document as a table"
)]

use mjutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use mjutest_cli::cli::Environment;
use rust_mutants::runner::Cancel;

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
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

/// The generation provider a run asks, read rather than written: see the script's own note.
fn provider() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/fake-generator.sh")
}

fn declaring(fixture: &Fixture) {
    std::fs::write(
        fixture.root.join(".mjutest.toml"),
        format!(
            "version = 1\n\n[generation]\ncommand = [\"/bin/sh\", {:?}]\n\
             environment = [\"FAKE_GENERATOR_OFFERS\"]\n",
            provider().to_string_lossy()
        ),
    )
    .expect("write");
}

fn mjutest(fixture: &Fixture, args: &[&str], offers: &str) -> Output {
    asked(
        &of(&fixture.root, &[("FAKE_GENERATOR_OFFERS", offers)]),
        args,
    )
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = mjutest_cli::run_from(
        std::iter::once("mjutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    mjutest_devkit::process::answered(code, out, err)
}

/// The environment a run of this suite composes: the four variables a toolchain needs, what a test named, and nothing else.
fn environment(root: &Path, cache: &Path, named: &[(&str, &str)]) -> Environment {
    let mut vars: Vec<(OsString, OsString)> = mjutest_devkit::paths::environment_for_a_run()
        .into_iter()
        .filter(|(name, _)| {
            matches!(
                name.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME"
            )
        })
        .collect();
    for (name, value) in named {
        vars.push((OsString::from(*name), OsString::from(*value)));
    }
    Environment {
        cache_directory: cache.to_path_buf(),
        working_directory: root.to_path_buf(),
        temp_directory: mjutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        vars,
        cancel: Cancel::new(),
    }
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
    declaring(&fixture);
    let offers = offering("tests/zero.rs", OFFERED);

    let verified = mjutest(&fixture, &["verify", "--offline", "--locked"], &offers);
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

    let listed = mjutest(&fixture, &["fix"], &offers);
    let said = String::from_utf8_lossy(&listed.stdout).into_owned();
    assert!(said.contains("tests/zero.rs"), "{said}");
    assert!(said.contains("held up"), "{said}");
    assert!(
        !fixture.root.join("tests/zero.rs").exists(),
        "listing writes nothing"
    );

    let applied = mjutest(
        &fixture,
        &["fix", "--apply", "--offline", "--locked"],
        &offers,
    );
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
    declaring(&fixture);
    let offers = offering("tests/useless.rs", useless);

    let verified = mjutest(&fixture, &["verify", "--offline", "--locked"], &offers);
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

    let applied = mjutest(
        &fixture,
        &["fix", "--apply", "--offline", "--locked"],
        &offers,
    );
    assert!(
        !fixture.root.join("tests/useless.rs").exists(),
        "nothing that did not hold up is written: {applied:?}"
    );
}

/// The environment of a fixture, with the cache and the scratch beside its root.
fn of(root: &Path, named: &[(&str, &str)]) -> Environment {
    let cache = mjutest_devkit::paths::cache_beside(root).expect("a cache directory");
    environment(root, &cache, named)
}
