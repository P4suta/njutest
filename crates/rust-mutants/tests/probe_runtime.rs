// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The generated probe runtime: that it compiles, that it answers, and that a float is refused.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::process::Command;

use rust_mutants::probe::runtime::{MARKER, MODULE_STEM, PROBE_ENV, UNAVAILABLE_EXIT, render};

fn compile(source: &str, name: &str) -> std::process::Output {
    let dir = mjutest_devkit::paths::workspace_root().join("target/probe-runtime");
    std::fs::create_dir_all(&dir).expect("a place to build");
    let path = dir.join(format!("{name}.rs"));
    std::fs::write(&path, source).expect("write");
    Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "lib"])
        .arg("--out-dir")
        .arg(&dir)
        .arg(&path)
        .output()
        .expect("rustc runs")
}

fn module() -> String {
    render(MODULE_STEM, &"a".repeat(64), 3, &[0, 1, 2])
}

#[test]
fn the_generated_runtime_says_what_it_is_and_which_catalog_it_is_about() {
    let text = module();
    assert!(text.contains(MARKER), "{text}");
    assert!(text.contains(&"a".repeat(64)), "{text}");
    assert!(text.contains(PROBE_ENV), "{text}");
    assert!(text.contains(&UNAVAILABLE_EXIT.to_string()), "{text}");
    assert!(
        !text.contains("unsafe"),
        "a crate that forbids unsafe code still compiles: {text}"
    );
}

#[test]
fn the_generated_runtime_compiles_on_its_own() {
    let output = compile(&module(), "alone");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_value_that_already_is_the_default_answers_yes_and_one_that_is_not_answers_no() {
    let source = format!(
        "{}\n\
         pub fn zero() -> bool {{ let v: i32 = 0; (&v).probed() }}\n\
         pub fn one() -> bool {{ let v: i32 = 1; (&v).probed() }}\n\
         pub fn empty() -> bool {{ let v = String::new(); (&v).probed() }}\n\
         use {MODULE_STEM}::Probe as _;\n",
        module()
    );
    let output = compile(&source, "answers");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_probe_of_a_float_is_a_compile_error_rather_than_a_wrong_answer() {
    let source = format!(
        "{}\n\
         pub fn zero() -> bool {{ let v: f64 = 0.0; (&v).probed() }}\n\
         use {MODULE_STEM}::{{FloatRefuse as _, Probe as _}};\n",
        module()
    );
    let output = compile(&source, "float");
    assert!(
        !output.status.success(),
        "-0.0 equals 0.0 and is not what Default writes, so the compiler is what keeps a \
         float probe out"
    );
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("Refused"), "{said}");
}
