// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run that asks for a repair: what it is offered, what it puts to the tests, and what `fix --apply` writes.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document as a table"
)]

use njutest_devkit::fixture::copy_tree;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use njutest::cli::Environment;
use rust_mutants::runner::Cancel;

/// The test the provider offers: the one the `#[ignore]` left out.
const OFFERED: &str = "Ly8gU1BEWC1GaWxlQ29weXJpZ2h0VGV4dDogMjAyNiBtanV0ZXN0IGNvbnRyaWJ1dG9ycwovLyBTUERYLUxpY2Vuc2UtSWRlbnRpZmllcjogTUlUIE9SIEFwYWNoZS0yLjAKCi8vISBPZmZlcmVkIGJ5IGEgZ2VuZXJhdGlvbiBwcm92aWRlciB0byBjbG9zZSB0aGUgZ2FwIHRoZSBpZ25vcmVkIHRlc3QgbGVmdC4KCiNbdGVzdF0KZm4gemVyb19oYXNfYV9zaWduX29mX2l0c19vd24oKSB7CiAgICBhc3NlcnRfZXEhKGZpeHR1cmVfYmFzZWxpbmU6OnNpZ24oMCksICJ6ZXJvIik7Cn0K";

struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join("fixture-baseline");
    let dir = tempfile::Builder::new()
        .prefix("njutest-generation-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(&source, &root);
    Fixture { root, _dir: dir }
}

/// The generation provider a run asks, as a program every platform can start.
fn provider(own: &Path) -> PathBuf {
    njutest_devkit::fake_cargo::example_in("fake_provider", own)
}

fn declaring(fixture: &Fixture) {
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        format!(
            "version = 1\n\n[generation]\ncommand = [{:?}, \"generation\"]\n\
             environment = [\"FAKE_GENERATOR_OFFERS\"]\n",
            provider(fixture.root.parent().expect("the fixture's own directory"))
                .to_str()
                .expect("test protocol paths are UTF-8")
        ),
    )
    .expect("write");
}

fn njutest(fixture: &Fixture, args: &[&str], offers: &str) -> Output {
    asked(
        &of(&fixture.root, &[("FAKE_GENERATOR_OFFERS", offers)]),
        args,
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

    let verified = njutest(&fixture, &["verify", "--offline", "--locked"], &offers);
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

    let listed = njutest(&fixture, &["fix"], &offers);
    let said = njutest_devkit::process::strict_utf8(&listed.stdout).into_owned();
    assert!(said.contains("tests/zero.rs"), "{said}");
    assert!(said.contains("held up"), "{said}");
    assert!(
        !fixture.root.join("tests/zero.rs").exists(),
        "listing writes nothing"
    );

    let applied = njutest(
        &fixture,
        &["fix", "--apply", "--offline", "--locked"],
        &offers,
    );
    let said = njutest_devkit::process::strict_utf8(&applied.stdout).into_owned();
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

    let verified = njutest(&fixture, &["verify", "--offline", "--locked"], &offers);
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

    let applied = njutest(
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
    let cache = njutest_devkit::paths::cache_beside(root).expect("a cache directory");
    environment(root, &cache, named)
}

#[test]
fn a_generation_provider_that_cannot_be_asked_is_a_limitation_and_not_a_silence() {
    let fixture = fixture();
    std::fs::write(
        fixture.root.join(".njutest.toml"),
        "version = 1\n\n[generation]\ncommand = [\"/nonexistent/generator\"]\n",
    )
    .expect("write");

    let verified = njutest(&fixture, &["verify", "--offline", "--locked"], "");
    assert_eq!(
        verified.status.code(),
        Some(2),
        "the gap is still there, and a provider that could not answer closes nothing: \
         {verified:?}"
    );

    let report = document(&fixture);
    let named: Vec<&str> = report["limitations"]
        .as_array()
        .expect("a report says what it could not do")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        named.contains(&njutest::limitation::GENERATION_PROVIDER_UNAVAILABLE),
        "a person who configured a generator and got no candidates would read the report \
         as one where the generator had nothing to offer, which is the opposite of what \
         happened: {named:?}"
    );
    let detail = report["limitations"]
        .as_array()
        .and_then(|all| {
            all.iter()
                .find(|one| one["name"] == njutest::limitation::GENERATION_PROVIDER_UNAVAILABLE)
        })
        .and_then(|one| one["detail"].as_str())
        .unwrap_or_default();
    assert!(
        detail.contains("could not be asked"),
        "and the sentence says the provider was not asked rather than that it declined: \
         {detail}"
    );
    assert!(
        report["candidates"].as_array().is_none_or(Vec::is_empty),
        "and nothing was offered: {report}"
    );
}

#[test]
fn a_provider_whose_answer_is_not_one_is_a_limitation_that_names_the_finding() {
    let fixture = fixture();
    declaring(&fixture);

    let verified = njutest(
        &fixture,
        &["verify", "--offline", "--locked"],
        "not a document at all",
    );
    assert_eq!(verified.status.code(), Some(2), "{verified:?}");

    let report = document(&fixture);
    let detail: Vec<&str> = report["limitations"]
        .as_array()
        .expect("a report says what it could not do")
        .iter()
        .filter(|one| one["name"] == njutest::limitation::GENERATION_PROVIDER_UNAVAILABLE)
        .filter_map(|one| one["detail"].as_str())
        .collect();
    assert!(
        !detail.is_empty(),
        "an answer nobody could read is not an answer of no candidates: {report}"
    );
    assert!(
        detail.iter().any(|said| said.contains("was not read")),
        "and the sentence says which finding the unreadable answer was about, because a \
         provider that answers for one and not another is the usual case: {detail:?}"
    );
}

#[test]
fn a_digest_that_names_no_candidate_is_not_a_run_that_was_offered_none() {
    let fixture = fixture();
    declaring(&fixture);
    let offers = offering("tests/zero.rs", OFFERED);

    let verified = njutest(&fixture, &["verify", "--offline", "--locked"], &offers);
    assert_eq!(verified.status.code(), Some(2), "{verified:?}");
    let listed = njutest(&fixture, &["fix"], &offers);
    let said = njutest_devkit::process::strict_utf8(&listed.stdout).into_owned();
    let digest = said
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .expect("a candidate is listed with its digest")
        .to_owned();

    let one = njutest(&fixture, &["fix", "--candidate", &digest], &offers);
    assert_eq!(
        one.status.code(),
        Some(0),
        "a digest that names a candidate names it: {}",
        njutest_devkit::process::strict_utf8(&one.stderr)
    );
    assert!(
        njutest_devkit::process::strict_utf8(&one.stdout).contains(&digest),
        "{}",
        njutest_devkit::process::strict_utf8(&one.stdout)
    );

    let nothing = njutest(&fixture, &["fix", "--candidate", "ffffffffffff"], &offers);
    assert_ne!(
        nothing.status.code(),
        Some(0),
        "a digest that names none of the candidates a run was offered is not a run that \
         was offered none: a person who mistyped a digest would read that the run had \
         nothing to propose, and a script would read success: {}",
        njutest_devkit::process::strict_utf8(&nothing.stdout)
    );
    let refusal = njutest_devkit::process::strict_utf8(&nothing.stderr).into_owned();
    assert!(
        refusal.contains("ffffffffffff") && refusal.contains("no candidate"),
        "and the refusal says which digest found nothing: {refusal}"
    );
    assert!(
        refusal.contains(" 1") || refusal.contains("offered"),
        "and how many there were to choose from, because that is what tells a person \
         whether to look again or to stop looking: {refusal}"
    );
}
