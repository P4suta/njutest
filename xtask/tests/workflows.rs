// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a step which can run on Windows says which shell it is written for.

#![expect(
    clippy::panic,
    reason = "the helper that reads the repository's own workflows is not itself a test: a \
              workflow that cannot be read leaves nothing to assert"
)]

use std::path::{Path, PathBuf};

fn workflows() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/workflows");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|error| panic!("entry under {}: {error}", dir.display())))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|one| one == "yml"))
        .collect();
    found.sort();
    found
}

/// The jobs of one workflow, as name and body.
fn jobs(source: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        let named = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && !rest.starts_with('#'))
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|name| !name.contains(' '));
        match named {
            Some(name) => found.push((name.to_owned(), String::new())),
            None => {
                if let Some(last) = found.last_mut() {
                    last.1.push_str(line);
                    last.1.push('\n');
                }
            }
        }
    }
    found
}

/// The steps of one job body, each as the lines from its `- ` to the next one's.
fn steps(body: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in body.lines() {
        if line.trim_start().starts_with("- ") && line.starts_with("      ") {
            found.push(String::new());
        }
        if let Some(last) = found.last_mut() {
            last.push_str(line);
            last.push('\n');
        }
    }
    found
}

#[test]
fn a_step_that_can_run_on_windows_says_which_shell_it_is_written_for() {
    let mut silent = Vec::new();
    for path in workflows() {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let file = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_else(|| panic!("a workflow file name is not UTF-8: {}", path.display()));
        for (job, body) in jobs(&source) {
            if !body.contains("windows-") {
                continue;
            }
            for step in steps(&body) {
                let runs = step
                    .lines()
                    .any(|line| line.trim_start().starts_with("run:"));
                let said = step
                    .lines()
                    .any(|line| line.trim_start().starts_with("shell:"));
                if runs && !said {
                    let name = step
                        .lines()
                        .find_map(|line| line.trim_start().strip_prefix("- name: "))
                        .unwrap_or("(unnamed)")
                        .to_owned();
                    silent.push(format!("{file}: {job}: {name}"));
                }
            }
        }
    }

    assert!(
        silent.is_empty(),
        "a step of a job that can run on Windows takes PowerShell unless it says \
         otherwise, and there a native command that fails does not stop the script: the \
         step runs on, and its status becomes the last command's. That is how a failing \
         test was reported forty-five minutes later as a cancelled job, and how one \
         could have been reported as a pass. Say `shell: bash`, which every runner has. \
         {silent:?}"
    );
}

#[test]
fn the_real_kani_job_installs_one_exact_locked_version() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/workflows/ci.yml");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let body = jobs(&source)
        .into_iter()
        .find_map(|(name, body)| (name == "kani-verified").then_some(body))
        .unwrap_or_else(|| panic!("{} has no kani-verified job", path.display()));
    assert!(
        body.contains("cargo install --locked kani-verifier --version '=0.68.0'"),
        "the proof job must make Cargo's exact-version intent machine-readable"
    );
}

/// The tools `mise.toml` pins, under the names `taiki-e/install-action` knows them by.
///
/// Three are spelled differently there, and a handful are this machine's alone: a pinned rust toolchain, the hook runner, and the compilation cache are not things a hosted runner installs through that action.
fn pinned() -> Vec<String> {
    const RENAMED: [(&str, &str); 3] = [
        ("typos", "typos-cli"),
        ("taplo", "taplo-cli"),
        ("mdbook", "mdbook"),
    ];
    const LOCAL_ONLY: [&str; 3] = ["rust", "lefthook", "sccache"];

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("mise.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let table = text
        .parse::<toml::Table>()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let Some(toml::Value::Table(tools)) = table.get("tools") else {
        panic!("mise.toml declares the tools it pins")
    };
    let mut found = Vec::new();
    for (name, version) in tools {
        let name = name.trim_matches('"');
        let bare = name.strip_prefix("cargo:").unwrap_or(name);
        if LOCAL_ONLY.contains(&bare) {
            continue;
        }
        let Some(version) = version.as_str() else {
            panic!("{bare} is pinned to one exact version")
        };
        let installed = RENAMED
            .iter()
            .find(|(mine, _)| *mine == bare)
            .map_or(bare, |(_, theirs)| theirs);
        found.push(format!("{installed}@{version}"));
    }
    found.sort();
    found
}

/// Every tool the pipeline installs through the pinned setup action, once each.
fn installed() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut pending = vec![
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".github"),
    ];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if std::fs::metadata(&path).is_ok_and(|one| one.is_dir()) {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|kind| kind != "yml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let Some((_, listed)) = line.split_once("tools:") else {
                    continue;
                };
                found.extend(
                    listed
                        .split(',')
                        .map(str::trim)
                        .filter(|tool| tool.contains('@'))
                        .map(ToOwned::to_owned),
                );
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn executable_tools_use_the_commit_pinned_installer_and_exact_versions() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/workflows/ci.yml");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let pinned = pinned();
    let installed = installed();
    assert!(pinned.len() > 4, "mise pins the tools: {pinned:?}");
    assert!(
        installed.len() > 4,
        "the pipeline installs them: {installed:?}"
    );
    let adrift: Vec<&String> = installed
        .iter()
        .filter(|tool| !pinned.contains(tool))
        .collect();
    assert!(
        adrift.is_empty(),
        "the pipeline installs a version mise.toml does not pin, so a local run and the \
         pipeline answer with different tools and the only thing holding them together \
         is a comment: {adrift:?} against {pinned:?}"
    );
    for unverified in ["curl ", "wget ", "Invoke-WebRequest", "| tar"] {
        assert!(
            !source.contains(unverified),
            "CI downloads an executable without an independently pinned digest ({unverified:?}): {source}"
        );
    }
}
