// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the dogfood tasks must and must not do.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking and slices a file it wrote itself"
)]

use std::path::Path;

fn mise() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../mise.toml"))
        .expect("the task file")
}

fn task(name: &str) -> String {
    let text = mise();
    let start = text
        .find(&format!("[tasks.{name}]"))
        .unwrap_or_else(|| panic!("no task {name}"));
    let rest = &text[start..];
    let end = rest[1..]
        .find("\n[tasks")
        .map_or(rest.len(), |at| at.saturating_add(1));
    rest[..end].to_owned()
}

#[test]
fn dogfood_runs_the_built_binary_and_never_cargo_run() {
    for name in [
        "dogfood",
        "\"dogfood:engine\"",
        "\"dogfood:audit\"",
        "\"dogfood:engine:audit\"",
    ] {
        let body = task(name);
        assert!(
            !body.contains("cargo run"),
            "a `cargo run` wrapper survives Ctrl-C as an orphan holding the child: {body}"
        );
        assert!(
            body.contains("--release"),
            "dogfooding a debug build measures the wrong program: {body}"
        );
        assert!(body.contains("./target/release/"), "{body}");
    }
}

#[test]
fn dogfood_verifies_rather_than_merely_building() {
    let body = task("dogfood");
    assert!(body.contains("mjutest verify"), "{body}");
    assert!(
        body.contains("--ui=plain"),
        "the output is for a person reading a terminal: {body}"
    );
}

#[test]
fn the_engine_audit_task_checks_the_recording_before_re_deciding_it() {
    let body = task("\"dogfood:engine:audit\"");
    assert!(
        body.contains("--trace"),
        "a run nobody recorded leaves every trace layer unaudited: {body}"
    );
    let checked = body.find("trace check").unwrap_or_else(|| panic!("{body}"));
    let audited = body
        .find("engine-audit")
        .unwrap_or_else(|| panic!("{body}"));
    assert!(
        checked < audited,
        "a recording that lost events is not one to re-decide a run from: {body}"
    );
    assert!(
        body.contains("--ledger .rust-mutants.toml"),
        "a survivor nobody accepted has to fail the gate: {body}"
    );
}

#[test]
fn the_engine_ledger_names_what_it_measures_and_asks_for_the_proof_layer_that_ships() {
    let ledger = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.rust-mutants.toml"),
    )
    .expect("the engine's own ledger");
    assert!(ledger.contains("packages = [\"rust-mutants\"]"), "{ledger}");
    assert!(
        ledger.contains("coverage = true"),
        "coverage routing is the one shipped proof layer, and the run is what proves it \
         removes work: {ledger}"
    );
}
