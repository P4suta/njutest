// SPDX-FileCopyrightText: 2026 njutest contributors
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

/// The jobs one workflow declares, by the two-space indent a job name carries.
fn jobs_of(workflow: &str) -> BTreeSet<String> {
    let text = read(&format!(".github/workflows/{workflow}"));
    let mut jobs = BTreeSet::new();
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
    jobs
}

/// What the `ci-success` job of `ci.yml` waits for, in either shape a workflow writes a list.
fn required() -> BTreeSet<String> {
    let text = read(".github/workflows/ci.yml");
    let block = text
        .split_once("\n  ci-success:")
        .map_or_else(String::new, |(_before, after)| after.to_owned());
    let after = block
        .split_once("needs:")
        .map_or_else(String::new, |(_before, after)| after.to_owned());
    if let Some((inside, _rest)) = after.trim_start().strip_prefix('[').and_then(|rest| {
        rest.split_once(']')
            .map(|(inside, rest)| (inside.to_owned(), rest.to_owned()))
    }) {
        return inside
            .split(',')
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .collect();
    }
    after
        .lines()
        .skip(1)
        .map_while(|line| line.trim_start().strip_prefix("- ").map(str::trim))
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn every_job_of_the_pipeline_is_one_that_can_fail_it() {
    let declared = jobs_of("ci.yml");
    let waited_for = required();
    let unguarded: Vec<&String> = declared
        .iter()
        .filter(|job| job.as_str() != "ci-success" && !waited_for.contains(*job))
        .collect();
    assert!(
        unguarded.is_empty(),
        "a branch is protected by requiring one check, and these jobs are not among what \
         it waits for: they may fail every time and nothing will say so, which is the \
         same as not having written them. {unguarded:?} against {waited_for:?}"
    );
    let gone: Vec<&String> = waited_for
        .iter()
        .filter(|job| !declared.contains(*job))
        .collect();
    assert!(
        gone.is_empty(),
        "and a job it waits for that the workflow no longer declares is a name GitHub \
         resolves to nothing, which passes: {gone:?}"
    );
}

#[test]
fn every_job_ci_md_names_exists_in_the_workflow_that_would_hold_it() {
    let page = read("docs/ci.md");
    let mut jobs: BTreeSet<String> = BTreeSet::new();
    for name in workflows() {
        jobs.extend(jobs_of(&name));
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

#[test]
fn every_relative_link_in_the_documentation_resolves() {
    let root = root();
    let mut broken = Vec::new();
    for page in pages(&root) {
        let text = std::fs::read_to_string(&page).unwrap_or_default();
        for link in links(&text) {
            let Some(parent) = page.parent() else {
                continue;
            };
            if !parent.join(&link).exists() {
                broken.push(format!(
                    "{} names {link}, which is not there",
                    page.strip_prefix(&root).unwrap_or(&page).display()
                ));
            }
        }
    }
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}

/// Every Markdown page of the repository, outside what a build wrote.
fn pages(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                found.push(path);
            }
        }
    }
    found
}

/// Every relative link a page names, without its fragment.
fn links(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        let tail = rest.split_at(at).1.get(2..).unwrap_or("");
        let end = tail.find(')').unwrap_or(tail.len());
        let link = tail.get(..end).unwrap_or("");
        let path = link.split('#').next().unwrap_or("");
        if !path.is_empty()
            && !path.starts_with("http://")
            && !path.starts_with("https://")
            && !path.starts_with("mailto:")
        {
            found.push(path.to_owned());
        }
        rest = tail.get(end..).unwrap_or("");
    }
    found
}

/// Every file of the repository a link could be written in, source and page alike.
fn linking_files() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                    stack.push(path);
                }
                continue;
            }
            if path
                .extension()
                .is_some_and(|extension| extension == "rs" || extension == "md")
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Every local path one file links to, as written.
fn linked(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        let after = rest.get(at.saturating_add(2)..).unwrap_or_default();
        let end = after.find(')').unwrap_or(after.len());
        let target = after.get(..end).unwrap_or_default();
        let target = target.split('#').next().unwrap_or_default().trim();
        let looks_like_a_path = !target.is_empty()
            && !target.contains("://")
            && !target.starts_with('#')
            && !target.starts_with("mailto:")
            && !target.contains(char::is_whitespace)
            && !target.contains('"')
            && target.contains('.');
        if looks_like_a_path {
            found.insert(target.to_owned());
        }
        rest = after.get(end..).unwrap_or_default();
    }
    found
}

#[test]
fn every_page_a_file_of_this_repository_links_to_is_one_it_holds() {
    let mut dangling = Vec::new();
    for path in linking_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = path.strip_prefix(root()).unwrap_or(&path).to_owned();
        let beside = path.parent().map_or_else(root, Path::to_path_buf);
        for target in linked(&text) {
            if beside.join(&target).exists() {
                continue;
            }
            dangling.push(format!("{}: {target}", relative.display()));
        }
    }
    assert!(
        dangling.is_empty(),
        "a link to a page nobody holds is a reader following it and finding nothing; rustdoc \
         checks the links that name items and not the ones that name files:\n{}",
        dangling.join("\n")
    );
}

#[test]
fn every_page_the_documentation_holds_is_one_the_book_summary_reaches() {
    let root = root();
    let summary = read("docs/SUMMARY.md");
    let held = linked(&summary);
    let missing: Vec<String> = pages(&root.join("docs"))
        .into_iter()
        .filter_map(|page| {
            let relative = page.strip_prefix(root.join("docs")).ok()?;
            let name = relative.to_string_lossy().replace('\\', "/");
            (name != "SUMMARY.md" && !held.contains(&name)).then_some(name)
        })
        .collect();

    assert!(
        missing.is_empty(),
        "mdbook builds what the summary names and quietly leaves out what it does not, \
         so a page missing from it is one the book has no way to reach and nobody \
         notices is gone:\n{}",
        missing.join("\n")
    );
}
