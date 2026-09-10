// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the doctor says about a world that is missing something, with every toolchain scripted.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::path::Path;
use std::process::Output;

use mjutest_devkit::fake_cargo::{Installed, Invocation, Script, install};
use mjutest_devkit::fixture::Fixture;

const CARGO_BANNER: &str = "cargo 1.98.0 (abc 2026-08-05)\nrelease: 1.98.0\ncommit-hash: abc\ncommit-date: 2026-08-05\nhost: x86_64-unknown-linux-gnu\n";
const RUSTC_BANNER: &str = "rustc 1.98.0 (abc 2026-08-05)\nbinary: rustc\nrelease: 1.98.0\nhost: x86_64-unknown-linux-gnu\nLLVM version: 20.1.0\n";

fn asked(fixture: &Fixture, path: &Path, extra: &[(&str, &str)]) -> Output {
    let mut command = mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command
        .env_clear()
        .env("NO_COLOR", "1")
        .env("PATH", path)
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache());
    for (name, value) in extra {
        command.env(name, value);
    }
    command
        .args(["doctor", "--root", &fixture.root().to_string_lossy()])
        .output()
        .expect("rust-mutants runs")
}

fn checks(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("the answer is JSON")
}

fn scripted(fixture: &Fixture, script: &Script, extra: &[(&str, &str)]) -> (Output, Installed) {
    let installed = install(script);
    let mut every: Vec<(String, String)> = installed
        .env()
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    for (name, value) in extra {
        every.push(((*name).to_owned(), (*value).to_owned()));
    }
    let borrowed: Vec<(&str, &str)> = every
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let output = asked(fixture, installed.bin(), &borrowed);
    (output, installed)
}

#[test]
fn a_doctor_without_a_cargo_fails_and_says_where_to_get_one() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("nothing");
    std::fs::create_dir_all(&empty).expect("an empty directory");
    let output = asked(&fixture, &empty, &[]);
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(output.status.code(), Some(2), "{text}");
    assert!(text.contains("FAIL toolchain"), "{text}");
    assert!(
        text.contains("try: install a toolchain with rustup"),
        "{text}"
    );
}

#[test]
fn a_sysroot_without_llvm_profdata_is_a_warning_that_names_the_component() {
    let fixture = Fixture::copy("fixture-simple");
    let script = Script::new()
        .answering(Invocation::new("cargo", &["-vV"]).printing(CARGO_BANNER))
        .answering(Invocation::new("rustc", &["-vV"]).printing(RUSTC_BANNER))
        .answering(
            Invocation::new("rustc", &["--print", "target-libdir"])
                .printing("/nonexistent/sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib\n"),
        );
    let (output, _installed) = scripted(&fixture, &script, &[("RUST_MUTANTS_JSON", "1")]);
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(text.contains("WARN llvm-tools"), "{text}");
    assert!(
        text.contains("try: rustup component add llvm-tools"),
        "{text}"
    );
}

#[test]
fn a_reserved_variable_is_what_the_doctor_reports_rather_than_what_stops_it() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("nothing");
    std::fs::create_dir_all(&empty).expect("an empty directory");
    let output = asked(&fixture, &empty, &[("RUST_MUTANTS_ACTIVE", "0123456789")]);
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        text.contains("FAIL environment"),
        "the one command a broken environment is for answers about it: {text}"
    );
    assert!(text.contains("RUST_MUTANTS_ACTIVE"), "{text}");
}

#[test]
fn every_check_the_lines_show_is_a_check_the_document_holds() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("nothing");
    std::fs::create_dir_all(&empty).expect("an empty directory");
    let lines = asked(&fixture, &empty, &[]);
    let mut command = mjutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    let document = command
        .env_clear()
        .env("NO_COLOR", "1")
        .env("PATH", &empty)
        .env("TMPDIR", fixture.temp())
        .env("XDG_CACHE_HOME", fixture.cache())
        .args([
            "doctor",
            "--json",
            "--root",
            &fixture.root().to_string_lossy(),
        ])
        .output()
        .expect("rust-mutants runs");
    let value: serde_json::Value = checks(&document);
    let text = String::from_utf8_lossy(&lines.stdout).into_owned();
    for check in value["checks"].as_array().expect("the checks") {
        let name = check["name"].as_str().expect("a name");
        let status = check["status"].as_str().expect("a standing");
        let said = text
            .lines()
            .find(|line| line.split_whitespace().nth(1) == Some(name))
            .unwrap_or_else(|| panic!("a line for {name} in:\n{text}"));
        assert!(
            said.starts_with(&status.to_uppercase()),
            "{name} stands {status} in the document and not in the lines: {said}"
        );
        assert!(
            said.contains(check["detail"].as_str().expect("a detail")),
            "{said}"
        );
        if let Some(remedy) = check["remedy"].as_str() {
            assert!(
                text.contains(remedy),
                "{remedy} is not in the lines:\n{text}"
            );
        }
    }
}
