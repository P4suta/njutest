// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run about a change set sees: which files differ from a revision, committed and not.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]
use njutest::git::{DEFAULT_BASE, changed};
use njutest::trace::Recorder;
use njutest::watch::Watch;
use njutest_devkit::repo::Repo;
use rust_mutants::runner::Cancel;

fn seen(repo: &Repo, base: &str) -> Option<njutest::git::Change> {
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let env: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter(|(key, _)| {
            ["PATH", "HOME", "USER", "TMPDIR"]
                .iter()
                .any(|name| njutest_devkit::paths::same_name(key, std::ffi::OsStr::new(name)))
        })
        .collect();
    changed(
        &njutest::git::Asked {
            root: repo.root(),
            env: &env,
            excluded: &njutest::evidence::tree::Excluded::beside(
                njutest::config::Config::default()
                    .reports
                    .directory
                    .as_path(),
            )
            .expect("the default report path is valid UTF-8"),
            watch: Watch::new(&cancel, &trace),
        },
        base,
    )
}

#[test]
fn a_tree_with_nothing_changed_has_an_empty_change_set() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    repo.commit();
    let change = seen(&repo, DEFAULT_BASE).expect("git can be asked");
    assert_eq!(change.base, DEFAULT_BASE);
    assert!(change.files.is_empty(), "{change:?}");
}

#[test]
fn a_file_written_since_the_commit_is_in_the_change_set_before_it_is_committed() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    repo.commit();
    repo.write("src/lib.rs", "pub fn f() -> i32 { 1 }\n");
    repo.write("src/added.rs", "pub fn g() {}\n");

    let change = seen(&repo, DEFAULT_BASE).expect("git can be asked");
    assert!(
        change.files.contains(&"src/lib.rs".to_owned()),
        "{change:?}"
    );
    assert!(
        change.files.contains(&"src/added.rs".to_owned()),
        "a file git does not track yet is still a file that changed: {change:?}"
    );
    let mut sorted = change.files.clone();
    sorted.sort();
    assert_eq!(change.files, sorted, "the change set is in a fixed order");
}

#[test]
fn a_tree_that_is_not_a_repository_says_nothing_rather_than_saying_nothing_changed() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    assert!(
        seen(&repo, DEFAULT_BASE).is_none(),
        "a run that could not see what changed must not look like one that saw nothing change"
    );
}

#[test]
fn a_revision_git_does_not_know_is_not_an_empty_change_set() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    repo.commit();
    assert!(seen(&repo, "no-such-revision").is_none());
}
