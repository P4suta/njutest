// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The pre-push gate proves that the tree it checks is the exact object Git is pushing.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a fixture that cannot be constructed leaves no pre-push protocol to test"
)]

use std::ffi::OsString;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use njutest_devkit::process::SupervisedChild;
use njutest_devkit::result::{ResultState, result_state};

struct Repository {
    directory: tempfile::TempDir,
    _commands: tempfile::TempDir,
    scratch: tempfile::TempDir,
    head: String,
    path: OsString,
}

impl Repository {
    fn new(check: &str) -> Self {
        let directory = tempfile::tempdir().expect("a temporary repository");
        command(directory.path(), "git", &["init", "--quiet"]);
        command(
            directory.path(),
            "git",
            &["config", "user.name", "pre-push-test"],
        );
        command(
            directory.path(),
            "git",
            &["config", "user.email", "pre-push@example.invalid"],
        );
        command(
            directory.path(),
            "git",
            &["config", "commit.gpgsign", "false"],
        );
        std::fs::write(
            directory.path().join(".gitignore"),
            "/target/\nlocal-only\n",
        )
        .expect("an ignored local-input name");
        std::fs::write(directory.path().join("tracked"), "before\n").expect("a tracked file");
        command(directory.path(), "git", &["add", ".gitignore", "tracked"]);
        command(
            directory.path(),
            "git",
            &["commit", "--quiet", "-m", "fixture"],
        );
        let head = String::from_utf8(
            isolated("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(directory.path())
                .output()
                .expect("git rev-parse")
                .stdout,
        )
        .expect("an ASCII object id")
        .trim()
        .to_owned();

        let commands = tempfile::tempdir().expect("a private bin directory");
        let bin = commands.path().to_path_buf();
        let mise = bin.join("mise");
        std::fs::write(
            &mise,
            format!("#!/usr/bin/env bash\nset -euo pipefail\n{check}\n"),
        )
        .expect("a scripted check");
        executable(&mise);
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths).expect("a PATH for the scripted check");

        Self {
            directory,
            _commands: commands,
            scratch: tempfile::tempdir().expect("a temporary directory the gate keeps its tree in"),
            head,
            path,
        }
    }

    fn push(&self, local: &str) -> Output {
        self.push_over(local, &"0".repeat(40))
    }

    fn push_over(&self, local: &str, remote: &str) -> Output {
        self.push_terminated(local, remote, "\n")
    }

    fn push_terminated(&self, local: &str, remote: &str, end: &str) -> Output {
        self.push_with(&update(local, remote, end), &[])
    }

    fn push_as_a_hook(&self, local: &str) -> Output {
        let git = self.directory.path().join(".git");
        self.push_with(
            &update(local, &"0".repeat(40), "\n"),
            &[("GIT_DIR", git.as_os_str()), ("GIT_PREFIX", "".as_ref())],
        )
    }

    fn push_with(&self, line: &str, handed: &[(&str, &std::ffi::OsStr)]) -> Output {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace root")
            .join("scripts/pre-push-check.sh");
        let mut command = isolated("bash");
        command
            .arg(script)
            .current_dir(self.directory.path())
            .env("PATH", &self.path)
            .env("TMPDIR", self.scratch.path())
            .envs(handed.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = SupervisedChild::launch(&mut command).expect("the pre-push gate");
        child
            .take_stdin()
            .expect("the gate's stdin")
            .write_all(line.as_bytes())
            .expect("a ref update");
        child.wait_with_output().expect("the gate's answer")
    }
}

fn update(local: &str, remote: &str, end: &str) -> String {
    format!("refs/heads/local {local} refs/heads/remote {remote}{end}")
}

fn symbolic_head(directory: &Path) -> String {
    String::from_utf8(
        isolated("git")
            .args(["symbolic-ref", "--quiet", "HEAD"])
            .current_dir(directory)
            .output()
            .expect("git symbolic-ref")
            .stdout,
    )
    .expect("an ASCII ref name")
    .trim()
    .to_owned()
}

fn object_id(directory: &Path, revision: &str) -> String {
    String::from_utf8(
        isolated("git")
            .args(["rev-parse", revision])
            .current_dir(directory)
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("an ASCII object id")
    .trim()
    .to_owned()
}

/// A command with none of the `GIT_*` variables a hook hands down, so it touches only the repository it runs in.
fn isolated(program: &str) -> Command {
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"GIT_") {
            command.env_remove(name);
        }
    }
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}

fn command(directory: &Path, program: &str, arguments: &[&str]) {
    let status = isolated(program)
        .args(arguments)
        .current_dir(directory)
        .status()
        .expect("the fixture command");
    assert!(status.success(), "{program} {arguments:?}: {status}");
}

fn executable(path: &Path) {
    let mut permissions = std::fs::metadata(path)
        .expect("the scripted check")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("an executable scripted check");
}

fn stderr(output: &Output) -> String {
    let decoded = String::from_utf8(output.stderr.clone());
    if let Err(error) = &decoded {
        assert_eq!(
            result_state(&decoded),
            ResultState::Returned,
            "the hook wrote non-UTF-8 stderr; bytes: {:02x?}",
            error.as_bytes()
        );
    }
    match decoded {
        Ok(stderr) => stderr,
        Err(_already_reported) => String::new(),
    }
}

#[test]
fn a_clean_exact_head_is_checked() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    let output = repository.push(&repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn the_check_is_handed_none_of_the_hooks_git_environment() {
    let repository = Repository::new(
        "if compgen -e | grep -q '^GIT_'; then compgen -e | grep '^GIT_' >&2; exit 98; fi",
    );
    let output = repository.push_as_a_hook(&repository.head);
    assert!(
        output.status.success(),
        "a check that inherits GIT_DIR answers about the repository being pushed, and a test \
         inside it that runs `git init` or `git config` rewrites that repository: {}",
        stderr(&output)
    );
}

#[test]
fn a_push_leaves_the_pushers_head_on_its_branch() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    let before = symbolic_head(repository.directory.path());
    for push in [
        "the one that makes the gate's tree",
        "the one that reuses it",
    ] {
        let output = repository.push_as_a_hook(&repository.head);
        assert!(output.status.success(), "{push}: {}", stderr(&output));
    }
    assert_eq!(
        symbolic_head(repository.directory.path()),
        before,
        "the gate's own checkout ran with the hook's GIT_DIR and moved the pusher's HEAD instead \
         of the isolated tree's"
    );
}

#[test]
fn a_different_object_is_refused_before_the_check() {
    let repository = Repository::new("exit 99");
    let output = repository.push(&"1".repeat(40));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("but the checked-out commit is"));
}

#[test]
fn an_existing_remote_ancestor_is_accepted() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    std::fs::write(repository.directory.path().join("tracked"), "after\n")
        .expect("a second revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "fast-forward"],
    );
    let local = object_id(repository.directory.path(), "HEAD");

    let output = repository.push_over(&local, &repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_ref_update_nothing_terminates_is_the_same_ref_update() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    std::fs::write(repository.directory.path().join("tracked"), "after\n")
        .expect("a second revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "fast-forward"],
    );
    let local = object_id(repository.directory.path(), "HEAD");

    let output = repository.push_terminated(&local, &repository.head, "");
    assert!(
        output.status.success(),
        "the dispatcher that feeds this gate captures the ref list with `$(cat)` and \
         replays it with `printf '%s'`, so the last line arrives with nothing after it. \
         A gate that drops it sees an empty push and refuses every one of them, which is \
         what it did: {}",
        stderr(&output)
    );
}

#[test]
fn a_non_fast_forward_update_is_refused_before_the_check() {
    let repository = Repository::new("exit 99");
    command(
        repository.directory.path(),
        "git",
        &["branch", "remote", "HEAD"],
    );
    std::fs::write(repository.directory.path().join("tracked"), "local\n")
        .expect("the local revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "local"],
    );
    let local = object_id(repository.directory.path(), "HEAD");
    command(
        repository.directory.path(),
        "git",
        &["checkout", "--quiet", "remote"],
    );
    std::fs::write(repository.directory.path().join("tracked"), "remote\n")
        .expect("the remote revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "remote"],
    );
    let remote = object_id(repository.directory.path(), "HEAD");
    command(
        repository.directory.path(),
        "git",
        &["checkout", "--quiet", "--detach", &local],
    );

    let output = repository.push_over(&local, &remote);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("non-fast-forward updates are forbidden"));
}

#[test]
fn an_unknown_remote_commit_is_refused_before_the_check() {
    let repository = Repository::new("exit 99");
    let output = repository.push_over(&repository.head, &"1".repeat(40));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("which is not present locally"));
}

#[test]
fn tracked_adjacent_and_ignored_edits_cannot_contaminate_the_isolated_check() {
    let repository = Repository::new(
        "test ! -e local-only && test ! -e adjacent && test \"$(cat tracked)\" = before",
    );
    std::fs::write(repository.directory.path().join("tracked"), "after\n")
        .expect("an uncommitted tracked edit");
    std::fs::write(
        repository.directory.path().join("local-only"),
        "ignored input\n",
    )
    .expect("an ignored file");
    std::fs::write(
        repository.directory.path().join("adjacent"),
        "untracked input\n",
    )
    .expect("an adjacent file");
    let output = repository.push(&repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_tree_changed_by_the_check_is_refused_afterwards() {
    let repository = Repository::new("printf 'after\\n' >> tracked");
    let output = repository.push(&repository.head);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("isolated check changed the tree"));
}

#[test]
fn a_test_push_leaves_no_tree_in_the_shared_temporary_directory() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    let output = repository.push(&repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
    let toplevel = String::from_utf8(
        isolated("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(repository.directory.path())
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("an ASCII path")
    .trim()
    .to_owned();
    let key = String::from_utf8(
        isolated("sh")
            .args([
                "-c",
                "printf '%s' \"$1\" | shasum | cut -c1-12",
                "_",
                &toplevel,
            ])
            .output()
            .expect("shasum")
            .stdout,
    )
    .expect("an ASCII key")
    .trim()
    .to_owned();
    let shared = std::env::temp_dir().join(format!("njutest-pre-push-{key}"));
    let left = shared
        .try_exists()
        .expect("the shared temporary directory can be read");
    assert!(
        !left,
        "the gate keeps one tree per repository on purpose, as the cache the next push reuses, \
         and a test's repository is new every run, so a tree left in the shared directory is \
         one more per run that nothing ever removes: {}",
        shared.display()
    );
}
