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

/// Every tool `mise.toml` pins, split by the key that says who installs it.
///
/// A `cargo:` prefix is a crate and carries its exact version to `taiki-e/install-action`; a bare name is a tool mise fetches, and carries no version because mise reads the same pin this does.
/// Three are named by no job: the toolchain comes from `rust-toolchain.toml`, a runner runs no hooks, and the compiler wrapper is installed by the setup action itself because every job inherits it from `[env]`.
fn pinned() -> (Vec<String>, Vec<String>) {
    const NAMED_BY_NO_JOB: [&str; 3] = ["rust", "lefthook", "sccache"];

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
    let mut crates = Vec::new();
    let mut fetched = Vec::new();
    for (name, version) in tools {
        let name = name.trim_matches('"');
        let Some(version) = version.as_str() else {
            panic!("{name} is pinned to one exact version")
        };
        if let Some(crate_name) = name.strip_prefix("cargo:") {
            crates.push(format!("{crate_name}@{version}"));
        } else if !NAMED_BY_NO_JOB.contains(&name) {
            fetched.push(name.to_owned());
        }
    }
    crates.sort();
    fetched.sort();
    (crates, fetched)
}

/// Every tool named to mise by a workflow, once each.
fn mise_installed() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for text in workflow_sources() {
        for line in text.lines() {
            let Some((_, listed)) = line.split_once("mise-tools:") else {
                continue;
            };
            found.extend(listed.split_whitespace().map(ToOwned::to_owned));
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Every `.yml` under `.github`, read once.
fn workflow_sources() -> Vec<String> {
    let mut found = Vec::new();
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
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            found.push(text);
        }
    }
    found
}

/// Every crate the pipeline installs through the pinned installer, once each.
fn installed() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for text in workflow_sources() {
        for line in text.lines() {
            if line.contains("mise-tools:") {
                continue;
            }
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
    let (crates, fetched) = pinned();
    let installed = installed();
    let by_mise = mise_installed();
    assert!(crates.len() > 4, "mise pins the crates: {crates:?}");
    assert!(
        installed.len() > 4,
        "the pipeline installs them: {installed:?}"
    );
    let adrift: Vec<&String> = installed
        .iter()
        .filter(|tool| !crates.contains(tool))
        .collect();
    assert!(
        adrift.is_empty(),
        "the pipeline hands install-action something mise.toml does not pin as a crate. \
         Either the version drifted, or the tool is pinned without a `cargo:` prefix and so \
         is one mise fetches rather than a crate — install-action would look for a crates.io \
         name it may not have, at a version it carries its own list of. Name it under \
         `mise-tools:` instead: {adrift:?} against {crates:?}"
    );
    let unpinned: Vec<&String> = by_mise
        .iter()
        .filter(|tool| !fetched.contains(tool))
        .collect();
    assert!(
        unpinned.is_empty(),
        "the pipeline asks mise for a tool mise.toml does not pin, so the runner resolves \
         a version nothing in this tree names: {unpinned:?} against {fetched:?}"
    );
    for unverified in ["curl ", "wget ", "Invoke-WebRequest", "| tar"] {
        assert!(
            !source.contains(unverified),
            "CI downloads an executable without an independently pinned digest ({unverified:?}): {source}"
        );
    }
}

/// Every program `mise.toml`'s `[env]` names, which activating mise puts into a job whether or not the runner has it.
fn environment_programs() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("mise.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let table = text
        .parse::<toml::Table>()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let Some(toml::Value::Table(environment)) = table.get("env") else {
        return Vec::new();
    };
    let Some(toml::Value::Table(tools)) = table.get("tools") else {
        panic!("mise.toml declares the tools it pins")
    };
    let pinned: Vec<String> = tools
        .keys()
        .map(|name| name.trim_matches('"').to_owned())
        .collect();
    environment
        .iter()
        .filter_map(|(variable, value)| {
            let named = value.as_str()?;
            pinned
                .iter()
                .find(|one| {
                    named
                        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                        .any(|word| word == one.as_str())
                })
                .map(|program| (variable.clone(), program.clone()))
        })
        .collect()
}

#[test]
fn a_program_the_activated_environment_names_is_one_every_job_has() {
    let setup = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/actions/setup-rust/action.yml");
    let source = std::fs::read_to_string(&setup)
        .unwrap_or_else(|error| panic!("{}: {error}", setup.display()));
    assert!(
        source.contains("jdx/mise-action"),
        "this law is about what activating mise brings with it"
    );
    let unaccounted: Vec<String> = environment_programs()
        .into_iter()
        .filter(|(variable, program)| {
            let installed = source.contains(&format!("mise install {program}"));
            let cleared = source.contains(&format!("{variable}=\" >>"));
            !installed && !cleared
        })
        .map(|(variable, program)| format!("{variable}={program}"))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "the setup action activates mise, so every job inherits `[env]` from mise.toml. \
         A variable naming a program the runner does not have fails every cargo invocation \
         in that job, including one that only reads metadata, with `could not execute \
         process ... (never executed)`. The action that activates the environment either \
         installs the program or clears the variable, and says which: {unaccounted:?}"
    );
}
