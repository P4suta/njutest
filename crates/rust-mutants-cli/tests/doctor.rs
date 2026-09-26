// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the doctor says about a world that is missing something, with every toolchain scripted.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::path::Path;
use std::process::Output;

use njutest_devkit::fake_cargo::{Installed, Invocation, Script, install};
use njutest_devkit::fixture::Fixture;

include!("support/metadata.rs");

const CARGO_BANNER: &str = "cargo 1.98.0 (abc 2026-08-05)\nrelease: 1.98.0\ncommit-hash: abc\ncommit-date: 2026-08-05\nhost: x86_64-unknown-linux-gnu\n";
const RUSTC_BANNER: &str = "rustc 1.98.0 (abc 2026-08-05)\nbinary: rustc\nrelease: 1.98.0\nhost: x86_64-unknown-linux-gnu\nLLVM version: 20.1.0\n";

fn asked(fixture: &Fixture, path: &Path, extra: &[(&str, &str)]) -> Output {
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command
        .env_clear()
        .env("NO_COLOR", "1")
        .env("PATH", path)
        .envs(njutest_devkit::paths::temporary_directory(fixture.temp()))
        .env("XDG_CACHE_HOME", fixture.cache());
    for (name, value) in extra {
        command.env(name, value);
    }
    command
        .args([
            "doctor",
            "--root",
            njutest_devkit::paths::utf8(fixture.root()),
        ])
        .output()
        .expect("rust-mutants runs")
}

fn checks(output: &Output) -> serde_json::Value {
    njutest_devkit::strictjson::decode_slice(&output.stdout).expect("the answer is JSON")
}

fn held_doctor(value: &serde_json::Value) -> rust_mutants_cli::report::doctor::DoctorDocument {
    fn text(object: &serde_json::Value, field: &str) -> String {
        object[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} is a string"))
            .to_owned()
    }

    let held_checks = value["checks"]
        .as_array()
        .expect("checks are an array")
        .iter()
        .map(|check| rust_mutants_cli::report::doctor::Check {
            name: text(check, "name"),
            ok: check["ok"].as_bool().expect("ok is a boolean"),
            status: text(check, "status"),
            detail: text(check, "detail"),
            remedy: check
                .get("remedy")
                .map(|remedy| remedy.as_str().expect("remedy is a string").to_owned()),
        })
        .collect();
    rust_mutants_cli::report::doctor::DoctorDocument {
        document_type: text(value, "document_type"),
        schema_version: u32::try_from(
            value["schema_version"]
                .as_u64()
                .expect("schema_version is an unsigned integer"),
        )
        .expect("schema_version fits u32"),
        tool_version: text(value, "tool_version"),
        ok: value["ok"].as_bool().expect("ok is a boolean"),
        checks: held_checks,
    }
}

fn scripted(fixture: &Fixture, script: &Script, extra: &[(&str, &str)]) -> (Output, Installed) {
    let installed = install(script);
    let mut every: Vec<(String, String)> = installed
        .env()
        .into_iter()
        .map(|(name, value)| {
            (
                njutest_devkit::paths::owned_utf8(name),
                njutest_devkit::paths::owned_utf8(value),
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
    let text = njutest_devkit::process::strict_utf8(&output.stdout).into_owned();
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
    let (output, installed) = scripted(&fixture, &script, &[("RUST_MUTANTS_JSON", "1")]);
    assert!(
        test_metadata(installed.bin()).is_dir(),
        "the scripted toolchain remains installed while read"
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout).into_owned();
    assert!(text.contains("WARN llvm-tools"), "{text}");
    assert!(
        text.contains("try: rustup component add llvm-tools"),
        "{text}"
    );
}

/// The doctor reports a reserved variable rather than stopping on it.
#[test]
fn a_reserved_variable_is_what_the_doctor_reports_rather_than_what_stops_it() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("nothing");
    std::fs::create_dir_all(&empty).expect("an empty directory");
    let output = asked(
        &fixture,
        &empty,
        &[("RUST_MUTANTS_TOUCH", "/nowhere/touch.log")],
    );
    let text = njutest_devkit::process::strict_utf8(&output.stdout).into_owned();
    assert!(
        text.contains("FAIL environment"),
        "the one command a broken environment is for answers about it: {text}"
    );
    assert!(text.contains("RUST_MUTANTS_TOUCH"), "{text}");
}

#[test]
fn every_check_the_lines_show_is_a_check_the_document_holds() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("nothing");
    std::fs::create_dir_all(&empty).expect("an empty directory");

    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    let document = command
        .env_clear()
        .env("NO_COLOR", "1")
        .env("PATH", &empty)
        .envs(njutest_devkit::paths::temporary_directory(fixture.temp()))
        .env("XDG_CACHE_HOME", fixture.cache())
        .args([
            "doctor",
            "--json",
            "--root",
            njutest_devkit::paths::utf8(fixture.root()),
        ])
        .output()
        .expect("rust-mutants runs");
    let value: serde_json::Value = checks(&document);
    let held = held_doctor(&value);
    let text = rust_mutants_cli::report::doctor::lines(&held);
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

/// Whether a detail names a thing rather than only asserting a state.
///
/// A number, a path, a variable or a quoted name is something a reader can go and look at.
/// "Nothing is left over" is a claim about somewhere nobody identified, and a reader whose `TMPDIR` is not what they think has been given a clean bill for the wrong place.
fn names_something(detail: &str) -> bool {
    detail.chars().any(|one| one.is_ascii_digit())
        || detail.contains('/')
        || detail.contains('\\')
        || detail.contains('`')
        || detail
            .split_whitespace()
            .any(|word| word.chars().filter(char::is_ascii_uppercase).count() >= 3)
}

/// A check that passed says what it looked at, not only that it was well.
///
/// Two of them said "nothing is left over" and "no reserved variable is set" and named neither the directory nor the variables, so a reader whose `TMPDIR` was not what they thought got a clean bill for somewhere nobody had identified.
/// A pass a reader cannot check is a pass they learn to skip,
/// and the failing branch of each of those checks already named the thing.
#[test]
fn every_check_that_passed_names_what_it_looked_at() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("nothing");
    std::fs::create_dir_all(&empty).expect("an empty directory");
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    let document = command
        .env_clear()
        .env("NO_COLOR", "1")
        .env("PATH", &empty)
        .envs(njutest_devkit::paths::temporary_directory(fixture.temp()))
        .env("XDG_CACHE_HOME", fixture.cache())
        .args([
            "doctor",
            "--json",
            "--root",
            njutest_devkit::paths::utf8(fixture.root()),
        ])
        .output()
        .expect("rust-mutants runs");
    let document: serde_json::Value = checks(&document);

    let silent: Vec<String> = document["checks"]
        .as_array()
        .expect("the checks")
        .iter()
        .filter(|check| check["status"].as_str() == Some("ok"))
        .filter(|check| !names_something(check["detail"].as_str().unwrap_or_default()))
        .map(|check| {
            format!(
                "{}: {}",
                check["name"].as_str().unwrap_or_default(),
                check["detail"].as_str().unwrap_or_default()
            )
        })
        .collect();

    assert!(
        silent.is_empty(),
        "these say a run is well and do not say about what: a path, a count or a version \
         is what makes a pass one a reader can check rather than one they take on trust. \
         {silent:?}"
    );
}

#[test]
fn a_cargo_named_on_the_command_line_is_used_where_the_path_has_none() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("nothing");
    std::fs::create_dir_all(&empty).expect("an empty directory");
    let installed = install(
        &Script::new()
            .answering(Invocation::new("cargo", &["-vV"]).printing(CARGO_BANNER))
            .answering(Invocation::new("rustc", &["-vV"]).printing(RUSTC_BANNER))
            .answering(
                Invocation::new("rustc", &["--print", "target-libdir"])
                    .printing("/nonexistent/sysroot/lib/rustlib/x86_64-unknown-linux-gnu/lib\n"),
            ),
    );
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command
        .env_clear()
        .env("NO_COLOR", "1")
        .envs(installed.env())
        .env("PATH", &empty)
        .envs(njutest_devkit::paths::temporary_directory(fixture.temp()))
        .env("XDG_CACHE_HOME", fixture.cache())
        .args([
            "doctor",
            "--root",
            njutest_devkit::paths::utf8(fixture.root()),
            "--cargo",
        ])
        .arg(installed.cargo());
    let output = command.output().expect("rust-mutants runs");
    let text = njutest_devkit::process::strict_utf8(&output.stdout).into_owned();
    assert!(
        !text.contains("FAIL toolchain") && text.contains("cargo 1.98.0"),
        "RM1012 tells a reader whose PATH has no cargo to name one with --cargo, so the cargo it \
         names is the one every lookup uses: {text}"
    );
}
