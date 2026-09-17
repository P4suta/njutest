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
        .flatten()
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
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
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
