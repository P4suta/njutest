// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The pipeline as the documentation describes it, against the pipeline.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "the helpers that read the repository's own files are not themselves tests"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// Every workflow file the repository commits.
fn workflows() -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(root().join(".github/workflows"))
        .expect("the workflow directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| Path::new(name).extension().is_some_and(|it| it == "yml"))
        .collect();
    found.sort();
    found
}

#[test]
fn every_workflow_file_is_named_in_ci_md() {
    let page = read("docs/ci.md");
    let missing: Vec<String> = workflows()
        .into_iter()
        .filter(|name| !page.contains(name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/ci.md does not say what these do: {missing:?}"
    );
}

#[test]
fn every_job_ci_md_names_exists_in_the_workflow_that_would_hold_it() {
    let page = read("docs/ci.md");
    let mut jobs: BTreeSet<String> = BTreeSet::new();
    for name in workflows() {
        let text = read(&format!(".github/workflows/{name}"));
        let mut in_jobs = false;
        for line in text.lines() {
            if line.starts_with("jobs:") {
                in_jobs = true;
                continue;
            }
            if !in_jobs {
                continue;
            }
            if let Some(job) = line.strip_prefix("  ")
                && let Some((job, rest)) = job.split_once(':')
                && rest.trim().is_empty()
                && !job.starts_with(' ')
                && !job.starts_with('#')
            {
                jobs.insert(job.to_owned());
            }
        }
    }
    assert!(jobs.contains("ci-success"), "{jobs:?}");
    for named in ["ci-success", "shard", "audit", "cargo-mutants"] {
        assert!(
            page.contains(named),
            "docs/ci.md does not name the {named} job"
        );
        assert!(
            jobs.contains(named),
            "docs/ci.md names a {named} job no workflow holds: {jobs:?}"
        );
    }
}
