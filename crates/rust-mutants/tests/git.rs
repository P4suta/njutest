// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What git is asked, and what it is never allowed to say by silence.

use std::ffi::OsString;

use njutest_devkit::repo::Repo;
use njutest_devkit::result::{
    OptionState::Present,
    ResultState::{Refused, Returned},
    option_state, result_state,
};
use rust_mutants::git::{
    Asking, Change, DEFAULT_BASE, Facts, Lines, Touched, changed, facts, lines,
};
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
            ["PATH", "HOME", "USER", "TMPDIR"]
                .iter()
                .any(|name| rust_mutants::vars::same_name(key, std::ffi::OsStr::new(name)))
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

    fn lines(&self, base: &str) -> Option<Lines> {
        lines(&self.asking(), base)
    }
}

/// Twenty lines, each saying which it is, with the ones in `edited` rewritten and the ones in `removed` gone.
fn twenty(edited: &[u32], removed: &[u32]) -> String {
    (1..=20_u32)
        .filter(|line| !removed.contains(line))
        .map(|line| {
            if edited.contains(&line) {
                format!("let edited_{line} = {line};\n")
            } else {
                format!("let line_{line} = {line};\n")
            }
        })
        .collect()
}

#[test]
fn a_committed_tree_names_its_commit_and_its_branch_and_is_clean() {
    let asked = Asked::new(Vec::new());
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();

    let facts = asked.facts();
    assert_eq!(option_state(facts.as_ref()), Present, "git facts");
    let Some(facts) = facts else { return };
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

    let facts = asked.facts();
    assert_eq!(option_state(facts.as_ref()), Present, "git facts");
    let Some(facts) = facts else { return };
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

    let change = asked.changed(DEFAULT_BASE);
    assert_eq!(option_state(change.as_ref()), Present, "git change set");
    let Some(change) = change else { return };
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

    let change = asked.changed(DEFAULT_BASE);
    assert_eq!(option_state(change.as_ref()), Present, "git change set");
    let Some(change) = change else { return };
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
    let asked = Asked::new(vec!["artifacts"]);
    asked.repo.package("demo").lib("pub fn f() {}\n");
    asked.repo.commit();
    asked.repo.write("artifacts/latest.json", "{}\n");
    asked.repo.write("src/added.rs", "pub fn g() {}\n");

    let change = asked.changed(DEFAULT_BASE);
    assert_eq!(option_state(change.as_ref()), Present, "git change set");
    let Some(change) = change else { return };
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
    assert_eq!(result_state(&within), Returned, "changed paths: {within:?}");
    let Ok(rust_mutants::git::Within::Changed(within)) = within else {
        panic!("two Rust files changed: {within:?}")
    };
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
    assert_eq!(
        within,
        Ok(rust_mutants::git::Within::Nothing {
            changed: vec!["README.md".to_owned()],
        }),
        "a change that names no Rust file says so, with what did change"
    );
}

#[test]
fn a_change_set_narrows_what_the_caller_already_selected_rather_than_widening_it() {
    let change = Change {
        base: DEFAULT_BASE.to_owned(),
        merge_base: None,
        files: vec!["src/lib.rs".to_owned(), "other/lib.rs".to_owned()],
    };
    let pattern = rust_mutants::glob::Pattern::compile("src/**");
    assert_eq!(result_state(&pattern), Returned, "include: {pattern:?}");
    let Ok(pattern) = pattern else { return };
    let include = vec![pattern];
    let within = rust_mutants::git::within(&change, &include);
    assert_eq!(result_state(&within), Returned, "changed paths: {within:?}");
    let Ok(rust_mutants::git::Within::Changed(within)) = within else {
        panic!("one included Rust file changed: {within:?}")
    };
    assert_eq!(within.len(), 1, "{within:?}");
    assert!(within.iter().any(|pattern| pattern.matches("src/lib.rs")));
    assert!(!within.iter().any(|pattern| pattern.matches("other/lib.rs")));
}

#[test]
fn an_unrepresentable_changed_path_is_not_silently_omitted() {
    let change = Change {
        base: DEFAULT_BASE.to_owned(),
        merge_base: None,
        files: vec!["src//lib.rs".to_owned()],
    };
    let error = rust_mutants::git::within(&change, &[]);
    assert_eq!(
        result_state(&error),
        Refused,
        "invalid changed path: {error:?}"
    );
    let Err(error) = error else { return };
    assert_eq!(error.pattern, "src//lib.rs");
}

#[test]
fn a_run_asks_about_the_tree_it_was_given_and_not_the_one_its_caller_was_in() {
    let elsewhere = Repo::new();
    elsewhere.write("src/lib.rs", "pub fn one() {}\n");
    elsewhere.commit();

    let here = Repo::new();
    here.write("src/lib.rs", "pub fn two() {}\n");
    here.commit();

    let mut env = environment();
    env.push((
        OsString::from("GIT_DIR"),
        OsString::from(elsewhere.root().join(".git")),
    ));
    env.push((
        OsString::from("GIT_WORK_TREE"),
        OsString::from(elsewhere.root()),
    ));

    let watch = Watched::new(cancel(), recorder());
    let asked = facts(&Asking {
        root: here.root(),
        env: &env,
        excluded: &[],
        watch: &watch,
    });
    assert_eq!(
        option_state(asked.as_ref()),
        Present,
        "asked repository facts"
    );
    let Some(asked) = asked else { return };

    let mine = facts(&Asking {
        root: here.root(),
        env: &environment(),
        excluded: &[],
        watch: &watch,
    });
    assert_eq!(
        option_state(mine.as_ref()),
        Present,
        "local repository facts"
    );
    let Some(mine) = mine else { return };

    assert_eq!(
        asked.commit, mine.commit,
        "a run names the commit of the tree it verified. `GIT_DIR` in the environment \
         it happened to be started with points git somewhere else, and a report that \
         followed it would name another repository's commit as the thing it \
         established something about — which is a conclusion drawn from how the run \
         was invoked rather than from what it looked at"
    );
}

#[test]
fn a_change_names_the_lines_it_left_and_every_line_of_a_file_git_does_not_track() {
    let asked = Asked::new(Vec::new());
    asked.repo.write("src/lib.rs", &twenty(&[], &[]));
    asked.repo.commit();
    asked.repo.write("src/lib.rs", &twenty(&[5, 12, 13], &[18]));
    asked.repo.write("src/added.rs", "pub fn g() {}\n");

    let said = asked.lines(DEFAULT_BASE);
    assert_eq!(option_state(said.as_ref()), Present, "the changed lines");
    let Some(said) = said else { return };
    assert_eq!(
        said.files.get("src/lib.rs"),
        Some(&Touched::Ranges(vec![(5, 5), (12, 13)])),
        "an edited line is named where the new file has it, and a removed one names no \
         line, because no line of the new file is it: {said:?}"
    );
    assert_eq!(
        said.files.get("src/added.rs"),
        Some(&Touched::Whole),
        "{said:?}"
    );
    assert!(
        said.touches("src/lib.rs", 12)
            && !said.touches("src/lib.rs", 11)
            && said.touches("src/added.rs", 1)
            && !said.touches("src/other.rs", 1),
        "{said:?}"
    );
}

#[test]
fn a_line_that_reads_like_a_header_is_what_the_file_says() {
    let asked = Asked::new(Vec::new());
    asked.repo.write("src/lib.rs", "let a = 1;\n");
    asked.repo.commit();
    asked.repo.write(
        "src/lib.rs",
        "let a = 1;\n++ b/src/elsewhere.rs\n@@ -1 +40,2 @@\n",
    );

    let said = asked.lines(DEFAULT_BASE);
    assert_eq!(option_state(said.as_ref()), Present, "the changed lines");
    let Some(said) = said else { return };
    assert_eq!(
        said.files,
        std::collections::BTreeMap::from([(
            "src/lib.rs".to_owned(),
            Touched::Ranges(vec![(2, 3)])
        )]),
        "an added line is content wherever it starts, so it names no file and no lines"
    );
}

#[test]
fn a_root_inside_the_repository_reads_the_lines_under_it_by_its_own_paths() {
    let asked = Asked::new(Vec::new());
    asked
        .repo
        .write("crates/demo/src/lib.rs", &twenty(&[], &[]));
    asked.repo.write("other/src/lib.rs", &twenty(&[], &[]));
    asked.repo.commit();
    asked
        .repo
        .write("crates/demo/src/lib.rs", &twenty(&[7], &[]));
    asked.repo.write("other/src/lib.rs", &twenty(&[7], &[]));

    let root = asked.repo.root().join("crates/demo");
    let said = lines(
        &Asking {
            root: &root,
            env: &asked.env,
            excluded: &asked.excluded,
            watch: &asked.watch,
        },
        DEFAULT_BASE,
    );
    assert_eq!(option_state(said.as_ref()), Present, "the changed lines");
    let Some(said) = said else { return };
    assert_eq!(
        said.files,
        std::collections::BTreeMap::from([(
            "src/lib.rs".to_owned(),
            Touched::Ranges(vec![(7, 7)])
        )]),
        "a report names files from the root it measured, so the lines are named from it \
         too, and a file outside it is not a file the report can name"
    );
}

#[test]
fn a_revision_git_does_not_know_names_no_lines_rather_than_none_changed() {
    let asked = Asked::new(Vec::new());
    asked.repo.write("src/lib.rs", "let a = 1;\n");
    asked.repo.commit();
    assert!(
        asked.lines("no-such-revision").is_none(),
        "a diff that could not be taken is not a diff with nothing in it"
    );
}
