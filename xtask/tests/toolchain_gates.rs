// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The repository gates run by `all`, with coverage measured separately in CI.

use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::gates;

mod claims_oracle;

struct SuiteBuild {
    engine: PathBuf,
    compiled: usize,
}

const SUITE: [&str; 5] = [
    "test",
    "--no-run",
    "--workspace",
    "--all-targets",
    "--all-features",
];

#[expect(
    clippy::expect_used,
    reason = "a toolchain test cannot assert anything when the suite's own build cannot be read"
)]
fn suite_build(repository: &Path) -> SuiteBuild {
    let mut command = Command::new(njutest_devkit::paths::cargo_binary());
    command
        .args(SUITE)
        .args([
            "--offline",
            "--locked",
            "--message-format",
            "json-render-diagnostics",
        ])
        .current_dir(repository);
    let output = njutest_devkit::cost::cargo(command, "the suite's own build")
        .expect("the suite's build starts");
    assert!(
        output.status.success(),
        "the suite's build completes: {}",
        output.stderr.escape_ascii()
    );
    let text = String::from_utf8(output.stdout).expect("cargo prints UTF-8");
    let mut engines = Vec::new();
    let mut compiled = 0_usize;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let message = xtask::strictjson::from_str(line).expect("cargo prints JSON messages");
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let artifact: cargo_metadata::Artifact =
            serde_json::from_value(message).expect("a compiler artifact message");
        if !artifact.fresh {
            compiled = compiled.checked_add(1).expect("the unit count fits");
        }
        if artifact.target.name == "rust-mutants"
            && artifact.target.is_bin()
            && !artifact.profile.test
            && let Some(executable) = artifact.executable
        {
            engines.push(executable.into_std_path_buf());
        }
    }
    let engine = engines.pop().expect("the suite's build makes the engine");
    assert!(engines.is_empty(), "the suite's build makes one engine");
    SuiteBuild { engine, compiled }
}

#[test]
fn the_typed_repository_set_runs_in_all_and_coverage_is_separate() {
    let root = gates::workspace_root();
    let established = suite_build(&root);
    let built_by_the_suite = njutest_devkit::reproducible::digest(&established.engine);
    let cargo = njutest_devkit::paths::cargo_binary();
    let environment = xtask::environment::Environment::of(std::env::vars_os());
    let running = std::env::current_exe().expect("the test names the program it runs in");
    let engine = xtask::claims::Engine {
        cargo: cargo.as_os_str(),
        build: &SUITE,
        environment: &environment,
        running: &running,
    };
    let report = gates::all(&root, &engine).expect("every gate passes on this tree");
    assert_eq!(
        njutest_devkit::reproducible::digest(&established.engine),
        built_by_the_suite,
        "a gate built the engine again under a selection of its own and put that build where \
         the suite's engine was, compiling what the suite never built and handing every test \
         after it a binary the suite never built"
    );
    assert_eq!(
        suite_build(&root).compiled,
        0,
        "the suite's own build is no longer current after the gates ran"
    );
    assert!(report.contains("skipped:"), "{report}");
    assert!(!report.contains("coverage-ratchet:"), "{report}");

    let workflow = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect(".github/workflows/ci.yml");
    let (regular, coverage) = workflow
        .split_once("\n  coverage:\n")
        .expect("CI has a separate coverage job");
    assert!(regular.contains("run: cargo xtask all"));
    assert!(coverage.contains("run: cargo xtask coverage-ratchet"));
}
