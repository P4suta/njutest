// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Nothing a build writes is committed, whichever way it came to be added.

#![expect(
    clippy::expect_used,
    reason = "a repository that cannot be made leaves no gate to test"
)]

use std::path::Path;
use std::process::Command;

use xtask::gates;

/// Runs git in `root` with nothing in the environment pointing it at another repository.
fn git(root: &Path, arguments: &[&str]) -> std::process::Output {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(arguments);
    for variable in gates::REDIRECTING_GIT {
        command.env_remove(variable);
    }
    command.output().expect("git runs")
}

/// A repository whose index holds `files`, each added past any ignore rule.
fn holding(files: &[&str]) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a directory");
    for file in files {
        let path = root.path().join(file);
        std::fs::create_dir_all(path.parent().expect("a file has a directory"))
            .expect("its directory");
        std::fs::write(&path, "x").expect("the file");
    }
    for arguments in [&["init", "-q"][..], &["add", "-f", "--", "."][..]] {
        let done = git(root.path(), arguments);
        assert!(
            done.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8(done.stderr).expect("git diagnostics are UTF-8")
        );
    }
    root
}

#[test]
fn build_output_under_any_target_directory_is_refused_however_it_was_added() {
    let root = holding(&[
        "src/lib.rs",
        "crates/njutest-macros/target/tests/trybuild/CACHEDIR.TAG",
        "crates/njutest-macros/target/debug/build/x.o",
    ]);
    let refused = gates::tracked(root.path())
        .expect_err("two committed files a build wrote, force-added past .gitignore");
    let said = refused.to_string();
    assert!(
        said.contains("crates/njutest-macros/target/ (2)") && said.contains("git rm -r --cached"),
        "the refusal names the directory a build wrote, how much of it is committed, and how \
         it comes out of the index: {said}"
    );
}

#[test]
fn a_name_that_only_looks_like_build_output_is_not_refused() {
    let root = holding(&[
        "fixtures/fixture-targets/src/lib.rs",
        "crates/njutest/src/targets.rs",
        "docs/target",
    ]);
    let passed = gates::tracked(root.path()).expect("nothing lies under a target directory");
    assert!(
        passed.contains("3 tracked paths"),
        "a file named `target`, and a directory whose name only starts with it, are sources: \
         {passed}"
    );
}
