// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What git is asked, and what it is never allowed to say by silence.

use std::ffi::OsString;

use njutest_devkit::repo::Repo;
use rust_mutants::git::{Asking, Change, DEFAULT_BASE, Facts, changed, facts};
use rust_mutants::runner::{Cancel, Watched};
use rust_mutants::trace::Recorder;

fn cancel() -> &'static Cancel {
    static CANCEL: std::sync::OnceLock<Cancel> = std::sync::OnceLock::new();
    CANCEL.get_or_init(Cancel::new)
}

fn recorder() -> &'static Recorder {
    static RECORDER: std::sync::OnceLock<Recorder> = std::sync::OnceLock::new();
    RECORDER.get_or_init(Recorder::disabled)
}

fn environment() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "USER" | "TMPDIR"
            )
        })
        .collect()
}

struct Asked {
    repo: Repo,
    env: Vec<(OsString, OsString)>,
    watch: Watched<'static>,
    excluded: Vec<&'static str>,
}

impl Asked {
    fn new(excluded: Vec<&'static str>) -> Self {
        Self {
            repo: Repo::new(),
            env: environment(),
            watch: Watched::new(cancel(), recorder()),
            excluded,
        }
    }

    fn asking(&self) -> Asking<'_, Watched<'_>> {
        Asking {
            root: self.repo.root(),
            env: &self.env,
            excluded: &self.excluded,
            watch: &self.watch,
        }
    }

    fn facts(&self) -> Option<Facts> {
        facts(&self.asking())
    }

    fn changed(&self, base: &str) -> Option<Change> {
        changed(&self.asking(), base)
    }
}

#[test]
fn a_committed_tree_names_its_commit_and_its_branch_and_is_clean() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();

    let facts = asked.facts().expect("git can be asked");
    assert_eq!(facts.commit.len(), 40, "{facts:?}");
    assert!(
        facts.commit.chars().all(|c| c.is_ascii_hexdigit()),
        "{facts:?}"
    );
    assert!(!facts.branch.is_empty(), "{facts:?}");
    assert!(!facts.dirty, "{facts:?}");
}

#[test]
fn a_tree_written_since_its_commit_is_dirty() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();
    asked.repo.write("src/lib.rs", "pub fn f() -> i32 { 1 }\n");

    let facts = asked.facts().expect("git can be asked");
    assert!(facts.dirty, "{facts:?}");
}

#[test]
fn a_tree_that_is_not_a_repository_states_no_facts() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    assert!(asked.facts().is_none());
}

#[test]
fn a_tree_with_nothing_changed_has_an_empty_change_set() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();

    let change = asked.changed(DEFAULT_BASE).expect("git can be asked");
    assert_eq!(change.base, DEFAULT_BASE);
    assert!(change.files.is_empty(), "{change:?}");
}

#[test]
fn a_file_written_since_the_commit_is_in_the_change_set_before_it_is_committed() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();
    asked.repo.write("src/lib.rs", "pub fn f() -> i32 { 1 }\n");
    asked.repo.write("src/added.rs", "pub fn g() {}\n");

    let change = asked.changed(DEFAULT_BASE).expect("git can be asked");
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
fn a_directory_the_caller_excluded_is_not_a_change_to_the_code() {
    let asked = Asked::new(vec!["reports"]);
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();
    asked.repo.write("reports/latest.json", "{}\n");
    asked.repo.write("src/added.rs", "pub fn g() {}\n");

    let change = asked.changed(DEFAULT_BASE).expect("git can be asked");
    assert_eq!(change.files, vec!["src/added.rs".to_owned()], "{change:?}");
}

#[test]
fn a_tree_that_is_not_a_repository_says_nothing_rather_than_saying_nothing_changed() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    assert!(
        asked.changed(DEFAULT_BASE).is_none(),
        "a run that could not see what changed must not look like one that saw nothing change"
    );
}

#[test]
fn a_revision_git_does_not_know_is_not_an_empty_change_set() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();
    assert!(asked.changed("no-such-revision").is_none());
}

#[test]
fn a_change_set_mutates_within_the_rust_files_it_names() {
    let change = Change {
        base: DEFAULT_BASE.to_owned(),
        merge_base: None,
        files: vec![
            "Cargo.toml".to_owned(),
            "src/lib.rs".to_owned(),
            "src/app/run.rs".to_owned(),
        ],
    };
    let within = rust_mutants::git::within(&change, &[]);
    assert_eq!(within.len(), 2, "{within:?}");
    assert!(within.iter().any(|pattern| pattern.matches("src/lib.rs")));
    assert!(
        within
            .iter()
            .any(|pattern| pattern.matches("src/app/run.rs"))
    );
    assert!(!within.iter().any(|pattern| pattern.matches("src/other.rs")));
}

#[test]
fn a_change_set_that_names_no_rust_file_mutates_nothing_rather_than_everything() {
    let change = Change {
        base: DEFAULT_BASE.to_owned(),
        merge_base: None,
        files: vec!["README.md".to_owned()],
    };
    let within = rust_mutants::git::within(&change, &[]);
    assert_eq!(within.len(), 1, "{within:?}");
    assert!(!within.iter().any(|pattern| pattern.matches("src/lib.rs")));
}

#[test]
fn a_change_set_narrows_what_the_caller_already_selected_rather_than_widening_it() {
    let change = Change {
        base: DEFAULT_BASE.to_owned(),
        merge_base: None,
        files: vec!["src/lib.rs".to_owned(), "other/lib.rs".to_owned()],
    };
    let include = vec![rust_mutants::glob::Pattern::compile("src/**").expect("a pattern")];
    let within = rust_mutants::git::within(&change, &include);
    assert_eq!(within.len(), 1, "{within:?}");
    assert!(within.iter().any(|pattern| pattern.matches("src/lib.rs")));
    assert!(!within.iter().any(|pattern| pattern.matches("other/lib.rs")));
}
