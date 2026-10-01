// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every dimension of a run is one column, and `whole-v1` is not assured while any of them is a hole (ADR 0033).

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Output;

use njutest::cli::Environment;
use njutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;

struct Fixture {
    root: PathBuf,
    dir: tempfile::TempDir,
}

fn fixture(name: &str, config: &str) -> Fixture {
    let source = njutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("njutest-matrix-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy_tree(&source, &root);
    std::fs::write(root.join(".njutest.toml"), config).expect("the configuration");
    Fixture { root, dir }
}

fn verify(fixture: &Fixture) -> Output {
    verify_with(fixture, &[])
}

fn verify_with(fixture: &Fixture, extra: &[&str]) -> Output {
    let events = fixture.dir.path().join("clock-events");
    std::fs::create_dir_all(&events).expect("clock events");
    let mut vars: rust_mutants::vars::Variables =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect();
    vars.set("NJUTEST_TEST_CLOCK", events.as_os_str());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        [
            "njutest",
            "verify",
            "--offline",
            "--locked",
            "--format",
            "lines",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(OsString::from),
        &Environment {
            cache_directory: fixture.root.join(".cache"),
            working_directory: fixture.root.clone(),
            temp_directory: njutest_devkit::paths::temp_beside(&fixture.root)
                .expect("a temporary directory"),
            program: PathBuf::from("this test never runs it"),
            vars,
            cancel: Cancel::new().with_clock(rust_mutants::runner::Clock::events(events)),
            terminal: njutest::presentation::Terminal::default(),
        },
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

fn dimensions(output: &Output) -> Vec<String> {
    njutest_devkit::process::strict_utf8(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("DIMENSION\t"))
        .map(|line| {
            line.split('\t')
                .skip(1)
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn a_whole_run_asks_every_dimension_and_is_not_assured_while_one_is_a_hole() {
    let fixture = fixture("fixture-faulted", "version = 1\ncontract = \"whole-v1\"\n");
    let output = verify(&fixture);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert_eq!(
        dimensions(&output),
        vec![
            "mutation measured",
            "repeatable measured",
            "fault measured",
            "schedule measured",
            "wire nothing-to-ask",
            "durable nothing-to-ask",
        ],
        "every dimension is a row, and a whole run asks every one it can: {said}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert!(
        said.contains("VERDICT\tINSUFFICIENT"),
        "one binary is not proven to run one thread and no schedule of it was established, which is a hole: {said}"
    );
    assert!(
        said.contains("FINDING\tdimension-not-measured\tschedule\t")
            && !said.contains("FINDING\tdimension-not-measured\tdurable\t"),
        "and a finding names it, while durability, which found no call that writes, is not: {said}"
    );
}

#[test]
fn a_standard_run_shows_the_matrix_and_is_decided_as_it_was() {
    let fixture = fixture(
        "fixture-faulted",
        "version = 1\ncontract = \"standard-v1\"\n",
    );
    let output = verify(&fixture);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert_eq!(
        dimensions(&output),
        vec![
            "mutation measured",
            "repeatable not-asked",
            "fault not-asked",
            "schedule measured",
            "wire nothing-to-ask",
            "durable not-asked",
        ],
        "{said}"
    );
    assert!(
        !said.contains("dimension-not-measured"),
        "a contract that does not ask every dimension raises nothing about one it did not ask: {said}"
    );
}

#[test]
fn a_run_that_names_no_contract_asks_every_dimension() {
    let fixture = fixture("fixture-faulted", "version = 1\n");
    let output = verify(&fixture);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        said.contains("contract=whole-v1"),
        "a run that names no contract gets the strictest answer (ADR 0033): {said}"
    );
    assert!(
        !dimensions(&output)
            .iter()
            .any(|row| row.ends_with("not-asked")),
        "and every dimension is asked: {said}"
    );
}

#[test]
fn a_whole_run_that_seals_nothing_is_one_the_audit_re_decides_without_a_violation() {
    let fixture = fixture("fixture-simple", "version = 1\ncontract = \"whole-v1\"\n");
    let trace = fixture.root.join("recorded");
    let traced = format!("--trace={}", trace.display());
    let output = verify_with(&fixture, &["--no-seal", "--no-cache", &traced]);
    let said = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        said.contains("FINDING\tdimension-not-measured\tmutation\t"),
        "with nothing sealed, every answer is a lead and the mutation column is a hole, which a \
         whole run names: {said}\n{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let runs: Vec<PathBuf> = std::fs::read_dir(
        fixture
            .root
            .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join("runs"),
    )
    .expect("the run wrote its report")
    .map(|entry| entry.expect("a stored run"))
    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
    .map(|entry| entry.path())
    .collect();
    let [run] = runs.as_slice() else {
        panic!("one run, one report: {runs:?}");
    };
    let root = njutest_devkit::paths::workspace_root();
    let audited = njutest_devkit::paths::command(&njutest_devkit::paths::cargo_binary())
        .args(["xtask", "proofaudit"])
        .arg(run)
        .arg("--trace")
        .arg(&trace)
        .current_dir(&root)
        .output()
        .expect("the audit starts");
    let audit = njutest_devkit::process::strict_utf8(&audited.stdout);
    assert!(
        audited.status.success() && audit.contains("; 0 violations, 0 unaudited"),
        "the runner and the audit each count a lead as a hole in the mutation column, and every \
         other column the same way, so a whole run that seals nothing is re-decided with nothing \
         to say against it: {audit}\n{}",
        njutest_devkit::process::strict_utf8(&audited.stderr)
    );
}
