// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test file compiles: into its crate's one suite, as a toolchain binary of its own, or as the one binary whose subject is this repository.

#![expect(
    clippy::expect_used,
    reason = "a workspace this law cannot read is a failure it reports"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The one test file of a crate whose subject is the committed tree rather than code, a binary apart from the suite so a measurement that rewrites the tree can leave it out.
const THIS_REPOSITORY: &str = "this_repository.rs";

/// The members whose integration tests keep a layout of their own, each with why.
const APART: [(&str, &str); 1] = [(
    "crates/njutest-macros",
    "trybuild reads its compile-fail cases from tests/ui, and all_variants.rs is the one binary \
     that drives them",
)];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Every member keeping integration tests but the ones kept apart, as a directory relative to the root, read from cargo so the crate added next is held to the law the day it arrives.
fn suited(root: &Path) -> Vec<String> {
    let canonical = std::fs::canonicalize(root).expect("the workspace root resolves");
    let keeping: Vec<String> = njutest_devkit::census::members(root)
        .into_iter()
        .filter(|member| !member.suites().is_empty())
        .map(|member| {
            let directory = std::fs::canonicalize(&member.directory).expect("a member resolves");
            directory
                .strip_prefix(&canonical)
                .expect("a member lies inside the workspace")
                .to_str()
                .expect("a member directory is named in UTF-8")
                .replace('\\', "/")
        })
        .collect();
    for (apart, why) in APART {
        assert!(
            keeping.iter().any(|member| member == apart),
            "{apart} is kept apart because {why}, and it is no member keeping integration tests"
        );
    }
    let suited: Vec<String> = keeping
        .into_iter()
        .filter(|member| !APART.iter().any(|(apart, _why)| apart == member))
        .collect();
    assert!(
        suited.len() > 3,
        "the crates are read from cargo, and this found almost none: {suited:?}"
    );
    suited
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
    for crate_dir in suited(&root) {
        let crate_dir = crate_dir.as_str();
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
            let apart = file == THIS_REPOSITORY;
            match (slow || apart, binary, module) {
                (true, true, false) | (false, false, true) => {}
                (true, _, _) => refused.push(format!(
                    "{crate_dir}/tests/{file} needs a toolchain or is about this repository, so it \
                     is a [[test]] of its own and not a module of the suite"
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
