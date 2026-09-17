// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That the gates a person runs are the gates the pipeline runs, and that the inner loop is the fast half of the suite.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own files are not themselves tests: a task \
              file that cannot be read leaves nothing to assert"
)]

use std::path::Path;

fn repository(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(name))
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn task(name: &str) -> String {
    let text = repository("mise.toml");
    let start = text
        .find(&format!("[tasks.{name}]"))
        .unwrap_or_else(|| panic!("no task {name}"));
    let rest = text.get(start..).unwrap_or_default();
    let end = rest
        .get(1..)
        .and_then(|after| after.find("\n[tasks"))
        .map_or(rest.len(), |at| at.saturating_add(1));
    rest.get(..end).unwrap_or_default().to_owned()
}

/// Every gate `cargo xtask all` runs, which is what CI runs.
const GATES: [&str; 5] = ["devgates", "lints", "deps", "fixtures", "release-check"];

#[test]
fn the_gates_a_person_runs_are_the_gates_the_pipeline_runs() {
    let local = task("gates");
    for gate in GATES {
        assert!(
            local.contains(&format!("cargo xtask {gate}")),
            "`mise run gates` does not run {gate}, so a change can pass every local gate and \
             fail the pipeline: {local}"
        );
    }
    let hooks = repository("lefthook.yml");
    assert!(
        hooks.contains("cargo xtask all"),
        "the pre-push hook runs one gate rather than every gate: {hooks}"
    );
}

#[test]
fn the_inner_loop_starts_no_toolchain_and_the_whole_suite_still_runs_everything() {
    let fast = task("\"test:fast\"");
    assert!(
        fast.contains("not binary(/^toolchain_/)"),
        "the fast suite is the one that starts no cargo: {fast}"
    );
    let slow = task("\"test:slow\"");
    assert!(slow.contains("binary(/^toolchain_/)"), "{slow}");
    let whole = task("test");
    for half in ["test:fast", "test:slow", "test:doc"] {
        assert!(
            whole.contains(half),
            "`mise run test` leaves out {half}, so something is only ever run in the pipeline: \
             {whole}"
        );
    }
}

#[test]
fn every_suite_that_starts_a_toolchain_says_so_in_its_name() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut wrong = Vec::new();
    for crate_name in ["rust-mutants", "rust-mutants-cli", "njutest-cli"] {
        let tests = root.join("crates").join(crate_name).join("tests");
        let Ok(entries) = std::fs::read_dir(&tests) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            let scripted = source.contains("fake_cargo::");
            let starts_cargo = !scripted
                && (source.contains("Workspace::open") || source.contains("cargo_binary()"))
                || (!scripted
                    && source.contains("CARGO_BIN_EXE")
                    && source.contains("Fixture::copy"));
            if starts_cargo && !name.starts_with("toolchain_") {
                wrong.push(format!("{crate_name}/{name}"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "these suites start a toolchain and are in the inner loop: {wrong:?}"
    );
}

#[test]
fn every_task_that_asks_for_pipefail_says_which_shell_it_is_asking() {
    let text = repository("mise.toml");
    let mut without = Vec::new();
    for block in text.split("\n[tasks.") {
        let Some((name, body)) = block.split_once(']') else {
            continue;
        };
        if body.contains("set -euo pipefail") && !body.contains("shell = \"bash -c\"") {
            without.push(name.to_owned());
        }
    }
    assert!(
        without.is_empty(),
        "`sh` is not bash on every machine, and a task that asks for pipefail without saying \
         which shell fails there for a reason that is not the task: {without:?}"
    );
}

#[test]
fn every_task_that_runs_the_suite_builds_the_scripted_toolchain_first() {
    for name in ["\"test:fast\"", "\"test:slow\"", "coverage"] {
        let body = task(name);
        let depends = body
            .lines()
            .find(|line| line.starts_with("depends = ["))
            .unwrap_or_default();
        assert!(
            depends.contains("\"build:examples\""),
            "`cargo test --all-targets` builds an example as a test harness rather than as the \
             program it is, so a suite that drives one fails on a clean checkout without this: \
             {body}"
        );
    }
    let builder = task("\"build:examples\"");
    assert!(builder.contains("--examples"), "{builder}");
}

#[test]
fn the_pipeline_builds_the_scripted_toolchain_before_it_runs_the_suite() {
    let workflow = repository(".github/workflows/ci.yml");
    let built = workflow
        .find("cargo build --locked --examples")
        .unwrap_or_else(|| panic!("no step builds the examples: {workflow}"));
    let tested = workflow
        .find("cargo nextest run --locked --workspace")
        .unwrap_or_else(|| panic!("no step runs the suite: {workflow}"));
    assert!(
        built < tested,
        "the suite runs before the scripted cargo it drives is built"
    );
}
