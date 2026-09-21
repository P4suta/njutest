// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two ways a test process ends other than by failing an assertion, and the one verdict both earn.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "the helpers that start the engine are not themselves tests, and a document this \
              test caused to be written is one it may index"
)]

use njutest_devkit::fixture::Fixture;
use std::path::Path;

/// Every row of a whole run of the fixture, by rule and line.
fn rows(fixture: &Fixture) -> serde_json::Value {
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", njutest_devkit::paths::utf8(fixture.root())]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    let output = command.output().expect("rust-mutants runs");
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let newest = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    )
    .join("run-report-v1.json");
    njutest_devkit::strictjson::decode_str(&std::fs::read_to_string(newest).expect("the report"))
        .expect("the report is a document")
}

fn row<'a>(document: &'a serde_json::Value, rule: &str) -> &'a serde_json::Value {
    document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .find(|row| row["rule"].as_str() == Some(rule))
        .unwrap_or_else(|| panic!("no row for {rule}: {document}"))
}

#[test]
fn a_should_panic_test_that_stops_panicking_is_a_kill() {
    let fixture = Fixture::copy("fixture-panics");
    let document = rows(&fixture);
    let moved = row(&document, "ge-to-gt");
    assert_eq!(
        moved["outcome"].as_str(),
        Some("killed"),
        "moving the bound past the value the should_panic test passes stops the panic, and a \
         test that asks for one and does not get it fails: {moved}"
    );
    assert_eq!(
        moved["exit_code"].as_i64(),
        Some(101),
        "a libtest binary reports a failing test by exiting 101: {moved}"
    );
}

#[test]
fn a_process_that_aborts_is_a_kill_on_every_platform() {
    let fixture = Fixture::copy("fixture-panics");
    let document = rows(&fixture);
    let aborted = row(&document, "eq-to-neq");
    assert_eq!(
        aborted["outcome"].as_str(),
        Some("killed"),
        "a process that stops without exiting is one no test could have passed under: {aborted}"
    );
    let exit = aborted["exit_code"].as_i64().unwrap_or_default();
    assert_ne!(exit, 0, "{aborted}");
    assert_ne!(
        exit, 101,
        "the abort is not a failing assertion; it is the process ending: {aborted}"
    );
    #[cfg(unix)]
    assert_eq!(
        exit, 134,
        "on unix a signal is reported as 128 plus its number, and abort is six: {aborted}"
    );
}

#[test]
fn every_mutation_but_the_one_no_test_in_this_process_can_reach_is_noticed() {
    let fixture = Fixture::copy("fixture-panics");
    let document = rows(&fixture);
    let accounting = &document["accounting"];
    assert_eq!(
        accounting["killed"].as_u64(),
        Some(12),
        "a process that ends without failing an assertion is a kill, whichever way it \
         ends: {accounting}"
    );
    assert_eq!(
        accounting["survived"].as_u64(),
        Some(1),
        "and one is not, by construction. `capacity(0)` aborts, so the only test that \
         could notice `if n == 0` being made false is one that calls it, and a test that \
         calls it takes the harness with it. The mutation that makes the abort happen is \
         noticed; the one that makes it not happen cannot be, and a fixture that claimed \
         otherwise would be claiming a test nobody can write: {accounting}"
    );
}
