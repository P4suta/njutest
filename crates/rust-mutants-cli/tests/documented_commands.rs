// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every `rust-mutants` command a documented workflow runs is one this release parses.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use clap::Parser as _;

/// Every markdown page of the documentation, with the path it is read from.
fn pages() -> Vec<(std::path::PathBuf, String)> {
    let root = njutest_devkit::paths::workspace_root();
    let mut pending = vec![root.join("docs")];
    let mut pages = vec![(root.join("README.md"), String::new())];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("a documentation directory") {
            let entry = entry.expect("a directory entry");
            let path = entry.path();
            if entry.file_type().expect("an entry's type").is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                pages.push((path, String::new()));
            }
        }
    }
    pages
        .into_iter()
        .map(|(path, _)| {
            let text = std::fs::read_to_string(&path).expect("a readable page");
            (path, text)
        })
        .collect()
}

#[test]
fn every_documented_rust_mutants_command_in_a_workflow_parses() {
    let mut asked = 0_usize;
    let mut refused = Vec::new();
    for (path, text) in pages() {
        for command in njutest_devkit::workflow_commands::commands(&text, "rust-mutants") {
            asked += 1;
            if let Err(error) = rust_mutants_cli::cli::Cli::try_parse_from(&command.argv) {
                refused.push(format!(
                    "{}:{}: {}\n{}",
                    path.display(),
                    command.line,
                    command.argv.join(" "),
                    error.render()
                ));
            }
        }
    }
    assert!(
        asked > 0,
        "the documentation shows no workflow that runs rust-mutants, so this law holds nothing"
    );
    assert!(
        refused.is_empty(),
        "a workflow a reader copies from the documentation runs a command this release refuses:\n{}",
        refused.join("\n")
    );
}
