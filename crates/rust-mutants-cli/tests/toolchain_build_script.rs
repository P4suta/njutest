// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library that reads a file its own build script wrote, and the environment that script left for it.

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

fn against(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied().take(1))
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all", "--offline", "--locked"])
            .chain(args.iter().copied().skip(1))
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

fn report(fixture: &Fixture) -> serde_json::Value {
    njutest_devkit::strictjson::decode_str(&njutest_devkit::fixture::stored_report(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    ))
    .expect("the report is a document")
}

#[test]
fn a_file_a_build_script_wrote_is_skipped_by_name_and_the_rest_is_measured() {
    let fixture = Fixture::copy("fixture-build-script");
    let output = against(&fixture, &["run"]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "a file the build wrote is not a reason to end the run: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document = report(&fixture);
    let skips = document["skips"].as_array().expect("the skips");
    let generated: Vec<&serde_json::Value> = skips
        .iter()
        .filter(|skip| skip["reason"].as_str() == Some("generated-outside-workspace"))
        .collect();
    assert_eq!(generated.len(), 1, "{skips:?}");
    let path = generated[0]["path"].as_str().unwrap_or_default();
    assert!(
        path.starts_with("<generated>/") && path.ends_with("table.rs"),
        "the report names the file rather than the build directory this run happened to use: \
         {path}"
    );
    assert_eq!(
        document["accounting"]["cataloged"].as_u64(),
        Some(4),
        "and the library itself is measured: {document}"
    );
}

#[test]
fn a_test_process_sees_out_dir_and_the_build_scripts_environment() {
    let fixture = Fixture::copy("fixture-build-script");
    let output = against(&fixture, &["run"]);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document = report(&fixture);
    assert_eq!(
        document["accounting"]["killed"].as_u64(),
        Some(4),
        "the fixture's own test reads OUT_DIR, and a run that did not say where the build \
         directory is would fail it for a reason that is not the mutation: {document}"
    );
    assert_eq!(document["accounting"]["errored"].as_u64(), Some(0));
}

#[test]
fn why_skipped_says_what_a_generated_file_is() {
    let fixture = Fixture::copy("fixture-build-script");
    let output = against(&fixture, &["why-skipped"]);
    assert_eq!(output.status.code(), Some(0));
    let text = njutest_devkit::process::strict_utf8(&output.stdout);
    assert!(text.contains("generated-outside-workspace"), "{text}");
    assert!(
        text.contains("the next build would write over"),
        "and why it is not a place to put a mutation: {text}"
    );
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
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    }
}
