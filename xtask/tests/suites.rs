// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test file compiles: into its crate's one suite, or as a toolchain binary of its own.

#![expect(
    clippy::expect_used,
    reason = "a workspace this law cannot read is a failure it reports"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The crates whose integration tests are one suite plus a binary per toolchain test.
const SUITED: [&str; 5] = [
    "crates/njutest",
    "crates/rust-mutants",
    "crates/rust-mutants-cli",
    "crates/njutest-devkit",
    "xtask",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("a file the law reads")
}

/// The top-level `.rs` files of `tests`, but the suite itself.
fn test_files(tests: &Path) -> BTreeSet<String> {
    std::fs::read_dir(tests)
        .expect("a tests directory")
        .map(|entry| entry.expect("an entry of the tests directory"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            entry
                .file_name()
                .into_string()
                .expect("a test file named in UTF-8")
        })
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "rs")
                && name != "suite.rs"
        })
        .collect()
}

/// The quoted file names after `marker` on each line of `text`.
fn quoted_after(text: &str, marker: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix(marker))
        .filter_map(|rest| rest.split('"').nth(1))
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_test_file_is_compiled_by_the_suite_or_as_a_toolchain_binary() {
    let root = root();
    let mut refused = Vec::new();
    for crate_dir in SUITED {
        let manifest = read(&root.join(crate_dir).join("Cargo.toml"));
        if !manifest
            .lines()
            .any(|line| line.trim() == "autotests = false")
        {
            refused.push(format!(
                "{crate_dir}: autotests is not off, so every file is a binary"
            ));
        }
        let declared: Vec<String> = quoted_after(&manifest, "path = ")
            .into_iter()
            .filter_map(|path| path.strip_prefix("tests/").map(str::to_owned))
            .collect();
        if !declared.iter().any(|path| path == "suite.rs") {
            refused.push(format!("{crate_dir}: no [[test]] compiles tests/suite.rs"));
        }
        let suite = read(&root.join(crate_dir).join("tests/suite.rs"));
        let moduled: Vec<String> = quoted_after(&suite, "#[path = ");
        let files = test_files(&root.join(crate_dir).join("tests"));
        for file in &files {
            let binary = declared.contains(file);
            let module = moduled.contains(file);
            let slow = file.starts_with("toolchain_");
            match (slow, binary, module) {
                (true, true, false) | (false, false, true) => {}
                (true, _, _) => refused.push(format!(
                    "{crate_dir}/tests/{file} needs a toolchain, so it is a [[test]] of its own and \
                     not a module of the suite"
                )),
                (false, _, _) => refused.push(format!(
                    "{crate_dir}/tests/{file} is compiled by nothing: add `#[path = \"{file}\"] mod \
                     …;` to tests/suite.rs"
                )),
            }
        }
        for named in declared.iter().chain(&moduled) {
            if named != "suite.rs" && !files.contains(named) {
                refused.push(format!(
                    "{crate_dir}: tests/{named} is named and does not exist"
                ));
            }
        }
    }
    assert!(
        refused.is_empty(),
        "with autotests off a test file nothing names is never compiled and never fails, so every \
         one is named exactly once:\n  {}",
        refused.join("\n  ")
    );
}
