// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the repository was when the run started, or that it could not be asked.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use njutest_cli::git;
use njutest_cli::report::UNAVAILABLE;
use njutest_cli::trace::Recorder;
use njutest_cli::watch::Watch;
use rust_mutants::runner::Cancel;

fn env() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(key, _)| {
            ["PATH", "HOME"]
                .iter()
                .any(|name| njutest_devkit::paths::same_name(key, std::ffi::OsStr::new(name)))
        })
        .collect()
}

fn ask(root: &Path) -> njutest_cli::report::Git {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    git::describe(&git::Asked {
        root,
        env: &env(),
        excluded: &excluded(),
        watch: Watch::new(&cancel, &trace),
    })
}

/// What no verification reads, for a project that has said nothing about where it writes.
fn excluded() -> njutest_cli::evidence::tree::Excluded {
    njutest_cli::evidence::tree::Excluded::beside(
        &njutest_cli::config::Config::default().reports.directory,
    )
}

/// A repository with one commit, built with git's own plumbing so the fixture is the same on every machine: `commit-tree` writes a commit without consulting anybody's configuration, and the identity comes from the environment rather than from a global file.
fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let run = |args: &[&str]| -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env_clear()
            .envs(env())
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", "2026-09-05T00:00:00Z")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_DATE", "2026-09-05T00:00:00Z")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    run(&["init", "--initial-branch=main"]);
    std::fs::write(dir.path().join("a.txt"), b"one\n").expect("a file");
    run(&["add", "a.txt"]);
    let tree = run(&["write-tree"]);
    let commit = run(&["commit-tree", &tree, "-m", "one"]);
    run(&["update-ref", "refs/heads/main", &commit]);
    run(&["symbolic-ref", "HEAD", "refs/heads/main"]);
    dir
}

#[test]
fn a_clean_repository_names_its_commit_and_its_branch() {
    let dir = repository();
    let facts = ask(dir.path());

    let said = facts.said().expect("git was asked and answered");
    assert_eq!(said.commit.len(), 40, "{}", said.commit);
    assert!(
        said.commit.chars().all(|c| c.is_ascii_hexdigit()),
        "{}",
        said.commit
    );
    assert_eq!(said.branch, "main");
    assert!(!said.dirty, "nothing was changed after the commit");
}

#[test]
fn a_tree_with_uncommitted_work_says_so() {
    let dir = repository();
    std::fs::write(dir.path().join("a.txt"), b"two\n").expect("a change");
    assert!(
        ask(dir.path()).dirty(),
        "a report that named the commit without saying the tree differed \
         would name code that was never verified"
    );
}

#[test]
fn an_untracked_file_makes_the_tree_dirty_too() {
    let dir = repository();
    std::fs::write(dir.path().join("b.txt"), b"new\n").expect("a new file");
    assert!(
        ask(dir.path()).dirty(),
        "a file git has never seen is still part of what was built"
    );
}

#[test]
fn a_directory_that_is_not_a_repository_is_unavailable_and_not_empty() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let facts = ask(dir.path());

    assert!(facts.said().is_none());
    assert_eq!(facts.commit(), UNAVAILABLE);
    assert_eq!(facts.branch(), UNAVAILABLE);
    assert!(!facts.dirty());
    assert!(facts.against().is_none());
}

#[test]
fn a_machine_without_git_is_unavailable_rather_than_a_failure() {
    let dir = repository();
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let facts = git::describe(&git::Asked {
        root: dir.path(),
        env: &[(OsString::from("PATH"), OsString::from("/nonexistent"))],
        excluded: &excluded(),
        watch: Watch::new(&cancel, &trace),
    });
    assert!(facts.said().is_none());
    assert_eq!(facts.commit(), UNAVAILABLE);
}
