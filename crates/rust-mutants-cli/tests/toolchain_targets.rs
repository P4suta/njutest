// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The target kinds a run has to tell apart, and the path it has to carry.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "the helpers that start the engine are not themselves tests, and a document this \
              test caused to be written is one it may index"
)]

use mjutest_devkit::fixture::Fixture;

fn report(fixture: &Fixture) -> serde_json::Value {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rust-mutants"));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", &fixture.root().to_string_lossy()]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    let output = command.output().expect("rust-mutants runs");
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let directory = fixture.root().join("reports/mutation");
    let mut runs: Vec<std::path::PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .flatten()
        .map(|entry| entry.path().join("run-report-v1.json"))
        .filter(|path| path.is_file())
        .collect();
    runs.sort();
    serde_json::from_str(
        &std::fs::read_to_string(runs.pop().expect("one stored run")).expect("the report"),
    )
    .expect("the report is a document")
}

#[test]
fn an_example_cargo_also_tests_is_a_target_and_a_bench_is_not() {
    let fixture = Fixture::copy("fixture-targets");
    let document = report(&fixture);
    let targets: Vec<&str> = document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .filter_map(|row| row["target"].as_str())
        .collect();
    assert!(
        targets.contains(&"fixture-targets/example/demo"),
        "an example with test = true has its own tests and is where the answer came from: \
         {targets:?}"
    );
    assert!(
        !targets.iter().any(|target| target.contains("bench")),
        "a benchmark is something the build produces and no run executes: {targets:?}"
    );
}

#[test]
fn an_edition_2021_crate_is_instrumented_and_measured() {
    let fixture = Fixture::copy("fixture-targets");
    let manifest = String::from_utf8(fixture.read("Cargo.toml")).expect("utf-8");
    assert!(
        manifest.contains("edition = \"2021\""),
        "this fixture is the one that is not edition 2024"
    );
    let document = report(&fixture);
    assert_eq!(
        document["accounting"]["cataloged"].as_u64(),
        Some(4),
        "{document}"
    );
    assert_eq!(document["accounting"]["errored"].as_u64(), Some(0));
    assert_eq!(document["rejections"].as_array().map(Vec::len), Some(0));
}

#[test]
fn a_path_that_is_not_ascii_is_snapshotted_named_and_hashed() {
    let fixture = Fixture::copy("fixture-targets");
    let document = report(&fixture);
    let rows: Vec<&serde_json::Value> = document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .filter(|row| row["path"].as_str().is_some_and(|path| path.contains('ü')))
        .collect();
    assert_eq!(rows.len(), 2, "{document}");
    for row in rows {
        assert_eq!(
            row["path"].as_str(),
            Some("src/ünits/mod.rs"),
            "the report names the path as a person would type it: {row}"
        );
        let column = row["column"].as_u64().unwrap_or_default();
        assert!(column > 0, "{row}");
        assert_eq!(
            row["source_digest"].as_str().map(str::len),
            Some(64),
            "a file whose path is not one byte per character is hashed like any other: {row}"
        );
    }
}
