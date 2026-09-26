// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest verify` with every knob asked for, end to end: each knob raises its finding about the one target it breaks, and nothing else the run establishes moves.
//!
//! Every test here reads a published report, which only a unix store publishes.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document by the names its writer put there"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest::cli::Environment;
use njutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;

/// Every knob, as the configuration names it.
const EVERY_KNOB: &str =
    r#"knobs = ["timezone", "locale", "temp-directory", "home", "umask", "columns", "threads"]"#;

/// A copy of `fixture-environment`, asking for every knob or for none, and the report of one verification of it, as written and as read back.
fn verified(knobs: bool) -> (serde_json::Value, njutest::report::Report) {
    let dir = tempfile::Builder::new()
        .prefix("njutest-repeatable-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-environment");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-environment"),
        &root,
    );
    if knobs {
        std::fs::write(
            root.join(".njutest.toml"),
            format!("version = 1\n\n[repeatable]\n{EVERY_KNOB}\n"),
        )
        .expect("the configuration");
    }
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--trace"]
            .into_iter()
            .map(OsString::from),
        &environment(&root),
        &mut out,
        &mut err,
    );
    let said = njutest_devkit::process::answered(code, out, err);
    assert!(
        said.status.code().is_some_and(|code| code <= 2),
        "the run itself did not fail: {}",
        njutest_devkit::process::strict_utf8(&said.stderr)
    );
    let text = std::fs::read_to_string(latest(&root)).expect("the published report");
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("the report is JSON");
    let read = njutest::report::json::parse(&text).expect("the report reads back");
    (document["report"].clone(), read)
}

fn environment(root: &Path) -> Environment {
    Environment {
        cache_directory: njutest_devkit::paths::cache_beside(root).expect("a cache directory"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect::<rust_mutants::vars::Variables>(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

/// Where the latest run wrote its report.
fn latest(root: &Path) -> PathBuf {
    let runs = root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs");
    let run = std::fs::read_dir(&runs)
        .expect("the runs directory")
        .map(|entry| entry.expect("a run entry"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .max_by_key(std::fs::DirEntry::file_name)
        .expect("a run");
    runs.join(run.file_name())
        .join(njutest::app::reports::DOCUMENT_NAME)
}

/// Every mutation's outcome, by its identity.
fn fates(report: &serde_json::Value) -> BTreeMap<String, String> {
    report["builds"][0]["parts"][0]["mutants"]
        .as_array()
        .expect("the mutation rows")
        .iter()
        .map(|row| {
            (
                row["id"].as_str().expect("an identity").to_owned(),
                row["decision"]["outcome"].to_string(),
            )
        })
        .collect()
}

/// The subjects of every finding of `kind`.
fn found(report: &serde_json::Value, kind: &str) -> BTreeSet<String> {
    report["builds"][0]["parts"][0]["findings"]
        .as_array()
        .expect("the findings")
        .iter()
        .filter(|finding| finding["kind"] == kind)
        .map(|finding| finding["subject"].as_str().expect("a subject").to_owned())
        .collect()
}

#[test]
fn every_knob_raises_its_finding_about_the_target_it_breaks_and_moves_nothing_else() {
    let (with, concluded) = verified(true);
    let (without, _) = verified(false);
    let records = with["builds"][0]["parts"][0]["knobs"]
        .as_array()
        .expect("the knob records");
    assert!(
        !records.is_empty(),
        "a run asked for knobs records what each established"
    );
    let put: BTreeSet<&str> = records
        .iter()
        .filter(|one| one["standing"]["state"] != "not-put")
        .map(|one| one["knob"].as_str().expect("a knob"))
        .collect();
    let broken = found(&with, "environment-dependent");
    for (knob, target) in [
        ("timezone", "environment/test/timezone"),
        ("locale", "environment/test/locale"),
        ("temp-directory", "environment/test/temp"),
        ("home", "environment/test/home"),
        ("umask", "environment/test/umask"),
        ("columns", "environment/test/columns"),
        ("threads", "environment/test/threads"),
    ] {
        assert_eq!(
            broken.contains(target),
            put.contains(knob),
            "{knob} was put exactly when {target} is found environment-dependent: {broken:?}"
        );
    }
    assert!(
        !broken.contains("environment/test/steady"),
        "a target that depends on nothing is broken by no knob: {broken:?}"
    );
    assert_eq!(
        concluded.verdict(),
        njutest::report::Verdict::Defect,
        "a suite whose answer depends on where it runs is wrong"
    );
    assert!(
        found(&without, "environment-dependent").is_empty(),
        "a run that asked for no knob raises nothing about them"
    );
    assert_eq!(
        fates(&with),
        fates(&without),
        "the knobs add controls and change nothing the mutation phase establishes"
    );
}
