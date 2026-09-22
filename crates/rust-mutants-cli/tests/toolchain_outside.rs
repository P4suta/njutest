// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mutation that stops a loop ending from outside it, and outside its file.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "the helper that starts the engine is not itself a test, and a document this test \
              caused to be written is one it may index"
)]

use std::path::Path;
use std::process::Output;

use njutest_devkit::fixture::Fixture;

/// Runs the fixture whole.
fn run(fixture: &Fixture) -> Output {
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", njutest_devkit::paths::utf8(fixture.root())]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    command.output().expect("rust-mutants runs")
}

/// The row of the mutation one rule proposed at one line of one file.
fn row(fixture: &Fixture, path: &str, rule: &str, line: u64) -> serde_json::Value {
    let newest = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    )
    .join("run-report-v1.json");
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&newest).expect("the report"),
    )
    .expect("the report is a document");
    document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .find(|row| {
            row["path"].as_str() == Some(path)
                && row["rule"].as_str() == Some(rule)
                && row["line"].as_u64() == Some(line)
        })
        .cloned()
        .unwrap_or_else(|| panic!("no row for {rule} at {path}:{line}: {document}"))
}

/// What to add to a failure when the row says the clock answered instead of the count.
///
/// Both stoppers are timers, so a slow enough machine loses this to the bound, and the failure then reads as a checkpoint that was never placed rather than one that was never reached in time.
fn outran_by_the_clock(row: &serde_json::Value) -> String {
    if row["outcome"].as_str() != Some("waited") || !row["step_notice"].is_null() {
        return String::new();
    }
    format!(
        "\n\nThe bound answered before the count did, which is this machine being slow rather \
         than the checkpoint being absent: the execution took {duration}ms of a two-second \
         bound that ten takes reach in about a tenth of, and left no step notice. Lower \
         `[mutation] steps` in fixtures/fixture-outside/.rust-mutants.toml",
        duration = row["duration_ms"].as_u64().unwrap_or_default()
    )
}

#[test]
fn a_mutation_outside_the_loop_and_outside_its_file_is_stopped_by_the_count() {
    let fixture = Fixture::copy("fixture-outside");
    let output = run(&fixture);
    assert!(
        output.status.code() == Some(2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );

    let literal = row(&fixture, "src/lib.rs", "int-decrement", 11);
    assert_eq!(
        literal["outcome"].as_str(),
        Some("step_limit_reached"),
        "the mutation is a literal handed to a loop in another file, so nothing at its own \
         site is taken more than once. The count reaches it because a checkpoint sits in \
         every mutable file rather than only where a mutation is.{}: {literal}",
        outran_by_the_clock(&literal)
    );
    assert_eq!(literal["step_notice"]["limit"], 10);
    assert!(
        literal["step_notice"]["observed"]
            .as_u64()
            .is_some_and(|observed| observed > 10),
        "and the boundary it names is the first count beyond the allowance: {literal}"
    );

    let inside = row(&fixture, "src/spin.rs", "delete-compound-assignment", 13);
    assert_eq!(
        inside["outcome"].as_str(),
        Some("step_limit_reached"),
        "the mutation inside the loop reaches the same allowance, which is what makes the \
         one outside it a fact about where checkpoints are rather than about this loop: \
         {inside}"
    );

    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(
        text.contains("waited=0"),
        "and no mutation of this fixture is left to the clock: {text}"
    );
}
