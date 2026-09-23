// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The target kinds a run has to tell apart, and the path it has to carry.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helpers that start the engine are not themselves tests, and a document this \
              test caused to be written is one it may index"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn report(fixture: &Fixture) -> serde_json::Value {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all", "--offline", "--locked"])
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    njutest_devkit::strictjson::decode_str(&njutest_devkit::fixture::stored_report(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    ))
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

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}
